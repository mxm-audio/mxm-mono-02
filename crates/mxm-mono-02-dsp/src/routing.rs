//! What mxm-mono-02 can modulate, and with what.
//!
//! `plans/plan-mxm-mono-02-modulation.md` §2 and §3. The shared machinery is [`mxm_modulation`]; this
//! module is the instrument's own declaration — its **source list**, its **target list**, each
//! pair's **full scale**, the one law that is this machine's rather than the collection's, and
//! **which routes the init patch holds**. Nothing in the voice asks whether a route is the machine's
//! own: [`MACHINE`] is the whole of that, and [`INIT_PRESENT`] which of them a fresh instance shows.
//!
//! # The machine's own wiring is the init patch
//!
//! The SH-2 hard-wired nine modulation paths behind nine depth controls and two source switches:
//! the modulator on pitch and on the cutoff, the auto bend's dip, the pulse-width section's LFO and
//! envelope positions, the envelope on the cutoff in either polarity, the follower, keyboard
//! tracking and the bender on the cutoff. Eight are [`MACHINE`] routes, at the scale their controls
//! always had, so a route at depth *d* reaches what the control at *d* reached. Six are present at
//! Init, **at zero depth**; **the pulse width's two are offered, not shown** (the owner, 2026-09-27:
//! the Pulse width stack starts empty), so a fresh instance sounds the machine but lists only what
//! moves pitch and cutoff. **The follower is gone**: it followed the external input, which was
//! removed (the owner, 2026-09-26), and its source and route went with it.
//!
//! # Evaluation order
//!
//! A route whose source is produced before its target reads reads **this** sample; one produced
//! after reads **last** sample's, which is the unit delay that makes a player-made cycle finite.
//! The voice publishes, in this order:
//!
//! 1. **Forward into every target**: the LFO, LFO to width, Envelope, Auto bend, Key, Velocity,
//!    Wheel, Pressure, Bend and Noise — all before the pitch sum.
//! 2. Pitch, then Pulse width, read their sums.
//! 3. **VCO-1, VCO-2 and Sub**, published after the oscillators and before the cutoff: a sample late
//!    into Pitch and Pulse width, on time into Cutoff and Amplitude.
//! 4. Cutoff, then Amplitude, read their sums.
//!
//! # Everything the machine did not have is the collection's standard
//!
//! Key, Velocity, Wheel, Pressure and Bend mean what they mean on every instrument, and a route the
//! SH-2 never had reaches what it reaches on every instrument ([`mxm_modulation::standard`];
//! `plans/plan-modulation-standard.md`). Velocity is `v − 1`, so a route from it does nothing at the
//! hardest note; Amplitude is the standard factor, silence to double.

use mxm_modulation::standard::{self, AMPLITUDE_SUM_BOUND, Law, Offer, Performance, reach};
use mxm_modulation::{Compacted, SourceFrame};

use crate::voice::{
    AUTO_BEND_SEMITONES, BEND_FILTER_OCTAVES, FILTER_ENV_OCTAVES, FILTER_LFO_OCTAVES,
    INVERTED_RATIO, KEY_TRACK_MAX, PWM_SWING, VCO_LFO_SEMITONES,
};

/// Every source this instrument can route, in **declared publication order** (module doc).
pub mod source {
    /// The modulator as its MODE switch selects it — the delayed sine, the square or the random.
    /// Bipolar.
    pub const LFO: usize = 0;
    /// The modulator's **un-delayed** sine, `0..=1`: the pulse-width section's own tap, whatever
    /// MODE selects (`sh-2.md` §3.1).
    pub const LFO_WIDTH: usize = 1;
    /// The envelope, `0..=1`.
    pub const ENVELOPE: usize = 2;
    /// The auto bend's dip, as a shape: one at the keyboard's trigger, decaying to nothing.
    pub const AUTO_BEND: usize = 3;
    /// The glided key, from middle C, over [`super::KEY_UNIT_SEMITONES`]. Signed.
    pub const KEY: usize = 4;
    /// The velocity of the press that last triggered the envelope, `v − 1`
    /// (`standard::velocity`): zero at the hardest note, and before any press.
    pub const VELOCITY: usize = 5;
    /// The mod wheel on the sounding press's channel, `0..=1`.
    pub const WHEEL: usize = 6;
    /// Channel pressure on the sounding press's channel, `0..=1`.
    pub const PRESSURE: usize = 7;
    /// The bender's lever position, signed — not its reach in semitones.
    pub const BEND: usize = 8;
    /// VCO-1 at audio rate, in whatever waveform its selector holds.
    pub const VCO1: usize = 9;
    /// VCO-2 at audio rate, noise included in its Noise position.
    pub const VCO2: usize = 10;
    /// The sub-oscillator at audio rate.
    pub const SUB: usize = 11;
    /// The voice's one noise sample, the one VCO-2 and the S&H already share.
    pub const NOISE: usize = 12;
}

/// How many sources the instrument declares.
pub const SOURCES: usize = 13;

/// Their names, in source order, for the interface and for accessibility.
pub const SOURCE_NAMES: [&str; SOURCES] = [
    "LFO",
    "LFO to width",
    "Envelope",
    "Auto bend",
    "Key",
    "Velocity",
    "Wheel",
    "Pressure",
    "Bend",
    "VCO-1",
    "VCO-2",
    "Sub",
    "Noise",
];

/// Every target this instrument declares (plan G1).
pub mod target {
    /// Both oscillators and the sub together, summed in semitones — VCO MOD's place.
    pub const PITCH: usize = 0;
    /// The one width both pulses share, summed onto the slider, then clamped `0.05..=0.5`.
    pub const PULSE_WIDTH: usize = 1;
    /// The cutoff, summed in octaves and clamped at the ceiling before the exponential (wart 12).
    pub const CUTOFF: usize = 2;
    /// The amplifier: **multiplies** whatever the VCA switch chose, `gain × max(0, 1 + Σ)`.
    pub const AMPLITUDE: usize = 3;
}

/// How many targets the instrument declares.
pub const TARGETS: usize = 4;

/// Their names, in target order.
pub const TARGET_NAMES: [&str; TARGETS] = ["Pitch", "Pulse width", "Cutoff", "Amplitude"];

/// Which of the standard's performance sources each source is — `None` for the machine's own
/// generators and the voice's audio, which keep their own meaning.
pub const PERFORMANCE: [Option<Performance>; SOURCES] = [
    None,
    None,
    None,
    None,
    Some(Performance::Key),
    Some(Performance::Velocity),
    Some(Performance::Wheel),
    Some(Performance::Pressure),
    Some(Performance::Bend),
    None,
    None,
    None,
    None,
];

/// Each target's law, for the standard's offer. The pulse width is a sum: its slider sits below
/// square, so a negative amount widens as far as a positive one narrows, and both halves are live.
pub const LAW: [Law; TARGETS] = [Law::Sum, Law::Sum, Law::Sum, Law::Factor];

/// Whether **the SH-2 itself** has this path — one of [`MACHINE`] — so its reach is the
/// machine's rather than the standard's, present at Init or not.
#[must_use]
pub const fn machine(target: usize, source: usize) -> bool {
    let mut i = 0;
    while i < MACHINE.len() {
        if MACHINE[i].0 == target && MACHINE[i].1 == source {
            return true;
        }
        i += 1;
    }
    false
}

/// Whether and how a pair is offered — `standard::offer`. Every target here sums or scales, so
/// every pair is offered on both halves.
#[must_use]
pub const fn offer(target: usize, source: usize) -> Offer {
    standard::offer(LAW[target], PERFORMANCE[source], machine(target, source))
}

/// The Key source's unit: **six octaves either side of middle C**, so every MIDI key — 0 at
/// −60 semitones, 127 at +67 — publishes inside unit magnitude and nothing is clamped. Chosen for
/// that property alone; the scale below undoes it exactly.
pub const KEY_UNIT_SEMITONES: f32 = 72.0;

/// What the Key source publishes for a glided key: `standard::key` over this instrument's unit.
#[inline]
#[must_use]
pub fn key_source(glided_key: f32) -> f32 {
    standard::key(glided_key, KEY_UNIT_SEMITONES)
}

/// Each pair's reach at an amount of one with its source at one, **per route** — so a route the
/// machine wires keeps the scale its control always had, and **a route a player adds takes the
/// collection's standard reach** (`standard::reach`).
///
/// | Target | Added | The machine's own |
/// |---|---|---|
/// | Pitch, semitones | 12, and Key 12 per octave | LFO 7, `VCO_LFO_SEMITONES`; Auto bend −12: an amount of one dips a full octave, from below |
/// | Pulse width | −0.45, narrowing from square; Key 9 % per octave | both of the section's positions, which narrowed by the one depth |
/// | Cutoff, octaves | 4 | Envelope 10, `FILTER_ENV_OCTAVES`; LFO 4, Key 1.2 × the key's octaves, Bend 2 — what the retired control reached at the widest bend range |
/// | Amplitude | 1: a route can silence or double the gain; Key 20 % per octave | — the machine never modulated its amplifier |
///
/// Before the standard, an added pitch route took the LFO's seven semitones — Pitch ← Key at full
/// was 1.17 semitones per octave — and an added cutoff route the envelope's ten octaves.
pub const FULL_SCALE: [[f32; SOURCES]; TARGETS] = {
    let key_linear = reach::KEY_LINEAR_FRACTION_PER_OCTAVE;
    let mut pitch = [reach::PITCH_SEMITONES; SOURCES];
    pitch[source::LFO] = VCO_LFO_SEMITONES;
    pitch[source::AUTO_BEND] = -AUTO_BEND_SEMITONES;
    pitch[source::KEY] =
        standard::key_scale(reach::KEY_PITCH_SEMITONES_PER_OCTAVE, KEY_UNIT_SEMITONES);
    // Narrowing, in this machine's sign: the section's own width swing is also the standard's.
    let mut width = [-PWM_SWING; SOURCES];
    width[source::KEY] = -standard::key_scale(reach::WIDTH * key_linear, KEY_UNIT_SEMITONES);
    let mut cutoff = [reach::OCTAVES; SOURCES];
    cutoff[source::ENVELOPE] = FILTER_ENV_OCTAVES;
    cutoff[source::LFO] = FILTER_LFO_OCTAVES;
    // Undoes the key source's unit: `(amount × (key − 60) / 72) × 7.2` is `amount × 1.2 × (key − 60)
    // / 12`, which is what the KYBD slider did at its top.
    cutoff[source::KEY] = KEY_TRACK_MAX * KEY_UNIT_SEMITONES / 12.0;
    cutoff[source::BEND] = BEND_FILTER_OCTAVES;
    let mut amplitude = [reach::AMPLITUDE; SOURCES];
    amplitude[source::KEY] = standard::key_scale(reach::AMPLITUDE * key_linear, KEY_UNIT_SEMITONES);
    [pitch, width, cutoff, amplitude]
};

/// **Wart 13, as a law.** The envelope's route into the cutoff reaches [`INVERTED_RATIO`] as far
/// down as it reaches up: the machine's inverting stage is not a mirror. The law is continuous
/// through zero, because both halves are zero there.
pub const INVERTED_ENVELOPE_OCTAVES: f32 = FILTER_ENV_OCTAVES * INVERTED_RATIO;

/// The scale a route applies at this amount — [`FULL_SCALE`], except the envelope's negative half
/// into the cutoff.
#[inline]
#[must_use]
pub fn scale(target: usize, source: usize, amount: f32) -> f32 {
    if target == target::CUTOFF && source == source::ENVELOPE && amount < 0.0 {
        INVERTED_ENVELOPE_OCTAVES
    } else {
        FULL_SCALE[target][source]
    }
}

/// What a route delivers at this amount with its source at one, in the target's own unit. This is
/// what the interface reads, so the number a player sees is the number the pair moves.
#[inline]
#[must_use]
pub fn reach(target: usize, source: usize, amount: f32) -> f32 {
    amount * scale(target, source, amount)
}

/// Each target's bound on its sum. Generous where the target clamps its own result — the pitch in
/// the oscillator, the width at square, the cutoff at the ceiling — and **the standard's one on the
/// amplitude**, which is what keeps the output inside `voice::OUTPUT_BOUND` whatever a player
/// routes there.
pub const BOUND: [f32; TARGETS] = [64.0, 64.0, 64.0, AMPLITUDE_SUM_BOUND];

/// The routes **the machine itself wires** — all but its ENV FOL'R position, which followed the
/// external input that is no longer here. Each reaches what its control reached.
pub const MACHINE: [(usize, usize); 8] = [
    (target::PITCH, source::LFO),
    (target::PITCH, source::AUTO_BEND),
    (target::PULSE_WIDTH, source::LFO_WIDTH),
    (target::PULSE_WIDTH, source::ENVELOPE),
    (target::CUTOFF, source::ENVELOPE),
    (target::CUTOFF, source::LFO),
    (target::CUTOFF, source::KEY),
    (target::CUTOFF, source::BEND),
];

/// The machine routes **present in the init patch**, at zero depth: all but the pulse width's
/// two, which the owner took off the Init stack (2026-09-27). An absent route at zero sounds as a
/// present one does, so Init is the machine either way.
pub const INIT_PRESENT: [(usize, usize); 6] = [
    (target::PITCH, source::LFO),
    (target::PITCH, source::AUTO_BEND),
    (target::CUTOFF, source::ENVELOPE),
    (target::CUTOFF, source::LFO),
    (target::CUTOFF, source::KEY),
    (target::CUTOFF, source::BEND),
];

/// Which sources are live into which targets, and how much of each.
///
/// **Presence is what the DSP reads.** An absent route contributes nothing whatever its amount
/// holds. **It travels beside the patch, not inside it**: `voice::Params` is `Copy` and rebuilt
/// every sample, and a grid carried there is a memcpy per sample for values that change on a
/// parameter event (`docs/code-review-notes.md` §7).
#[derive(Debug, Clone, Copy)]
pub struct Routing {
    /// Per target, per source: whether that route exists.
    pub present: [[bool; SOURCES]; TARGETS],
    /// Per target, per source: how much, signed, as a fraction of that route's scale. Only a live
    /// route's amount is filled each sample; an absent one is never read.
    pub amounts: [[f32; SOURCES]; TARGETS],
    live: [(u8, u8); TARGETS * SOURCES],
    live_len: usize,
}

impl Default for Routing {
    fn default() -> Self {
        Self::new()
    }
}

impl Routing {
    /// Nothing routed anywhere.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            present: [[false; SOURCES]; TARGETS],
            amounts: [[0.0; SOURCES]; TARGETS],
            live: [(0, 0); TARGETS * SOURCES],
            live_len: 0,
        }
    }

    /// Rebuilds the live list from [`Routing::present`]. Once per interval, never per sample, and
    /// every caller that changes `present` owes it before the next render.
    pub fn compact(&mut self) {
        self.live_len = 0;
        for (t, target) in self.present.iter().enumerate() {
            for (s, &on) in target.iter().enumerate() {
                if on {
                    self.live[self.live_len] = (t as u8, s as u8);
                    self.live_len += 1;
                }
            }
        }
    }

    /// The live pairs, `(target, source)`.
    #[inline]
    #[must_use]
    pub fn live(&self) -> &[(u8, u8)] {
        &self.live[..self.live_len]
    }

    /// Whether any live route into `target` has a depth other than zero. **Presence alone is not
    /// enough to count**: a route at zero depth carries nothing, and a predicate that read presence
    /// would let it keep a silent voice awake.
    #[must_use]
    pub fn moves(&self, target: usize) -> bool {
        self.live()
            .iter()
            .any(|&(t, s)| t as usize == target && self.amounts[target][s as usize] != 0.0)
    }

    /// What the init patch holds: [`INIT_PRESENT`], at zero depth.
    #[must_use]
    pub const fn init() -> Self {
        Self::wired_at_zero(&INIT_PRESENT)
    }

    /// Every one of the machine's own routes present, at zero depth — [`MACHINE`]: the SH-2's
    /// panel, whatever Init shows.
    #[must_use]
    pub const fn machine() -> Self {
        Self::wired_at_zero(&MACHINE)
    }

    const fn wired_at_zero(pairs: &[(usize, usize)]) -> Self {
        let mut routing = Self::new();
        let mut i = 0;
        while i < pairs.len() {
            let (t, s) = pairs[i];
            routing.present[t][s] = true;
            i += 1;
        }
        // In declared order, as `compact` builds it: the tables are not in that order.
        let mut len = 0;
        let mut t = 0;
        while t < TARGETS {
            let mut s = 0;
            while s < SOURCES {
                if routing.present[t][s] {
                    routing.live[len] = (t as u8, s as u8);
                    len += 1;
                }
                s += 1;
            }
            t += 1;
        }
        routing.live_len = len;
        routing
    }
}

/// The voice's routing state: one frame, and one compacted list per target.
#[derive(Debug, Clone)]
pub struct Graph {
    frame: SourceFrame<SOURCES>,
    live: [Compacted<SOURCES>; TARGETS],
    /// Which sources a live route reads. **A source nothing reads is not published**, which is the
    /// cheap half of the cost model; the generators still run.
    needed: [bool; SOURCES],
    /// Whether any route at all is live: with none, the voice opens no frame and takes no sum.
    any: bool,
}

impl Default for Graph {
    fn default() -> Self {
        Self::new()
    }
}

impl Graph {
    /// An empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame: SourceFrame::new(),
            live: [const { Compacted::new() }; TARGETS],
            needed: [false; SOURCES],
            any: false,
        }
    }

    /// Rebuilds which routes are live. Once per interval, never per sample.
    ///
    /// **A source that becomes needed starts from silence.** While nothing read it, nothing
    /// published it, so its slot still holds whatever it held the last time something did — which a
    /// backward route added to a running voice would read for one sample, from a different phrase
    /// and a span the host's buffers decided. Clearing the slot makes that sample a deterministic
    /// zero (`crates/mxm-modulation/AGENTS.md`, *A gated publication owes a clear*).
    pub fn set_topology(&mut self, routing: &Routing) {
        for (live, present) in self.live.iter_mut().zip(routing.present.iter()) {
            live.build(present);
        }
        self.any = self.live.iter().any(|l| !l.is_empty());
        let was_needed = self.needed;
        self.needed = [false; SOURCES];
        for present in &routing.present {
            for (needed, &on) in self.needed.iter_mut().zip(present.iter()) {
                *needed |= on;
            }
        }
        for (source, (&needed, &before)) in self.needed.iter().zip(was_needed.iter()).enumerate() {
            if needed && !before {
                self.frame.clear(source);
            }
        }
    }

    /// Whether a live route reads this source.
    #[inline]
    #[must_use]
    pub fn needs(&self, source: usize) -> bool {
        self.needed[source]
    }

    /// Whether anything is routed at all.
    #[inline]
    #[must_use]
    pub fn any_live(&self) -> bool {
        self.any
    }

    /// Whether no route is live into that target.
    #[inline]
    #[must_use]
    pub fn is_empty(&self, target: usize) -> bool {
        self.live[target].is_empty()
    }

    /// Opens a sample.
    #[inline]
    pub fn begin_sample(&mut self) {
        self.frame.begin_sample();
    }

    /// Publishes a source's value for this sample, **if a live route reads it**.
    #[inline]
    pub fn write(&mut self, source: usize, value: f32) {
        if self.needed[source] {
            self.frame.write(source, value);
        }
    }

    /// What the frame holds for a source: this sample's if published, otherwise last sample's.
    #[inline]
    #[must_use]
    pub fn read(&self, source: usize) -> f32 {
        self.frame.read(source)
    }

    /// This target's summed modulation, in its own unit.
    ///
    /// Each route is `(amount × source) × scale` — the shared crate's order — with the scale chosen
    /// by [`scale`], so the envelope's negative half into the cutoff takes the inverted reach.
    #[inline]
    #[must_use]
    pub fn sum(&self, target: usize, routing: &Routing) -> f32 {
        let amounts = &routing.amounts[target];
        let mut total = 0.0f32;
        for &source in self.live[target].sources() {
            let amount = amounts[source];
            // A route at zero depth adds exactly nothing, so it is not multiplied out — the init
            // patch's eight all sit here. No bit moves: a sum that starts at +0.0 is never −0.0, and
            // adding ±0.0 to anything else leaves it as it was.
            if amount == 0.0 {
                continue;
            }
            total += (amount * self.frame.read(source)) * scale(target, source, amount);
        }
        if total.is_finite() {
            total.clamp(-BOUND[target], BOUND[target])
        } else {
            0.0
        }
    }

    /// Clears both halves of the frame, leaving no tail between renders.
    pub fn reset(&mut self) {
        self.frame.reset();
    }
}
