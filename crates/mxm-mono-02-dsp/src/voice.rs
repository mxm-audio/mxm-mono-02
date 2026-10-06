//! The voice: the SH-2's signal flow, wired.
//!
//! ```text
//!  keys ─► KEYBOARD ─► key CV ─► PORTAMENTO ─┬─► VCO-1 ─[ sine|saw|square|pulse ]─┐
//!          trigger ─┬─► AUTO BEND ───────────┤     └─► SUB (÷2) ───────────────────┤
//!                   ├─► DELAY restart        └─► VCO-2 ─[ noise|saw|square|pulse ]─┼─► MIXER ─► VCF ─► VCA ─► VOLUME ─► out
//!                   └─► ENV (GATE+TRIG)          shared pulse width + Σ routes     │            ▲      ▲
//!  MODULATOR ─► MODE ─► LFO; un-delayed sine ─► LFO to width; square ─► ENV (LFO mode)    CUTOFF Σ   HOLD|ENV|GATE × AMPLITUDE Σ
//!  ENV, AUTO BEND, KEY, the performance inputs and the voice's own audio ─► routes
//!  BENDER ─► VCO-2 always, VCO-1 by its switch; its lever position is a source
//! ```
//!
//! # Modulation is routing, and the machine's wiring is the init patch
//!
//! Every modulation path the SH-2 hard-wired is a route (`crate::routing`), present at Init at zero
//! depth and at the scale its control had: the modulator on pitch and on the cutoff, the auto
//! bend's dip, the pulse-width section's LFO and envelope positions, the envelope on the cutoff in
//! either polarity, keyboard tracking, the bender on the cutoff. What stays circuit is what no route
//! that *scales* can express: the VCA switch, the trigger switch and the bender's own pitch path.
//!
//! **The machine's external input is not here** (the owner, 2026-09-26): the SH-2's EXT IN jack, its
//! fixed-level path into the mixer and the envelope follower it fed (`sh-2.md` §7.5) were removed
//! with the plugin's auxiliary input, and with them the Follower source and the ENV FOL'R route.
//!
//! `research:instruments/sh-2.md` §2 is the diagram this reproduces; `plans/plan-mxm-mono-02.md`
//! §5 is the design. Every rule the review made explicit is a test here or in the module
//! that owns it, and the constants the research does not have are `pub const`s with
//! **chosen** beside them.
//!
//! # What the trigger reaches
//!
//! The keyboard block's one trigger (`keyboard.rs`) has three consumers: the envelope in
//! GATE+TRIG mode, the modulator's delay restart, and the auto bend's restart. A higher key
//! over a held lower one produces no trigger, so none of the three moves; a lower key
//! produces one, so all three do. GATE mode triggers the envelope on the gate's rising edge
//! only; LFO mode on every rising edge of the modulator's square while the gate is high.
//!
//! # Idle is defined
//!
//! A host may stop calling a quiet plugin, so at idle the state with no target — the
//! oscillators, the modulator's phase, the noise — freezes where it is, and the state with
//! a target settles to it: the portamento lag to the last key, the delay's fade to full,
//! the auto bend's charge to nothing. A labelled deviation from hardware that never stops,
//! so that no phrase depends on how long the host slept (plan §5.6).
//!
//! # HOLD, and the panic that must beat it
//!
//! HOLD is the amplifier jammed open: the voice sounds with no key down, before the first
//! note at a defined pitch and after the last at the key CV's held pitch, and it is active
//! for as long as its output is audible — never because the switch says HOLD. All Sound
//! Off silences every mode, and in HOLD it **latches** the amplifier closed until a
//! deliberate act: a note-on, `reset`, or HOLD left and re-entered (a session that sends no notes
//! needs a way back a performer can reach).

use crate::envelope::Adsr;
use crate::filter::Ladder;
use crate::keyboard::{Keyboard, NoteId, Outcome, Owner, Press};
use crate::lfo::{Mode, Modulator, Outputs};
use crate::oscillator::{DcBlocker, Noise, Sub, Vco, Wave1, Wave2};
use crate::routing::{Graph, Routing, source, target};
use crate::{filter, flush};
use mxm_modulation::standard;

/// The RANGE rotaries: exact octave steps from a resistor chain (`sh-2.md` §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Range {
    Ft32,
    Ft16,
    #[default]
    Ft8,
    Ft4,
    Ft2,
}

impl Range {
    /// Octaves relative to 8', where a MIDI key sounds at its concert pitch.
    #[inline]
    pub fn octaves(self) -> f32 {
        match self {
            Range::Ft32 => -2.0,
            Range::Ft16 => -1.0,
            Range::Ft8 => 0.0,
            Range::Ft4 => 1.0,
            Range::Ft2 => 2.0,
        }
    }
}

/// The VCA switch (§3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VcaMode {
    Hold,
    #[default]
    Env,
    Gate,
}

/// The envelope's trigger switch (§5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriggerMode {
    #[default]
    GateTrig,
    Gate,
    Lfo,
}

/// The patch, as plain values. Amounts are `0..=1` unless said otherwise; times in
/// seconds; tunes in cents. Smoothing is the plugin's business.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    // Modulator
    pub lfo_rate_hz: f32,
    pub lfo_delay_s: f32,
    pub lfo_mode: Mode,
    // VCO, shared
    /// The width both pulses start from, `0.05..=0.5`: square at 0.5, narrowest at 0.05. Routes
    /// into [`target::PULSE_WIDTH`] add to it.
    pub pulse_width: f32,
    pub total_tune_cents: f32,
    // VCO-1
    pub vco1_range: Range,
    pub vco1_wave: Wave1,
    /// The BENDER ON/OFF switch: off, and VCO-2 alone bends.
    pub vco1_bender: bool,
    // VCO-2
    pub vco2_range: Range,
    pub vco2_wave: Wave2,
    /// The TUNE knob, already through its NARROW / WIDE range: ±200 or ±1750.
    pub vco2_tune_cents: f32,
    // Mixer
    pub sub_level: f32,
    pub vco1_level: f32,
    pub vco2_level: f32,
    // VCF
    /// A position, `0..=1`, over the hardware's range — not a frequency.
    pub cutoff: f32,
    pub resonance: f32,
    // VCA
    pub vca_mode: VcaMode,
    // ENV
    pub attack_s: f32,
    pub decay_s: f32,
    pub sustain: f32,
    pub release_s: f32,
    pub trigger_mode: TriggerMode,
    // Voice
    /// The portamento slider, `0..=5` s; zero is the only off.
    pub portamento_s: f32,
    /// The bender, in semitones, already through its VCO sensitivity and read from
    /// the owner's channel — the plugin resolves the channel, the voice does not.
    pub bend_semitones: f32,
    /// The bender's lever position, `-1..=1`, from the same channel: the Bend source.
    pub bend_position: f32,
    /// The mod wheel, `0..=1`: the Wheel source. Its push on the vibrato depth is the plugin's,
    /// added into that route's amount.
    pub mod_wheel: f32,
    /// Channel pressure, `0..=1`, from the owner's channel: the Pressure source.
    pub pressure: f32,
    pub volume: f32,
}

impl Default for Params {
    /// The init patch's shape: every amount zero, every configuration useful, one
    /// plain source, VCO-2 off unison. The plugin's defaults are the real init patch;
    /// this is what tests start from.
    fn default() -> Self {
        Self {
            lfo_rate_hz: 5.0,
            lfo_delay_s: 0.0,
            lfo_mode: Mode::Sine,
            pulse_width: 0.5,
            total_tune_cents: 0.0,
            vco1_range: Range::Ft8,
            vco1_wave: Wave1::Saw,
            vco1_bender: true,
            vco2_range: Range::Ft8,
            vco2_wave: Wave2::Saw,
            vco2_tune_cents: 7.0,
            sub_level: 0.0,
            vco1_level: 1.0,
            vco2_level: 0.0,
            cutoff: 1.0,
            resonance: 0.0,
            vca_mode: VcaMode::Env,
            attack_s: 0.002,
            decay_s: 0.3,
            sustain: 1.0,
            release_s: 0.1,
            trigger_mode: TriggerMode::GateTrig,
            portamento_s: 0.0,
            bend_semitones: 0.0,
            bend_position: 0.0,
            mod_wheel: 0.0,
            pressure: 0.0,
            volume: 1.0,
        }
    }
}

// ---- Constants the research does not have. Each is chosen and says so. ----

/// The cutoff range in octaves above [`CUTOFF_FLOOR_HZ`]: 10 Hz to about 20 kHz, the
/// specification's span. The CV sum is clamped to this range **before** the
/// exponential, which is the machine's ceiling (wart 12); the filter's own Nyquist
/// clamp sits beneath it at 44.1 kHz.
pub const CUTOFF_RANGE_OCTAVES: f32 = 11.0;
pub const CUTOFF_FLOOR_HZ: f32 = 10.0;

/// How far the envelope moves the cutoff at full ENV amount, in octaves — (Cutoff ← Envelope)'s
/// scale, and the Cutoff target's declared one. **Chosen**:
/// the summer weighs the envelope through 22 kΩ against the slider's 150 kΩ
/// (`sh-2.md` §7.3), so it dominates and full depth sends the sum past the top from
/// most slider positions — the sweep clips at the ceiling before the attack ends,
/// which is the wart. The envelope's peak voltage is unverified, so the span is a
/// number that produces that behaviour rather than a derivation.
pub const FILTER_ENV_OCTAVES: f32 = 10.0;

/// The inverted position's reach relative to the positive one. **Chosen** from a
/// reading of the schematic's inverting stage (plan §13: R76 33 kΩ feedback against
/// R77 120 kΩ input, about −0.28); the other orientation (−3.6) is the alternative
/// to record if the board-to-board pins prove otherwise.
pub const INVERTED_RATIO: f32 = 0.275;

/// The modulator's reach on the cutoff at full VCF MOD, in octaves. **Chosen**.
pub const FILTER_LFO_OCTAVES: f32 = 4.0;

/// (Cutoff ← Bend)'s reach: at full amount, the lever at full moves the cutoff two octaves. It reads
/// the lever's position, not the bend in semitones, so it no longer depends on the bend range — the
/// machine's two sensitivity sliders are independent (`sh-2.md` §4.9), and the coupling was this
/// implementation's (plan N1). **Two, not one, because the retired control reached two**: a full
/// `bendfilter` at the 24-semitone range moved the cutoff two octaves, and a smaller scale would have
/// left that state unreachable (`tests/legacy_reachability.rs`).
pub const BEND_FILTER_OCTAVES: f32 = 2.0;

/// The modulator's reach on pitch at full VCO MOD, in semitones. **Chosen**.
pub const VCO_LFO_SEMITONES: f32 = 7.0;

/// How far the pulse-width section narrows from square at full depth: from 0.5 to the narrowest
/// 0.05. The Pulse width target's scale, negative, because both positions narrow.
pub const PWM_SWING: f32 = 0.5 - 0.05;

/// Where keyboard tracking pivots: middle C, mono-01's.
pub const KEY_TRACK_CENTRE: f32 = 60.0;

/// Tracking at the top of the KYBD slider: the manual says the top *over*-tracks —
/// "tone actually brightens as higher pitches are played" (§3.6). **Chosen** amount.
pub const KEY_TRACK_MAX: f32 = 1.2;

/// How far below the key the auto bend starts at full depth, in semitones.
/// **Chosen**; the slider takes a proportion of a decaying voltage (§4.8).
pub const AUTO_BEND_SEMITONES: f32 = 12.0;

/// The auto bend's time constant. **Chosen** from the plan's reading of the schematic
/// (§13): C76 is the timing capacitor, most likely the parts list's 0.33 µF, decaying
/// through R251's 220 kΩ — about 73 ms. The shape, one exponential, is firm; the
/// duration is not.
pub const AUTO_BEND_TAU_S: f32 = 0.073;

/// The GATE VCA mode's rise and fall — "silence the instant all are released", with
/// the short slope a real gate through 12 kΩ has. **Chosen**, mono-01's.
pub const GATE_TIME_S: f32 = 0.002;

/// How long the output must be silent before the voice reports idle. **Chosen**,
/// `mxm-poly-06-dsp`'s shape: a settle, not an exact-zero detector along the chain.
pub const POST_TAIL_S: f32 = 0.5;

/// Below this the output counts as silent for the settle: about −140 dBFS, far under
/// anything audible and far over the flush threshold. Without it the coupling stage's
/// tail — a 6 Hz one-pole, audible only under HOLD's open amplifier — would hold the
/// voice active for the best part of a second after the sound was gone; the output
/// itself still reaches exact zero, and idle makes it exact at once.
pub const SILENCE_FLOOR: f32 = 1e-7;

/// The VCA's input coupling — C14 (0.1 µF) into R69 (270 kΩ), `sh-2.md` §7.4 — the one
/// AC-coupling stage between the filter and the jack whose corner is anywhere near the
/// audio band: 1 / (2π · 0.1 µF · 270 kΩ) = 5.9 Hz. **Derived** from the schematic, not
/// measured. The output network's 10 µF (§7.6) sits a decade lower, about 1 Hz into its
/// 14.7 kΩ and load-dependent, and is not modelled. The stage sits **before** the
/// amplifier, as on the machine: what the amplifier gates has no offset left to thump
/// with, and a closed amplifier hides the stage's tail. At the lowest F of the 32' range
/// this costs about 1.1 dB, the machine's own thinning; mono-01's 15 Hz, which this crate
/// carried until round 3 of the code review found it, cost 4.6 dB there
/// (`the_coupling_is_the_vcas_and_costs_the_bottom_of_32_feet_a_decibel`).
pub const VCA_COUPLING_HZ: f32 = 5.9;

/// Headroom in the mixer: three sources at full must not slam the filter's input
/// stage. mono-01's.
const MIX_HEADROOM: f32 = 0.5;

/// What a HOLD drone sounds before the first press since `reset`. **Chosen**: middle C,
/// on the first channel.
pub const DEFAULT_KEY: u8 = 60;
pub const DEFAULT_CHANNEL: u8 = 0;

/// The output bound: the filter's bound through an amplifier routes can double, at unity volume —
/// `routing::BOUND` holds the Amplitude sum to one, so its factor is at most two.
pub const OUTPUT_BOUND: f32 = filter::OUTPUT_BOUND * 2.0;

#[inline]
fn key_to_hz(key: f32) -> f32 {
    440.0 * ((key - 69.0) / 12.0).exp2()
}

/// A one-pole lowpass on a control signal, coefficient cached against its time.
///
/// The state is `f64`, and that is load-bearing: in `f32` a lag towards a key number
/// near 72 stalls a few hundredths of a semitone short, because the per-sample step
/// falls under half an ulp of the value it is added to — the same stall
/// `mxm-poly-06-dsp` found in the envelope's decay. Measured here: a 0.2 s glide
/// sat 0.037 semitones flat for ever. Control-rate arithmetic, so `f64` costs nothing.
#[derive(Debug, Clone, Copy)]
struct Lag {
    value: f64,
    tau_s: f32,
    coef: f64,
}

impl Lag {
    const fn new(value: f32) -> Self {
        Self {
            value: value as f64,
            tau_s: -1.0,
            coef: 0.0,
        }
    }

    #[inline]
    fn set(&mut self, value: f32) {
        self.value = value as f64;
    }

    #[inline]
    fn get(&self) -> f32 {
        self.value as f32
    }

    #[inline]
    fn step(&mut self, target: f32, tau_s: f32, sample_rate: f32) -> f32 {
        if tau_s != self.tau_s {
            self.tau_s = tau_s;
            self.coef = if tau_s <= 0.0 {
                0.0
            } else {
                (-1.0 / (tau_s as f64 * sample_rate as f64)).exp()
            };
        }
        let target = target as f64;
        self.value = target + (self.value - target) * self.coef;
        self.value as f32
    }
}

#[derive(Debug, Clone)]
pub struct Voice {
    sample_rate: f32,
    keyboard: Keyboard,
    vco1: Vco,
    vco2: Vco,
    sub: Sub,
    noise: Noise,
    modulator: Modulator,
    envelope: Adsr,
    filter: Ladder,
    dc: DcBlocker,
    /// The portamento lag on the key CV, in MIDI key units.
    glide: Lag,
    /// The auto bend's dip, as a shape: one at the trigger, decaying to nothing.
    auto_bend: Lag,
    /// The GATE VCA mode's smoothed gate.
    gate: Lag,
    /// All Sound Off's latch on HOLD.
    hold_latched: bool,
    /// The velocity of **the press that last triggered the envelope** — the Velocity source, by the
    /// modulation standard. A legato press in GATE mode sounds its key without retriggering, so it
    /// keeps the phrase's. Full before any press, so the source rests at zero under a HOLD drone.
    velocity: f32,
    last_vca_mode: VcaMode,
    /// Samples of exact silence at the output so far.
    silent_samples: u32,
    idle: bool,
    /// The routing's frame and compacted lists.
    graph: Graph,
    // Read-only telemetry of the last sample.
    last_pitch_hz: f32,
    last_cutoff_hz: f32,
    /// The mixer's output on the last sample, before the filter: what a scope of the mix shows.
    last_mixed: f32,
    last_width: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

impl Voice {
    pub fn new() -> Self {
        let mut v = Self {
            sample_rate: 48_000.0,
            keyboard: Keyboard::new(DEFAULT_KEY, DEFAULT_CHANNEL),
            vco1: Vco::new(),
            vco2: Vco::new(),
            sub: Sub::new(),
            noise: Noise::new(),
            modulator: Modulator::new(),
            envelope: Adsr::new(),
            filter: Ladder::new(),
            dc: DcBlocker::new(),
            glide: Lag::new(DEFAULT_KEY as f32),
            auto_bend: Lag::new(0.0),
            gate: Lag::new(0.0),
            hold_latched: false,
            velocity: 1.0,
            last_vca_mode: VcaMode::Env,
            silent_samples: u32::MAX,
            idle: true,
            graph: Graph::new(),
            last_pitch_hz: 0.0,
            last_cutoff_hz: 0.0,
            last_mixed: 0.0,
            last_width: 0.5,
        };
        v.set_sample_rate(48_000.0);
        v
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.envelope.set_sample_rate(sample_rate);
        self.dc.set_corner(VCA_COUPLING_HZ, sample_rate);
        // Force the lags to recompute against the new rate.
        self.glide.tau_s = -1.0;
        self.auto_bend.tau_s = -1.0;
        self.gate.tau_s = -1.0;
    }

    /// A fresh start: the machine powered on. Every press forgotten, every tail gone,
    /// the key CV at the default, the latch cleared — a HOLD patch drones from here.
    pub fn reset(&mut self) {
        self.keyboard.reset();
        self.velocity = 1.0;
        self.vco1.reset();
        self.vco2.reset();
        self.sub.reset();
        self.noise.reset();
        self.modulator.reset();
        self.envelope.reset();
        self.filter.reset();
        self.dc.reset();
        self.glide = Lag::new(DEFAULT_KEY as f32);
        self.auto_bend = Lag::new(0.0);
        self.gate = Lag::new(0.0);
        self.hold_latched = false;
        self.silent_samples = u32::MAX;
        self.idle = true;
        self.graph.reset();
        self.set_sample_rate(self.sample_rate);
    }

    // ---- Events ----

    /// The keyboard trigger's two consumers besides the envelope: the delay's dump and the auto
    /// bend's charge. **The dip charges to its full shape whatever its route's depth** — the depth is
    /// the route's, applied every sample — so the source runs whether or not anything reads it.
    fn on_trigger(&mut self) {
        self.modulator.trigger();
        self.auto_bend.set(1.0);
    }

    /// A key went down, at a velocity the Velocity source carries and nothing else reads.
    pub fn note_on(&mut self, id: NoteId, trigger_mode: TriggerMode, velocity: f32) -> Outcome {
        let o = self.keyboard.note_on(id, velocity);
        // A note-on is the deliberate act that clears the panic latch.
        self.hold_latched = false;
        self.wake();
        if o.trigger {
            self.on_trigger();
        }
        // Which presses trigger the envelope, and so carry the Velocity source. In LFO mode the
        // modulator triggers the envelope and no key does, so a press that triggers the keyboard
        // carries it.
        let (envelope, carries_velocity) = match trigger_mode {
            TriggerMode::GateTrig => (o.trigger, o.trigger),
            TriggerMode::Gate => (o.gate_opened, o.gate_opened),
            TriggerMode::Lfo => (false, o.trigger),
        };
        if envelope {
            self.envelope.trigger();
        }
        if carries_velocity {
            self.velocity = velocity;
        }
        o
    }

    pub fn note_off(&mut self, voice_id: Option<i32>, channel: u8, key: u8) -> Outcome {
        let o = self.keyboard.note_off(voice_id, channel, key);
        if o.gate_closed {
            self.envelope.release();
        }
        o
    }

    /// A host's targeted choke: with nothing left the voice stops without its tail.
    ///
    /// The cut closes the amplifier, and the coupling stage sits before it, so it is left
    /// alone: the output is exact zero without it, and resetting it under HOLD's open
    /// amplifier would put a step into the drone.
    pub fn choke(&mut self, voice_id: Option<i32>, channel: u8, key: u8) -> Outcome {
        let o = self.keyboard.choke(voice_id, channel, key);
        if o.cut {
            self.envelope.silence();
            self.gate.set(0.0);
        } else if o.gate_closed {
            self.envelope.release();
        }
        o
    }

    /// All Notes Off: every key rising at once. Never a panic; a HOLD drone plays on.
    pub fn all_notes_off(&mut self) -> Outcome {
        let o = self.keyboard.all_notes_off();
        if o.gate_closed {
            self.envelope.release();
        }
        o
    }

    /// All Sound Off: immediate silence in every mode, and in HOLD a latch that only
    /// a note-on, `reset` or HOLD re-entered will clear.
    ///
    /// The recursive state that carries what was heard — the ladder and the coupling stage — is
    /// cleared, because the voice runs on through the settle and a note inside it would otherwise
    /// read it. The owner and the free-running state are kept.
    pub fn all_sound_off(&mut self) {
        self.keyboard.all_notes_off();
        self.envelope.silence();
        self.filter.reset();
        self.dc.reset();
        self.gate.set(0.0);
        self.auto_bend.set(0.0);
        self.hold_latched = true;
    }

    pub fn set_expression(
        &mut self,
        voice_id: Option<i32>,
        channel: u8,
        key: u8,
        semitones: f32,
    ) -> bool {
        self.keyboard
            .set_expression(voice_id, channel, key, semitones)
    }

    pub fn sounding(&self) -> Option<Press> {
        self.keyboard.sounding()
    }

    /// Who the voice follows — the plugin reads bend and wheel from this channel.
    pub fn owner(&self) -> Owner {
        self.keyboard.owner()
    }

    pub fn is_held(&self) -> bool {
        self.keyboard.is_held()
    }

    pub fn hold_latched(&self) -> bool {
        self.hold_latched
    }

    // ---- Telemetry, read-only ----

    pub fn pitch_hz(&self) -> f32 {
        self.last_pitch_hz
    }

    pub fn cutoff_hz(&self) -> f32 {
        self.last_cutoff_hz
    }

    /// The mixer's output on the last sample, before the filter.
    pub fn mixer_output(&self) -> f32 {
        self.last_mixed
    }

    /// The width both pulses share this sample.
    pub fn pulse_width(&self) -> f32 {
        self.last_width
    }

    pub fn glided_key(&self) -> f32 {
        self.glide.get()
    }

    /// What this voice's frame holds for `source`, for tests.
    #[cfg(test)]
    pub(crate) fn published_for_test(&self, source: usize) -> f32 {
        self.graph.read(source)
    }

    /// The auto bend's dip shape, `0..=1` — the Auto bend source before any route scales it.
    pub fn auto_bend_shape(&self) -> f32 {
        self.auto_bend.get()
    }

    pub fn lfo_fade(&self) -> f32 {
        self.modulator.fade()
    }

    pub fn envelope_level(&self) -> f32 {
        self.envelope.level()
    }

    // ---- Activity ----

    /// Whether the voice is live: the output was above the floor within the settle, the
    /// envelope is the gain and still running, or **a key is down** — a held key never
    /// idles, whatever the output, because idling would silence the envelope it still
    /// needs (`a_held_gate_note_with_nothing_fed_keeps_its_envelope`). Never derived from
    /// the mode switch: a silent HOLD is idle.
    pub fn is_active(&self) -> bool {
        !self.idle
    }

    /// How long the tail can last, in samples — `None` when it cannot end on its own: a key
    /// down in any mode, or a HOLD drone. The plugin turns this into its process status.
    ///
    /// **Audible, not hidden.** The envelope runs on behind a closed amplifier, so its release
    /// is the tail only in ENV mode; a released GATE note closes in the gate's slope, and a
    /// latched HOLD is cut already. Each plus the settle the activity verdict waits for.
    pub fn tail_samples(&self, release_s: f32, vca_mode: VcaMode) -> Option<u32> {
        if self.idle {
            return Some(0);
        }
        if self.keyboard.is_held() || (vca_mode == VcaMode::Hold && !self.hold_latched) {
            return None;
        }
        let settle = (POST_TAIL_S * self.sample_rate) as u32;
        let audible = match vca_mode {
            VcaMode::Env => self.envelope.tail_samples(release_s),
            VcaMode::Gate => (GATE_TIME_S * self.sample_rate) as u32,
            VcaMode::Hold => 0,
        };
        Some(audible + settle)
    }

    fn wake(&mut self) {
        self.idle = false;
        self.silent_samples = 0;
    }

    /// The transition into idle: hidden state with a target settles to it.
    fn go_idle(&mut self) {
        self.idle = true;
        self.envelope.silence();
        self.dc.reset();
        self.filter.reset();
        self.modulator.settle();
        self.glide.set(self.keyboard.owner().key as f32);
        self.auto_bend.set(0.0);
        self.gate
            .set(if self.keyboard.is_held() { 1.0 } else { 0.0 });
    }

    // ---- Audio ----

    /// Rebuilds which routes are live. **Once per processing interval, never per sample**: topology
    /// is discrete and changes only on a parameter event.
    pub fn set_topology(&mut self, routing: &Routing) {
        self.graph.set_topology(routing);
    }

    /// Whether the mixer has anything for an open amplifier to pass: a level up, or the filter
    /// singing on its own past the excitation threshold, which needs no source at all.
    #[inline]
    fn fed(p: &Params) -> bool {
        p.sub_level > 0.0
            || p.vco1_level > 0.0
            || p.vco2_level > 0.0
            || p.resonance > crate::filter::EXCITATION_THRESHOLD
    }

    /// Where the cutoff rests while the voice is idle, for the display: the slider, and the routes
    /// whose sources hold still while nothing plays — the key, the velocity, the wheel, pressure and
    /// the bender's lever. A route at zero depth moves it by exactly nothing.
    fn resting_cutoff_octaves(&self, p: &Params, routing: &Routing) -> f32 {
        let mut octaves = p.cutoff.clamp(0.0, 1.0) * CUTOFF_RANGE_OCTAVES;
        for &(t, s) in routing.live() {
            let (t, s) = (t as usize, s as usize);
            if t != target::CUTOFF {
                continue;
            }
            let value = match s {
                source::KEY => crate::routing::key_source(self.glided_key()),
                source::VELOCITY => standard::velocity(self.velocity),
                source::WHEEL => standard::wheel(p.mod_wheel),
                source::PRESSURE => standard::pressure(p.pressure),
                source::BEND => standard::bend(p.bend_position),
                _ => continue,
            };
            let amount = routing.amounts[t][s];
            octaves += (amount * value) * crate::routing::scale(t, s, amount);
        }
        octaves
    }

    /// One sample. `routing` is the patch's routes, whose topology [`Voice::set_topology`] has
    /// built.
    #[inline]
    pub fn process(&mut self, p: &Params, routing: &Routing) -> f32 {
        let fs = self.sample_rate;

        // The VCA switch: entering HOLD is a deliberate act that clears the latch.
        if p.vca_mode != self.last_vca_mode {
            if p.vca_mode == VcaMode::Hold {
                self.hold_latched = false;
            }
            self.last_vca_mode = p.vca_mode;
        }

        // **Idle advances nothing.** The free-running state — both VCOs, the modulator, the
        // noise — freezes here rather than by the host's grace, so the next phrase starts from
        // the same phases whether the host kept calling through the silence or not. What a call
        // can carry that wakes the voice: HOLD open onto a fed mixer. A key is `note_on`'s business.
        if self.idle {
            // A key down never idles (below), so an open amplifier here is HOLD unlatched.
            let open = p.vca_mode == VcaMode::Hold && !self.hold_latched;
            if open && Self::fed(p) {
                self.wake();
            } else {
                let octaves = self.resting_cutoff_octaves(p, routing);
                self.last_cutoff_hz =
                    CUTOFF_FLOOR_HZ * octaves.clamp(0.0, CUTOFF_RANGE_OCTAVES).exp2();
                self.last_mixed = 0.0;
                return 0.0;
            }
        }

        // The modulator, with the instrument's one noise sample shared by its S&H and
        // VCO-2's noise position.
        let noise = self.noise.sample();
        let m: Outputs = self
            .modulator
            .process(p.lfo_rate_hz, p.lfo_delay_s, noise, fs);
        // The square retriggers the **envelope only** (`sh-2.md` §2): the delay's restart and
        // the auto bend are the keyboard trigger's consumers, and an LFO edge is not a key.
        if p.trigger_mode == TriggerMode::Lfo && m.square_rose && self.keyboard.is_held() {
            self.envelope.trigger();
        }
        let mod_selected = m.selected(p.lfo_mode);

        // The envelope, one object for three consumers.
        let env = self
            .envelope
            .process(p.attack_s, p.decay_s, p.sustain, p.release_s);

        // The key CV through the portamento — always in circuit, zero the only off — and the auto
        // bend's dip, which is a source now: its depth is the route's.
        let owner = self.keyboard.owner();
        let glided = self
            .glide
            .step(owner.key as f32, p.portamento_s.max(0.0), fs);
        let bend_shape = self.auto_bend.step(0.0, AUTO_BEND_TAU_S, fs);

        // **The forward sources, published in declared order** (`crate::routing`), and only the
        // ones a live route reads, the performance sources through the collection's standard so
        // each is zero at its rest. With nothing routed the voice opens no frame at all.
        let routed = self.graph.any_live();
        if routed {
            let graph = &mut self.graph;
            graph.begin_sample();
            graph.write(source::LFO, mod_selected);
            graph.write(source::LFO_WIDTH, 0.5 * (1.0 + m.pwm_sine));
            graph.write(source::ENVELOPE, env);
            graph.write(source::AUTO_BEND, bend_shape);
            graph.write(source::KEY, crate::routing::key_source(glided));
            graph.write(source::VELOCITY, standard::velocity(self.velocity));
            graph.write(source::WHEEL, standard::wheel(p.mod_wheel));
            graph.write(source::PRESSURE, standard::pressure(p.pressure));
            graph.write(source::BEND, standard::bend(p.bend_position));
            graph.write(source::NOISE, noise);
        }

        // Pitch: the key, the tune, the host's per-note expression, and the routes.
        let pitch = if routed && !self.graph.is_empty(target::PITCH) {
            self.graph.sum(target::PITCH, routing)
        } else {
            0.0
        };
        let base = glided + p.total_tune_cents / 100.0 + owner.expression_semitones + pitch;
        let key1 = base
            + 12.0 * p.vco1_range.octaves()
            + if p.vco1_bender { p.bend_semitones } else { 0.0 };
        let key2 =
            base + 12.0 * p.vco2_range.octaves() + p.vco2_tune_cents / 100.0 + p.bend_semitones;
        let hz1 = key_to_hz(key1);
        self.vco1.set_freq(hz1, fs);
        self.vco2.set_freq(key_to_hz(key2), fs);

        // The shared pulse width: one width for both pulses, the slider plus its routes, and never
        // past square whatever they ask (wart 4).
        let width = if routed && !self.graph.is_empty(target::PULSE_WIDTH) {
            (p.pulse_width + self.graph.sum(target::PULSE_WIDTH, routing)).clamp(0.05, 0.5)
        } else {
            p.pulse_width.clamp(0.05, 0.5)
        };

        // The oscillators, the sub divided from VCO-1's reset, and the mixer.
        let sub = self.sub.process(&self.vco1);
        let osc1 = self.vco1.process_1(p.vco1_wave, width, fs);
        self.sub.clock(&self.vco1);
        let osc2 = self.vco2.process_2(p.vco2_wave, width, noise, fs);
        // **The voice's own audio is published here**, after the oscillators and before the cutoff:
        // a sample late into pitch and width, on time into the cutoff and the amplifier.
        if routed {
            self.graph.write(source::VCO1, osc1);
            self.graph.write(source::VCO2, osc2);
            self.graph.write(source::SUB, sub);
        }
        let mixed = (sub * p.sub_level + osc1 * p.vco1_level + osc2 * p.vco2_level) * MIX_HEADROOM;
        self.last_mixed = mixed;

        // The cutoff: the slider and the routes, a sum in octaves clamped at the machine's ceiling
        // before the exponential. The envelope dominates and the sweep clips (wart 12).
        let cutoff_mod = if routed && !self.graph.is_empty(target::CUTOFF) {
            self.graph.sum(target::CUTOFF, routing)
        } else {
            0.0
        };
        let octaves = p.cutoff.clamp(0.0, 1.0) * CUTOFF_RANGE_OCTAVES + cutoff_mod;
        let cutoff_hz = CUTOFF_FLOOR_HZ * octaves.clamp(0.0, CUTOFF_RANGE_OCTAVES).exp2();
        let filtered = self.filter.process(mixed, cutoff_hz, p.resonance, fs);

        // The VCA's input coupling, before the amplifier as on the machine
        // (`VCA_COUPLING_HZ`): what is gated carries no offset.
        let coupled = self.dc.process(filtered);

        // The VCA: HOLD open (unless latched), ENV the envelope, GATE the gate with its
        // short slope. The envelope keeps running behind a closed amplifier.
        let gate_target = if self.keyboard.is_held() { 1.0 } else { 0.0 };
        let gate = self.gate.step(gate_target, GATE_TIME_S, fs);
        let gain = match p.vca_mode {
            VcaMode::Hold => {
                if self.hold_latched {
                    0.0
                } else {
                    1.0
                }
            }
            VcaMode::Env => env,
            VcaMode::Gate => gate,
        };
        // Amplitude routes **scale** what the switch chose, never add to it, so no route can open a
        // closed amplifier or hold a latched HOLD open — the collection's one amplitude law.
        let gain = if routed && !self.graph.is_empty(target::AMPLITUDE) {
            gain * standard::amplitude_factor(self.graph.sum(target::AMPLITUDE, routing))
        } else {
            gain
        };
        let out = flush(coupled * gain * p.volume.clamp(0.0, 1.0));

        // Activity, from the output.
        if out.abs() > SILENCE_FLOOR {
            self.silent_samples = 0;
            self.idle = false;
        } else if !self.idle {
            self.silent_samples = self.silent_samples.saturating_add(1);
            // The envelope holds idle off only where it is the gain: in ENV mode a slow attack
            // can spend the settle below the floor and must not be cut. Behind a closed GATE or
            // a latched HOLD it runs on inaudibly, and waiting for it would keep a released
            // GATE note "active" for its whole hidden release.
            // And a key down never idles: idling silences the shared envelope, which a held
            // GATE note still needs on the filter and the pulse width the moment a level comes
            // up — and the process-status contract promises KeepAlive for a key down.
            let envelope_may_open = p.vca_mode == VcaMode::Env && self.envelope.is_active();
            // **A HOLD drone an amplitude route is holding silent** is not a drone that has ended:
            // idling would reset the filter and freeze the modulator that is about to open it again.
            // A route at zero depth moves nothing, so it holds nothing awake (`Routing::moves`).
            let tremolo_may_open = p.vca_mode == VcaMode::Hold
                && !self.hold_latched
                && routing.moves(target::AMPLITUDE)
                && Self::fed(p);
            let may_open = self.keyboard.is_held() || envelope_may_open || tremolo_may_open;
            if self.silent_samples >= (POST_TAIL_S * fs) as u32 && !may_open {
                self.go_idle();
            }
        }

        self.last_pitch_hz = hz1;
        self.last_cutoff_hz = cutoff_hz;
        self.last_width = width;
        out
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // `let mut p = Params::default(); p.x = …` reads as the patch it is
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn id(key: u8) -> NoteId {
        NoteId {
            voice_id: None,
            channel: 0,
            key,
        }
    }

    fn vid(v: i32, key: u8) -> NoteId {
        NoteId {
            voice_id: Some(v),
            channel: 0,
            key,
        }
    }

    /// Nothing routed: the voice with no modulation at all.
    const NONE: Routing = Routing::new();

    fn render(v: &mut Voice, p: &Params, secs: f32) -> Vec<f32> {
        (0..(FS * secs) as usize)
            .map(|_| v.process(p, &NONE))
            .collect()
    }

    /// A routing with exactly these pairs present, at these amounts.
    fn wired(pairs: &[(usize, usize, f32)]) -> Routing {
        let mut r = Routing::new();
        for &(t, s, a) in pairs {
            r.present[t][s] = true;
            r.amounts[t][s] = a;
        }
        r.compact();
        r
    }

    /// Renders under a routing, with the topology set first as the plugin sets it each block.
    fn render_routed(v: &mut Voice, p: &Params, r: &Routing, secs: f32) -> Vec<f32> {
        v.set_topology(r);
        (0..(FS * secs) as usize).map(|_| v.process(p, r)).collect()
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    /// Frequency of the voice's own reported pitch is exact; the *rendered* pitch is
    /// what a listener hears, estimated from upward zero crossings.
    fn rendered_hz(x: &[f32]) -> f32 {
        let (mut first, mut last, mut n, mut prev) = (None, 0usize, 0usize, f32::NAN);
        for (i, &y) in x.iter().enumerate() {
            if prev <= 0.0 && y > 0.0 {
                if first.is_none() {
                    first = Some(i);
                }
                last = i;
                n += 1;
            }
            prev = y;
        }
        match first {
            Some(f) if n > 1 => (n - 1) as f32 * FS / (last - f) as f32,
            _ => 0.0,
        }
    }

    #[test]
    fn a_note_sounds_at_its_pitch_and_release_reaches_exact_silence() {
        let mut v = Voice::new();
        let p = Params::default();
        v.note_on(id(69), p.trigger_mode, 1.0);
        let x = render(&mut v, &p, 1.0);
        let hz = rendered_hz(&x[FS as usize / 2..]);
        assert!((hz - 440.0).abs() < 1.0, "A4 rendered at {hz} Hz");
        v.note_off(None, 0, 69);
        let tail = render(&mut v, &p, 2.0);
        assert_eq!(
            *tail.last().unwrap(),
            0.0,
            "exact silence after the release"
        );
        assert!(!v.is_active(), "and the voice reports idle");
    }

    #[test]
    fn a_higher_key_over_a_held_lower_key_moves_none_of_the_triggers_consumers() {
        let mut p = Params::default();
        p.lfo_delay_s = 1.0;
        let mut v = Voice::new();
        v.note_on(id(48), p.trigger_mode, 1.0);
        render(&mut v, &p, 0.2);
        let (fade, bend, level, pitch) = (
            v.lfo_fade(),
            v.auto_bend_shape(),
            v.envelope_level(),
            v.pitch_hz(),
        );
        v.set_expression(None, 0, 48, 0.5);
        v.note_on(id(60), p.trigger_mode, 1.0);
        assert_eq!(v.lfo_fade(), fade, "the delay did not restart");
        assert_eq!(v.auto_bend_shape(), bend, "the auto bend did not restart");
        assert_eq!(v.envelope_level(), level, "the envelope did not re-attack");
        v.process(&p, &NONE);
        assert!(
            (v.pitch_hz() / pitch - 2.0f32.powf(0.5 / 12.0)).abs() < 1e-3,
            "pitch: the held key plus its standing expression, nothing else"
        );
        assert_eq!(
            v.owner().expression_semitones,
            0.5,
            "the expression survived the ignored press"
        );
    }

    #[test]
    fn ties_are_the_machines_legato_asymmetric_by_trigger_mode() {
        // The player emits the new note before the old one's release, same frame.
        let tie = |v: &mut Voice, p: &Params, from: u8, to: u8| {
            v.note_on(vid(2, to), p.trigger_mode, 1.0);
            v.note_off(Some(1), 0, from);
        };
        for (mode, downward_retriggers) in
            [(TriggerMode::GateTrig, true), (TriggerMode::Gate, false)]
        {
            let mut p = Params::default();
            p.trigger_mode = mode;
            p.attack_s = 0.5;
            // Upward: never re-attacks.
            let mut v = Voice::new();
            v.note_on(vid(1, 48), mode, 0.0);
            render(&mut v, &p, 0.1);
            let stage_before = v.envelope.stage();
            tie(&mut v, &p, 48, 60);
            assert_eq!(
                v.envelope.stage(),
                stage_before,
                "{mode:?}: an upward tie retriggered"
            );
            v.process(&p, &NONE);
            assert!(
                (v.pitch_hz() - key_to_hz(60.0)).abs() < 0.5,
                "and the pitch moved to the new key"
            );
            // Downward: re-attacks in GATE+TRIG only.
            let mut v = Voice::new();
            v.note_on(vid(1, 60), mode, 0.0);
            render(&mut v, &p, 1.0); // well past the attack
            let level_before = v.envelope_level();
            tie(&mut v, &p, 60, 48);
            let retriggered = v.envelope.stage() == crate::envelope::Stage::Attack;
            assert_eq!(
                retriggered, downward_retriggers,
                "{mode:?}: downward tie, level {level_before}"
            );
            // Same pitch: never.
            let mut v = Voice::new();
            v.note_on(vid(1, 55), mode, 0.0);
            render(&mut v, &p, 1.0);
            tie(&mut v, &p, 55, 55);
            assert_ne!(
                v.envelope.stage(),
                crate::envelope::Stage::Attack,
                "{mode:?}: a same-pitch tie retriggered"
            );
            assert!(v.is_held(), "and it still sounds");
        }
    }

    #[test]
    fn lfo_trigger_mode_re_attacks_once_per_cycle_while_held_and_stops_after_release() {
        let mut p = Params::default();
        p.trigger_mode = TriggerMode::Lfo;
        p.lfo_rate_hz = 4.0;
        p.attack_s = 0.01;
        p.decay_s = 0.05;
        p.sustain = 0.0;
        let mut v = Voice::new();
        v.note_on(id(60), p.trigger_mode, 0.0);
        let mut attacks = 0;
        let mut was_attack = false;
        for _ in 0..(FS * 2.0) as usize {
            v.process(&p, &NONE);
            let now = v.envelope.stage() == crate::envelope::Stage::Attack;
            if now && !was_attack {
                attacks += 1;
            }
            was_attack = now;
        }
        assert!(
            (7..=8).contains(&attacks),
            "4 Hz over 2 s: {attacks} attacks"
        );
        v.note_off(None, 0, 60);
        let mut more = 0;
        for _ in 0..(FS * 2.0) as usize {
            v.process(&p, &NONE);
            let now = v.envelope.stage() == crate::envelope::Stage::Attack;
            if now && !was_attack {
                more += 1;
            }
            was_attack = now;
        }
        assert_eq!(more, 0, "no attacks after release");
    }

    #[test]
    fn both_pulses_share_one_width_that_narrows_from_square_and_never_widens() {
        for source in [source::LFO_WIDTH, source::ENVELOPE] {
            let r = wired(&[(target::PULSE_WIDTH, source, 1.0)]);
            let mut p = Params::default();
            p.lfo_rate_hz = 3.0;
            p.attack_s = 0.3;
            p.decay_s = 0.3;
            p.sustain = 0.5;
            let mut v = Voice::new();
            v.set_topology(&r);
            v.note_on(id(60), p.trigger_mode, 0.0);
            let (mut lo, mut hi) = (1.0f32, 0.0f32);
            for _ in 0..(FS * 2.0) as usize {
                v.process(&p, &r);
                lo = lo.min(v.pulse_width());
                hi = hi.max(v.pulse_width());
            }
            assert!(
                hi <= 0.5 + 1e-6,
                "{source:?}: the width passed square: {hi}"
            );
            assert!(
                lo < 0.2,
                "{source:?}: the modulation must actually narrow: {lo}"
            );
        }
    }

    #[test]
    fn the_delay_restarts_on_a_lower_key_and_not_on_a_higher_one() {
        let mut p = Params::default();
        p.lfo_delay_s = 1.0;
        let mut v = Voice::new();
        v.note_on(id(60), p.trigger_mode, 0.0);
        render(&mut v, &p, 0.5);
        let fade = v.lfo_fade();
        assert!(fade > 0.0);
        v.note_on(id(72), p.trigger_mode, 0.0);
        assert_eq!(v.lfo_fade(), fade, "a higher key over a held lower one");
        v.note_on(id(48), p.trigger_mode, 0.0);
        assert_eq!(v.lfo_fade(), 0.0, "a lower key restarts the fade");
    }

    #[test]
    fn auto_bend_dips_from_below_on_a_trigger_and_not_on_an_upward_tie_and_nothing_at_zero() {
        let p = Params::default();
        let r = wired(&[(target::PITCH, source::AUTO_BEND, 1.0)]);
        let mut v = Voice::new();
        v.set_topology(&r);
        v.note_on(id(60), p.trigger_mode, 1.0);
        v.process(&p, &r);
        assert!(
            v.pitch_hz() < key_to_hz(60.0) * 0.6,
            "the onset is well below the key"
        );
        render_routed(&mut v, &p, &r, 1.0);
        assert!(
            (v.pitch_hz() - key_to_hz(60.0)).abs() < 0.5,
            "and it arrives"
        );
        // An upward tie: no trigger, so no dip.
        v.note_on(vid(9, 72), p.trigger_mode, 1.0);
        v.note_off(None, 0, 60);
        v.process(&p, &r);
        assert!(
            (v.pitch_hz() - key_to_hz(72.0)).abs() < 0.5,
            "an upward joint is not dipped"
        );
        // Zero depth: a present route at zero moves nothing.
        let zero = wired(&[(target::PITCH, source::AUTO_BEND, 0.0)]);
        let mut v = Voice::new();
        v.set_topology(&zero);
        v.note_on(id(60), p.trigger_mode, 1.0);
        v.process(&p, &zero);
        assert!((v.pitch_hz() - key_to_hz(60.0)).abs() < 0.5);
    }

    #[test]
    fn portamento_is_fixed_time_never_snaps_and_reaches_the_filter() {
        let mut p = Params::default();
        p.portamento_s = 0.2;
        let r = wired(&[(target::CUTOFF, source::KEY, 1.0)]);
        let render = |v: &mut Voice, p: &Params, secs: f32| render_routed(v, p, &r, secs);
        let mut v = Voice::new();
        v.set_topology(&r);
        // The first note after reset glides from the pre-note key.
        v.note_on(id(72), p.trigger_mode, 0.0);
        v.process(&p, &r);
        assert!(
            v.glided_key() < 61.0,
            "the first glide starts from the default key"
        );
        render(&mut v, &p, 2.0);
        assert!(
            (v.glided_key() - 72.0).abs() < 0.01,
            "after 2 s at tau 0.2: {}",
            v.glided_key()
        );
        let cutoff_high = v.cutoff_hz();
        // An octave and a semitone cover the same fraction in the same time.
        let fraction = |v: &mut Voice, to: u8| {
            let from = v.glided_key();
            v.note_on(id(to), p.trigger_mode, 0.0);
            render(v, &p, 0.2);
            let f = (v.glided_key() - from) / (to as f32 - from);
            v.note_off(None, 0, to);
            f
        };
        let a = fraction(&mut v, 60);
        render(&mut v, &p, 2.0);
        v.all_notes_off();
        render(&mut v, &p, 2.0);
        let b = fraction(&mut v, 59);
        assert!((a - b).abs() < 0.02, "octave {a:.3} vs semitone {b:.3}");
        // The filter's tracking glided with the pitch: at the lower key the cutoff sits lower.
        render(&mut v, &p, 2.0);
        assert!(
            v.cutoff_hz() < cutoff_high,
            "tracking follows the glided key"
        );
        // A phrase's first note glides from the previous note, not from its own pitch.
        v.all_notes_off();
        render(&mut v, &p, 1.5);
        assert!(!v.is_active(), "silent between phrases");
        v.note_on(id(84), p.trigger_mode, 0.0);
        v.process(&p, &r);
        assert!(
            v.glided_key() < 60.5,
            "the next phrase starts from the last note: {}",
            v.glided_key()
        );
    }

    #[test]
    fn the_envelope_dominates_the_cutoff_and_the_sweep_clips_at_the_ceiling() {
        let mut p = Params::default();
        p.cutoff = 0.5;
        p.attack_s = 0.1;
        let full = wired(&[(target::CUTOFF, source::ENVELOPE, 1.0)]);
        let mut v = Voice::new();
        v.set_topology(&full);
        v.note_on(id(60), p.trigger_mode, 0.0);
        let mut hit_ceiling_at = None;
        for i in 0..(FS * 0.1) as usize {
            v.process(&p, &full);
            if hit_ceiling_at.is_none()
                && v.cutoff_hz() >= CUTOFF_FLOOR_HZ * CUTOFF_RANGE_OCTAVES.exp2() * 0.999
            {
                hit_ceiling_at = Some(i as f32 / FS);
            }
        }
        let t = hit_ceiling_at.expect("full envelope depth must reach the ceiling");
        assert!(t < 0.1, "and it does so before the attack ends: {t} s");
        // Inverted is not mirrored: its reach is the chosen ratio of the positive one.
        let excursion = |amount: f32| {
            let mut p = p;
            p.cutoff = 0.5;
            p.attack_s = 0.5;
            let r = wired(&[(target::CUTOFF, source::ENVELOPE, amount)]);
            let mut v = Voice::new();
            v.note_on(id(60), p.trigger_mode, 0.0);
            let base = CUTOFF_FLOOR_HZ * (0.5 * CUTOFF_RANGE_OCTAVES).exp2();
            render_routed(&mut v, &p, &r, 0.05);
            (v.cutoff_hz() / base).log2()
        };
        let up = excursion(1.0);
        let down = excursion(-1.0);
        assert!(up > 0.0 && down < 0.0);
        assert!(
            ((-down / up) - INVERTED_RATIO).abs() < 0.02,
            "ratio {}",
            -down / up
        );
    }

    #[test]
    fn hold_sounds_with_no_gate_and_gate_steps_while_the_envelope_sweeps_the_filter() {
        let mut p = Params::default();
        p.vca_mode = VcaMode::Hold;
        let mut v = Voice::new();
        let x = render(&mut v, &p, 0.5);
        assert!(peak(&x) > 0.1, "HOLD sounds before any note");
        assert!(v.is_active());
        assert_eq!(
            v.tail_samples(p.release_s, p.vca_mode),
            None,
            "a drone cannot end on its own"
        );
        // GATE: a step, and the envelope still sweeps the filter behind it.
        p.vca_mode = VcaMode::Gate;
        p.cutoff = 0.6;
        p.attack_s = 0.5;
        let r = wired(&[(target::CUTOFF, source::ENVELOPE, 1.0)]);
        let render = |v: &mut Voice, p: &Params, secs: f32| render_routed(v, p, &r, secs);
        let mut v = Voice::new();
        v.note_on(id(60), p.trigger_mode, 0.0);
        let x = render(&mut v, &p, 0.01);
        assert!(
            peak(&x[(FS * 0.005) as usize..]) > 0.05,
            "the gate is up within its slope"
        );
        let c1 = v.cutoff_hz();
        render(&mut v, &p, 0.2);
        assert!(
            v.cutoff_hz() > c1 * 1.5,
            "the envelope sweeps the cutoff under GATE"
        );
        let sounding = peak(&render(&mut v, &p, 0.05));
        v.note_off(None, 0, 60);
        // The gate falls within its slope, and with the coupling stage before the
        // amplifier nothing rings on after it: the output reaches exact zero.
        let x = render(&mut v, &p, 0.02);
        let after = peak(&x[(FS * 0.01) as usize..]);
        assert!(
            after < sounding * 0.1,
            "the gate falls at once: {after} against {sounding}"
        );
        let tail = render(&mut v, &p, 1.0);
        assert_eq!(*tail.last().unwrap(), 0.0);
    }

    #[test]
    fn all_sound_off_silences_hold_until_a_deliberate_act() {
        let mut p = Params::default();
        p.vca_mode = VcaMode::Hold;
        let mut v = Voice::new();
        render(&mut v, &p, 0.2);
        v.all_sound_off();
        let x = render(&mut v, &p, 0.3);
        assert_eq!(
            peak(&x[(FS * 0.05) as usize..]),
            0.0,
            "exact zero within the block"
        );
        // A mixer move does not resume it.
        p.vco1_level = 0.7;
        let x = render(&mut v, &p, 0.1);
        assert_eq!(peak(&x), 0.0);
        // Leaving HOLD and re-entering does.
        p.vca_mode = VcaMode::Env;
        render(&mut v, &p, 0.05);
        p.vca_mode = VcaMode::Hold;
        let x = render(&mut v, &p, 0.1);
        assert!(peak(&x) > 0.05, "re-entering HOLD resumes the drone");
        // And so does a note-on.
        v.all_sound_off();
        assert_eq!(peak(&render(&mut v, &p, 0.1)), 0.0);
        v.note_on(id(50), p.trigger_mode, 0.0);
        assert!(peak(&render(&mut v, &p, 0.1)) > 0.05);
        // Sabotage the plan asks for: without the latch the drone would be back at once;
        // the latch is what the first assertion measured.
    }

    /// **A panic clears the recursive state it owns.** Two voices play the same key for the same
    /// number of samples, so every piece of free-running state the contract keeps — both VCOs, the
    /// sub, the modulator, the noise, the glide — is identical between them, while their filters are
    /// set far apart: one singing at full resonance near the top, one closed and flat. So the ladder
    /// and the coupling stage are the only state that differs when both are panicked and play the
    /// same note on the same settings. The voice keeps processing through the settle after a panic, so
    /// a ladder or a coupling stage still holding what it rang with would carry it into that note.
    /// The owner outlives the panic, as it outlives any release.
    #[test]
    fn panic_clears_the_filter_and_the_coupling_but_retains_the_owner() {
        let mut singing = Params::default();
        singing.cutoff = 0.8;
        singing.resonance = 1.0;
        let mut closed = Params::default();
        closed.cutoff = 0.1;
        closed.resonance = 0.0;
        let mut long_history = Voice::new();
        let mut short_history = Voice::new();
        long_history.note_on(vid(1, 48), singing.trigger_mode, 0.9);
        short_history.note_on(vid(1, 48), closed.trigger_mode, 0.2);
        for _ in 0..5_000 {
            long_history.process(&singing, &NONE);
            short_history.process(&closed, &NONE);
        }

        long_history.all_sound_off();
        short_history.all_sound_off();
        assert!(!long_history.is_held() && long_history.sounding().is_none());
        assert_eq!(long_history.owner().key, 48, "the owner outlives the panic");
        assert_eq!(long_history.owner().velocity, 0.9);
        let mut p = Params::default();
        p.cutoff = 0.3;
        p.resonance = 0.6;
        assert!(
            long_history
                .tail_samples(p.release_s, p.vca_mode)
                .is_some_and(|n| n > 0),
            "the voice keeps processing through the settle after a panic"
        );

        long_history.note_on(vid(2, 60), p.trigger_mode, 0.5);
        short_history.note_on(vid(2, 60), p.trigger_mode, 0.5);
        let long_wake: Vec<f32> = (0..2_048)
            .map(|_| long_history.process(&p, &NONE))
            .collect();
        let short_wake: Vec<f32> = (0..2_048)
            .map(|_| short_history.process(&p, &NONE))
            .collect();
        assert!(
            peak(&long_wake) > 0.01,
            "the note is heard after the panic: {}",
            peak(&long_wake)
        );
        let first_difference = long_wake
            .iter()
            .zip(&short_wake)
            .position(|(a, b)| a.to_bits() != b.to_bits())
            .map(|i| (i, long_wake[i], short_wake[i]));
        assert_eq!(
            first_difference, None,
            "the ladder or the coupling stage leaked through All Sound Off"
        );
        assert_eq!(long_history.owner(), short_history.owner());
        assert_eq!(long_history.owner().key, 60);
    }

    #[test]
    fn holds_activity_follows_the_output() {
        let mut p = Params::default();
        p.vca_mode = VcaMode::Hold;
        p.vco1_level = 0.0; // every source silent, resonance below excitation
        let mut v = Voice::new();
        render(&mut v, &p, POST_TAIL_S + 0.1);
        assert!(!v.is_active(), "a silent HOLD is idle");
        p.vco1_level = 1.0;
        render(&mut v, &p, 0.1);
        assert!(v.is_active(), "and active again once something sounds");
        // The panic-latched state goes idle too.
        v.all_sound_off();
        render(&mut v, &p, POST_TAIL_S + 0.1);
        assert!(!v.is_active());
    }

    #[test]
    fn choke_all_notes_off_and_all_sound_off_are_three_outcomes() {
        let p = Params::default();
        let mut v = Voice::new();
        v.note_on(vid(1, 48), p.trigger_mode, 0.0);
        v.note_on(vid(2, 60), p.trigger_mode, 0.0);
        render(&mut v, &p, 0.2);
        // Choke the higher: nothing audible changes.
        v.choke(Some(2), 0, 60);
        v.process(&p, &NONE);
        assert!((v.pitch_hz() - key_to_hz(48.0)).abs() < 0.5);
        // Choke the last: no tail at all.
        let o = v.choke(Some(1), 0, 48);
        assert!(o.cut);
        let x = render(&mut v, &p, 0.05);
        assert!(
            peak(&x[64..]) < 1e-3,
            "a choke with nothing left cuts without a tail"
        );
        // All Notes Off: a release tail, and HOLD plays on.
        let mut p2 = p;
        p2.release_s = 0.5;
        let mut v = Voice::new();
        v.note_on(vid(1, 60), p2.trigger_mode, 0.0);
        render(&mut v, &p2, 0.2);
        v.all_notes_off();
        let x = render(&mut v, &p2, 0.1);
        assert!(peak(&x) > 0.05, "All Notes Off leaves the release tail");
        p2.vca_mode = VcaMode::Hold;
        let mut v = Voice::new();
        render(&mut v, &p2, 0.1);
        v.all_notes_off();
        assert!(
            peak(&render(&mut v, &p2, 0.1)) > 0.05,
            "a HOLD drone plays on through All Notes Off"
        );
    }

    /// **A non-finite expression is refused at the keyboard block**, and the press keeps the
    /// offset it had. A NaN in the pitch sum reaches both VCOs' phases and the sub, which never
    /// recover. The reference is the same voice never sent it.
    #[test]
    fn a_non_finite_expression_is_refused_and_the_pitch_stays_finite() {
        let p = Params::default();
        let bent = || {
            let mut v = Voice::new();
            v.note_on(id(48), p.trigger_mode, 1.0);
            assert!(v.set_expression(None, 0, 48, 3.0));
            v
        };
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let (mut actual, mut reference) = (bent(), bent());
            assert!(!actual.set_expression(None, 0, 48, bad), "{bad} is refused");
            assert_eq!(actual.owner().expression_semitones, 3.0, "{bad}");
            let heard = render(&mut actual, &p, 0.05);
            assert!(heard.iter().all(|y| y.is_finite()), "{bad}");
            assert_eq!(heard, render(&mut reference, &p, 0.05), "{bad}");
            assert!(peak(&heard) > 0.01, "the note sounds");
        }
    }

    #[test]
    fn idle_freezes_free_running_state_and_settles_targeted_state_host_independently() {
        let r = wired(&[(target::PITCH, source::LFO, 0.3)]);
        let render = |v: &mut Voice, p: &Params, secs: f32| render_routed(v, p, &r, secs);
        let phrase = |v: &mut Voice, p: &Params, key: u8| {
            v.note_on(id(key), p.trigger_mode, 1.0);
            let x = render(v, p, 0.3);
            v.note_off(None, 0, key);
            render(v, p, 0.2);
            x
        };
        let second_phrase_after_gap = |gap: f32| {
            let mut p = Params::default();
            p.portamento_s = 3.0; // far longer than the release
            p.vco2_level = 1.0; // two oscillators beating: the phase would show
            let mut v = Voice::new();
            phrase(&mut v, &p, 60);
            // Silence until idle, then a gap the host may or may not have called through.
            render(&mut v, &p, POST_TAIL_S + 0.2);
            assert!(!v.is_active());
            // A host that keeps calling through the gap must change nothing: exact zeros out,
            // and the same second phrase as from a host that stopped calling.
            let through = render(&mut v, &p, gap);
            assert!(
                through.iter().all(|s| *s == 0.0),
                "idle renders exact zeros"
            );
            phrase(&mut v, &p, 67)
        };
        let a = second_phrase_after_gap(0.0);
        let b = second_phrase_after_gap(3.0);
        assert_eq!(a, b, "the second phrase must not depend on the gap");
        // And the glide cut short by idle still starts the next phrase from the previous note.
        let mut p = Params::default();
        p.portamento_s = 3.0;
        let mut v = Voice::new();
        phrase(&mut v, &p, 60);
        render(&mut v, &p, POST_TAIL_S + 0.2);
        assert!(
            (v.glided_key() - 60.0).abs() < 1e-3,
            "settled to the last key at idle"
        );
        v.note_on(id(72), p.trigger_mode, 0.0);
        v.process(&p, &r);
        assert!(
            v.glided_key() < 60.1,
            "the next phrase glides from 60, not from mid-glide"
        );
    }

    /// The modulator's square retriggers the envelope and nothing else: the delay's fade and the
    /// auto bend belong to the keyboard trigger (`sh-2.md` §2), and an LFO edge is not a key.
    /// Round 1 of the code review found every edge restarting all three.
    #[test]
    fn an_lfo_edge_retriggers_the_envelope_and_nothing_else() {
        let mut p = Params::default();
        p.trigger_mode = TriggerMode::Lfo;
        p.lfo_rate_hz = 5.0;
        p.lfo_delay_s = 0.3;
        p.sustain = 0.2;
        p.decay_s = 0.05;
        let mut v = Voice::new();
        v.note_on(id(60), p.trigger_mode, 1.0);
        // Long enough for the key's own auto bend to decay and the fade to complete.
        render(&mut v, &p, 2.0);
        assert!(
            v.auto_bend_shape().abs() < 1e-3,
            "the key's bend has decayed"
        );
        assert!(v.lfo_fade() > 0.99, "the fade has completed");
        // Across two LFO periods: the envelope re-attacks; the bend and the fade do not move.
        let mut env_peak = 0.0f32;
        for _ in 0..(FS * 0.4) as usize {
            v.process(&p, &NONE);
            env_peak = env_peak.max(v.envelope_level());
            assert!(
                v.auto_bend_shape().abs() < 1e-3,
                "an LFO edge must not restart the auto bend"
            );
            assert!(v.lfo_fade() > 0.99, "an LFO edge must not empty the fade");
        }
        assert!(
            env_peak > 0.5,
            "the envelope re-attacked from sustain: peak {env_peak}"
        );
    }

    /// The process status: a key down has no finite tail in any keyed mode, and a released
    /// GATE note's tail is the gate's slope plus the settle — not the envelope's release, which
    /// runs on behind a closed amplifier. Round 1 of the code review, two findings.
    #[test]
    fn a_held_key_has_no_tail_and_a_gate_tail_ignores_the_hidden_release() {
        for mode in [VcaMode::Env, VcaMode::Gate] {
            let mut p = Params::default();
            p.vca_mode = mode;
            p.release_s = 10.0;
            let mut v = Voice::new();
            v.note_on(id(60), p.trigger_mode, 0.0);
            render(&mut v, &p, 0.1);
            assert_eq!(
                v.tail_samples(p.release_s, mode),
                None,
                "{mode:?}: a key down cannot end on its own"
            );
            v.note_off(None, 0, 60);
            v.process(&p, &NONE);
            let tail = v.tail_samples(p.release_s, mode).expect("released: finite");
            let settle = (POST_TAIL_S * FS) as u32;
            match mode {
                VcaMode::Gate => assert!(
                    tail > settle && tail <= (GATE_TIME_S * FS) as u32 + settle + 1,
                    "GATE: {tail}"
                ),
                _ => assert!(tail > 5 * settle, "ENV with a 10 s release: {tail}"),
            }
            // And the voice goes idle when the *audible* tail ends: GATE once the gate's slope
            // has reached the floor and the settle has run — the coupling stage sits before
            // the amplifier, so nothing else is audible after it — a little over half a
            // second; ENV not until the release has run (round 2's finding).
            render(&mut v, &p, POST_TAIL_S + 0.4);
            match mode {
                VcaMode::Gate => assert!(
                    !v.is_active(),
                    "GATE: idle after the gate's slope and the settle, not the hidden release"
                ),
                _ => assert!(v.is_active(), "ENV: still releasing"),
            }
        }
    }

    /// A held GATE note with every level down keeps its envelope: it never idles, so raising a
    /// level brings the sound back with the envelope at its sustain, still on the filter and the
    /// pulse width (round 3's finding: idling silenced the shared envelope under a held key).
    #[test]
    fn a_held_gate_note_with_nothing_fed_keeps_its_envelope() {
        let mut p = Params::default();
        p.vca_mode = VcaMode::Gate;
        p.vco1_level = 0.0;
        let mut v = Voice::new();
        v.note_on(id(60), p.trigger_mode, 0.0);
        render(&mut v, &p, POST_TAIL_S + 0.5);
        assert!(v.is_active(), "a key down never idles");
        assert_eq!(v.tail_samples(p.release_s, p.vca_mode), None);
        assert!(
            v.envelope_level() > 0.9,
            "the envelope holds at sustain: {}",
            v.envelope_level()
        );
        p.vco1_level = 1.0;
        let x = render(&mut v, &p, 0.2);
        assert!(peak(&x) > 0.05, "the level up brings the sound back");
        assert!(
            v.envelope_level() > 0.9,
            "with the envelope still at sustain"
        );
    }

    /// A HOLD patch with every level down still has one source: the filter itself, past the
    /// excitation threshold. From idle, selecting HOLD with the resonance up must wake the voice
    /// to the filter singing, not render zeros forever (round 2's finding).
    #[test]
    fn an_idle_hold_wakes_to_the_filter_singing() {
        let mut p = Params::default();
        p.vco1_level = 0.0;
        p.cutoff = 0.5;
        let mut v = Voice::new();
        render(&mut v, &p, 0.2);
        assert!(!v.is_active(), "nothing fed and nothing open: idle");
        p.resonance = 0.95;
        p.vca_mode = VcaMode::Hold;
        let x = render(&mut v, &p, 3.0);
        let last = &x[(FS * 2.0) as usize..];
        assert!(
            peak(last) > 0.01,
            "the filter should sing on its own in HOLD: peak {}",
            peak(last)
        );
        assert_eq!(v.tail_samples(p.release_s, p.vca_mode), None, "a drone");
    }

    /// `sh-2.md` §7.4: C14 into R69 is a first-order high-pass at 5.9 Hz (derived). At the
    /// keyboard's lowest F two octaves down — 10.9 Hz if the 8' bottom is F1, which the
    /// page does not record — the analytic loss is 20·log10(f / √(f² + fc²)) = −1.1 dB,
    /// the machine's own. mono-01's 15 Hz, which this crate copied, costs 4.6 dB there:
    /// the sabotage this test fails.
    #[test]
    fn the_coupling_is_the_vcas_and_costs_the_bottom_of_32_feet_a_decibel() {
        let loss_db = |corner: f32, freq: f32| {
            let mut dc = DcBlocker::new();
            dc.set_corner(corner, FS);
            let settle = (FS * 2.0) as usize;
            let measure = (FS / freq * 3.0) as usize;
            let mut peak = 0.0f32;
            for n in 0..settle + measure {
                let y = dc.process((2.0 * std::f32::consts::PI * freq * n as f32 / FS).sin());
                if n >= settle {
                    peak = peak.max(y.abs());
                }
            }
            20.0 * peak.log10()
        };
        let analytic =
            |corner: f32, freq: f32| 20.0 * (freq / (freq * freq + corner * corner).sqrt()).log10();

        let bottom_f = key_to_hz(29.0 + 12.0 * Range::Ft32.octaves());
        assert!(
            (bottom_f - 10.9).abs() < 0.05,
            "the lowest F at 32': {bottom_f:.2} Hz"
        );
        let measured = loss_db(VCA_COUPLING_HZ, bottom_f);
        let expected = analytic(VCA_COUPLING_HZ, bottom_f);
        println!(
            "coupling at {VCA_COUPLING_HZ} Hz: {measured:.2} dB at {bottom_f:.1} Hz (analytic \
             {expected:.2}), {:.2} dB an octave up; 15 Hz would cost {:.2} dB",
            loss_db(VCA_COUPLING_HZ, 2.0 * bottom_f),
            analytic(15.0, bottom_f)
        );
        assert!(
            (measured - expected).abs() < 0.2,
            "the stage must be the schematic's first-order corner: {measured:.2} dB against \
             {expected:.2}"
        );
        assert!(
            measured > -1.5,
            "the bottom of the 32' range must lose no more than the machine's own coupling \
             takes: {measured:.2} dB"
        );
        assert!(
            loss_db(VCA_COUPLING_HZ, 2.0 * bottom_f) > -0.5,
            "an octave up the coupling must be all but transparent"
        );
    }

    /// A narrow pulse carries an offset. On the machine the coupling capacitor is between
    /// the filter and the VCA (`sh-2.md` §7.4), so what the amplifier gates has no offset
    /// left, and once the envelope has closed it the output is exactly zero — no decaying
    /// thump from a coupling stage *after* the gain watching the level fall, which is what
    /// the stage's earlier place after the amplifier produced.
    #[test]
    fn the_coupling_sits_before_the_amplifier_so_a_closed_amplifier_leaves_no_tail() {
        let mut p = Params::default();
        p.vco1_wave = Wave1::Pulse;
        p.pulse_width = 0.1;
        p.release_s = 0.02;
        let mut v = Voice::new();
        v.note_on(id(48), p.trigger_mode, 0.0);
        let held = render(&mut v, &p, 0.3);
        assert!(peak(&held) > 0.1, "the note sounded");
        v.note_off(None, 0, 48);
        // The envelope's own tail, plus 10 ms so the last step is behind us.
        let tail = v.envelope.tail_samples(p.release_s) as usize + (FS * 0.01) as usize;
        for _ in 0..tail {
            v.process(&p, &NONE);
        }
        // A 5.9 Hz stage after the amplifier would still be ringing for the next 50 ms.
        let after = render(&mut v, &p, 0.05);
        assert!(
            after.iter().all(|&y| y == 0.0),
            "peak {} after the amplifier closed; the coupling must sit before it",
            peak(&after)
        );
    }

    #[test]
    fn silence_in_gives_exactly_zero_out_and_reset_leaves_no_tail() {
        let p = Params::default();
        let mut v = Voice::new();
        for _ in 0..10_000 {
            assert_eq!(v.process(&p, &NONE), 0.0);
        }
        v.note_on(id(60), p.trigger_mode, 0.0);
        render(&mut v, &p, 0.5);
        v.reset();
        for _ in 0..10_000 {
            assert_eq!(v.process(&p, &NONE), 0.0, "reset must leave no tail");
        }
    }

    #[test]
    fn no_nan_and_bounded_across_a_parameter_sweep() {
        for fs in [44_100.0f32, 48_000.0, 96_000.0, 192_000.0] {
            let mut v = Voice::new();
            v.set_sample_rate(fs);
            let mut p = Params::default();
            p.vco1_level = 1.0;
            p.vco2_level = 1.0;
            p.sub_level = 1.0;
            p.resonance = 1.0;
            // The machine's own eight routes, every one at full.
            let mut r = Routing::machine();
            for (t, s) in crate::routing::MACHINE {
                r.amounts[t][s] = 1.0;
            }
            v.set_topology(&r);
            p.vco1_range = Range::Ft2;
            p.vco2_range = Range::Ft32;
            v.note_on(id(120), p.trigger_mode, 1.0);
            let mut peak_seen = 0.0f32;
            for i in 0..(fs * 2.0) as usize {
                p.cutoff = (i as f32 / (fs * 2.0)).fract();
                let y = v.process(&p, &r);
                assert!(y.is_finite(), "non-finite at {fs}");
                peak_seen = peak_seen.max(y.abs());
            }
            assert!(
                peak_seen < OUTPUT_BOUND,
                "peak {peak_seen} against {OUTPUT_BOUND} at {fs}"
            );
        }
    }

    #[test]
    fn two_instances_render_identically() {
        let mut p = Params::default();
        p.vco2_wave = Wave2::Noise;
        p.vco2_level = 1.0;
        p.lfo_mode = Mode::Random;
        let r = wired(&[(target::CUTOFF, source::LFO, 1.0)]);
        let mut a = Voice::new();
        let mut b = Voice::new();
        a.note_on(id(60), p.trigger_mode, 0.0);
        b.note_on(id(60), p.trigger_mode, 0.0);
        assert_eq!(
            render_routed(&mut a, &p, &r, 0.5),
            render_routed(&mut b, &p, &r, 0.5)
        );
    }
}
