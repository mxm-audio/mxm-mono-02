//! mxm-mono-02 — monophonic two-oscillator synthesizer.
//!
//! Architecture inspired by the Roland SH-2: two free-running VCOs with one waveform each, a sub
//! divided from the first, one four-pole filter with a diode clamp in its resonance path, one
//! envelope shared three ways, one modulator whose delay fades the sine and nothing else, auto
//! bend, portamento, and a low-note-priority keyboard whose trigger fires only from below. Not
//! affiliated with or endorsed by Roland.
//!
//! This file is the plugin shell: identity, parameter plumbing, and MIDI. All the signal processing
//! lives in `mxm-mono-02-dsp`, which knows nothing about nice-plug.
//!
//! # Every note event goes to the keyboard block, and the voice follows its owner
//!
//! The DSP's `Keyboard` decides what sounds — the lowest held press — and what triggers, so the
//! plugin does not keep its own idea of the sounding note. What the plugin keeps is **per-channel
//! performance state**: pitch bend, the mod wheel and channel pressure, sixteen of each. Which channel's values reach
//! the voice is the **sounding press's channel**, read from the voice's owner every sample, so an
//! ignored higher press on another channel moves nothing and a release that falls back to an older
//! press selects that press's channel — and after the last release the owner stays the last press,
//! so a release tail or a HOLD drone bends with the channel that played it.
//!
//! # No audio input
//!
//! The SH-2 has an external audio input, mixed in at a fixed level and rectified into an envelope
//! follower that could drive the cutoff (`sh-2.md` §7.5). **It is not built** (the owner,
//! 2026-09-26): the plugin advertises two configurations, stereo and mono, and neither has an input
//! of any kind — the auxiliary port that carried the jack, the follower and its routes were removed
//! together.

/// The plugin's name, and the **only** place it is written in this crate.
///
/// Everything else that names the instrument derives from here: [`NAME`], which the host shows, and
/// [`CLAP_ID`], which it remembers. A rename is this line.
macro_rules! plugin_name {
    () => {
        "mxm-mono-02"
    };
}

/// What the host displays.
pub const NAME: &str = plugin_name!();

/// The permanent CLAP identifier.
///
/// **Deliberately assembled from [`plugin_name!`] and not from `CARGO_PKG_NAME`.** Deriving it from
/// the package name would mean a future `git mv` of this directory silently changed the plugin's
/// permanent identity — no compile error, no failing test, and every preset and saved project
/// written under the old id orphaned.
pub const CLAP_ID: &str = concat!("dk.mxm.", plugin_name!());

// Public for `apps/mxm-layout-lab` (in the private archive since the split) on the
// `dynamic-layout` branch: the lab draws these real cards outside a host. Nothing else about them
// changes, and the shipped cdylib is unaffected.
pub mod editor;
pub mod params;
pub mod preset;
pub mod routes;
pub mod telemetry;

use mxm_mono_02_dsp::keyboard::NoteId;
use mxm_mono_02_dsp::routing::Routing;
use mxm_mono_02_dsp::voice::{Params as VoiceParams, Voice};
use nice_plug::prelude::*;
use params::MxmMono02Params;
use std::sync::Arc;

/// Upper bound on how many samples are rendered between event checks.
///
/// Splitting only on events is not enough: a buffer containing no MIDI at all would otherwise
/// become one arbitrarily long block, and per-sample modulation would be the only thing keeping it
/// honest.
const MAX_BLOCK_SIZE: usize = 64;

/// MIDI channels, for the per-channel bend and wheel state.
const NUM_CHANNELS: usize = 16;

/// **The collection's developer channel, off unless asked for** (`plugins/AGENTS.md`). With
/// `MXM_DEV_CC` set in the plugin's process environment when an instance is made, CC 119 selects the
/// category (0–5) or Parameters (127); CC 118 opens (≥ 64) or closes its expander, through telemetry
/// atomics; the DSP reads nothing. It exists so a script, a screenshot run or an AI can put the
/// editor in a state CLAP gives a host no way to ask for — through the player, `mxm-cli cc 119 1`.
const DEV_VIEW_CC: u8 = 119;
const DEV_DISCLOSURE_CC: u8 = 118;
const DEV_BROWSER_CC: u8 = 117;
const DEV_THEME_CC: u8 = 116;
const DEV_CC_ENV: &str = "MXM_DEV_CC";

pub struct MxmMono02 {
    params: Arc<MxmMono02Params>,
    voice: Voice,

    /// Pitch bend per channel, in `-1..=1`. CLAP delivers `0..=1` with 0.5 centred.
    bend: [f32; NUM_CHANNELS],
    /// Mod wheel per channel, `0..=1`.
    wheel: [f32; NUM_CHANNELS],
    /// Channel pressure per channel, `0..=1`: the Pressure source.
    pressure: [f32; NUM_CHANNELS],
    /// The routing the last interval ran — its topology, and each live route's amount for the sample
    /// being rendered. Kept across buffers so a newly present route can be told from a standing one.
    routing: Routing,
    /// Which of those routes are still ramping this interval (`routes::Ramps`).
    ramps: routes::Ramps,

    sample_rate: f32,

    /// DSP -> editor, atomics only. Held even with no editor open: `activate` publishes the sample
    /// rate, and the per-block stores are not worth branching on. The per-**sample** scope writes
    /// are, and `process()` skips them while `Telemetry::editor_open` is false.
    telemetry: Arc<telemetry::Telemetry>,
    /// Whether the developer channel is on: `DEV_CC_ENV` was set when this instance was made.
    dev_cc: bool,
    /// The LFO rate as its sync resolved it for this buffer, or `None` for the free rate.
    synced_lfo_hz: Option<f32>,
}

impl Default for MxmMono02 {
    fn default() -> Self {
        Self {
            params: Arc::new(MxmMono02Params::default()),
            voice: Voice::new(),
            bend: [0.0; NUM_CHANNELS],
            wheel: [0.0; NUM_CHANNELS],
            pressure: [0.0; NUM_CHANNELS],
            routing: Routing::new(),
            ramps: routes::Ramps::default(),
            sample_rate: 48_000.0,
            telemetry: telemetry::Telemetry::shared(),
            dev_cc: std::env::var_os(DEV_CC_ENV).is_some(),
            synced_lfo_hz: None,
        }
    }
}

impl MxmMono02 {
    /// Build one sample's worth of plain values from the parameter smoothers.
    ///
    /// Called per sample. Every smoother must be advanced exactly once per sample, so this is the
    /// only place they are read.
    #[inline]
    fn next_patch(&self) -> VoiceParams {
        let p = &self.params;
        // **The sounding press's channel**, or after the last release the last press's, or the
        // default before any: the voice's owner, never the latest note-on's.
        let channel = self.voice.owner().channel as usize % NUM_CHANNELS;

        VoiceParams {
            // Synced, the LFO runs at its division (`plans/plan-tempo-sync-controls.md`).
            lfo_rate_hz: self.synced_lfo_hz.unwrap_or_else(|| p.lfo_rate.value()),
            lfo_delay_s: p.lfo_delay.value(),
            lfo_mode: p.lfo_mode.value().into(),
            pulse_width: p.pulse_width.smoothed.next(),
            total_tune_cents: p.tune.smoothed.next(),
            vco1_range: p.vco1_range.value().into(),
            vco1_wave: p.vco1_wave.value().into(),
            vco1_bender: p.vco1_bender.value(),
            vco2_range: p.vco2_range.value().into(),
            vco2_wave: p.vco2_wave.value().into(),
            vco2_tune_cents: p.vco2_tune.smoothed.next() * p.vco2_tune_range.value().cents(),
            sub_level: p.sub.smoothed.next(),
            vco1_level: p.vco1.smoothed.next(),
            vco2_level: p.vco2.smoothed.next(),
            cutoff: p.cutoff.smoothed.next(),
            resonance: p.resonance.smoothed.next(),
            vca_mode: p.vca_mode.value().into(),
            attack_s: p.attack.value(),
            decay_s: p.decay.value(),
            sustain: p.sustain.smoothed.next(),
            release_s: p.release.value(),
            trigger_mode: p.trigger.value().into(),
            portamento_s: p.portamento.value(),
            // The stored controller position times the smoothed range, so a range edit under a
            // held bend ramps instead of stepping the pitch.
            bend_semitones: self.bend[channel] * p.bend_range.smoothed.next(),
            bend_position: self.bend[channel],
            mod_wheel: self.wheel[channel],
            pressure: self.pressure[channel],
            volume: p.volume.smoothed.next(),
        }
    }

    /// Resolves this interval's routing, **once per buffer**: which routes are live, resolved against
    /// last buffer's so a route that has just become present has its smoother snapped to its stored
    /// depth rather than resuming a stale ramp (`Routes::topology_from`), then each settled route's
    /// amount (`Routes::settle`). Returns whether anything is routed.
    fn begin_interval(&mut self) -> bool {
        self.routing = self.params.routes.topology_from(&self.routing);
        self.voice.set_topology(&self.routing);
        let routed = !self.routing.live().is_empty();
        if routed {
            self.params
                .routes
                .settle(&mut self.routing, &mut self.ramps);
        }
        routed
    }

    /// One sample through the plugin's own path: the patch from the smoothers, each ramping route's
    /// amount and the wheel's push on the vibrato, then the voice.
    #[inline]
    fn render_sample(&mut self, routed: bool) -> f32 {
        let patch = self.next_patch();
        if routed {
            let wheel = self.wheel[self.voice.owner().channel as usize % NUM_CHANNELS];
            self.params
                .routes
                .advance(&mut self.routing, &mut self.ramps, wheel);
        }
        self.voice.process(&patch, &self.routing)
    }

    /// Renders one block through the plugin's own per-sample path, for measurement.
    ///
    /// **A measurement seam, not a second `process()`.** It calls what `process()` calls —
    /// [`Self::begin_interval`] once, then [`Self::render_sample`] per sample — and omits the
    /// wrapper's per-block event handling and buffer plumbing. `mxm-mono-01`
    /// carries the same seam for the same reason: the routing conversion's cost lands on the smoothers
    /// and the per-sample patch, which a framework-free DSP bench cannot reach
    /// (`plans/plan-mxm-mono-02-modulation.md` §7).
    pub fn render_block_for_test(&mut self, out: &mut [f32]) {
        let routed = self.begin_interval();
        for sample in out.iter_mut() {
            *sample = self.render_sample(routed);
        }
    }

    fn handle_event(&mut self, event: NoteEvent<()>) {
        match event {
            NoteEvent::NoteOn {
                voice_id,
                channel,
                note,
                velocity,
                ..
            } => {
                // Velocity zero is a note-off by convention. Otherwise it reaches the voice as the
                // Velocity source and nothing else: the machine's keyboard had none.
                if velocity <= 0.0 {
                    self.voice.note_off(voice_id, channel, note);
                } else {
                    let id = NoteId {
                        voice_id,
                        channel,
                        key: note,
                    };
                    self.voice
                        .note_on(id, self.params.trigger.value().into(), velocity);
                }
            }

            NoteEvent::NoteOff {
                voice_id,
                channel,
                note,
                ..
            } => {
                self.voice.note_off(voice_id, channel, note);
            }

            // A targeted choke: that press retired and its sound stopped. The voice falls back to
            // the next lowest held press exactly as on a release, or cuts without a tail — in ENV
            // and GATE. In HOLD the drone belongs to no press and plays on; only All Sound Off
            // stops it (the plan's ruling, `plans/plan-mxm-mono-02.md` §5.1).
            NoteEvent::Choke {
                voice_id,
                channel,
                note,
                ..
            } => {
                self.voice.choke(voice_id, channel, note);
            }

            // **Per-note pitch, from the host's piano roll.** Addressed to the press it names;
            // the keyboard block routes it and drops one for a press it does not hold. A
            // non-finite one is dropped here, and the keyboard block refuses it again.
            NoteEvent::PolyTuning {
                voice_id,
                channel,
                note,
                tuning,
                ..
            } if tuning.is_finite() => {
                self.voice.set_expression(voice_id, channel, note, tuning);
            }

            NoteEvent::MidiPitchBend { channel, value, .. } => {
                self.bend[channel as usize % NUM_CHANNELS] = 2.0 * (value - 0.5);
            }

            // Channel pressure, kept per channel like the bend and the wheel: the Pressure source,
            // read from the owner's channel.
            NoteEvent::MidiChannelPressure {
                channel, pressure, ..
            } => {
                self.pressure[channel as usize % NUM_CHANNELS] = pressure;
            }

            NoteEvent::MidiCC {
                channel, cc, value, ..
            } => match cc {
                // The collection's developer channel, only when this instance was started with it.
                DEV_VIEW_CC if self.dev_cc => self
                    .telemetry
                    .request_view((value.clamp(0.0, 1.0) * 127.0).round() as u8),
                DEV_DISCLOSURE_CC if self.dev_cc => {
                    self.telemetry.request_disclosure(value >= 0.5);
                }
                DEV_BROWSER_CC if self.dev_cc => {
                    self.telemetry.request_browser(value >= 0.5);
                }
                // A theme by index, 0 light / 1 dark / 2 system, as the app bar's control lists
                // them. Applied to the editor and never saved: a capture run must not rewrite the
                // choice the person at the machine made.
                DEV_THEME_CC if self.dev_cc => {
                    self.telemetry
                        .request_theme((value.clamp(0.0, 1.0) * 127.0).round() as u8);
                }
                // The mod wheel pushes the vibrato route's amount at play time without writing a
                // parameter, and is the Wheel source. The machine has no wheel; this is the
                // interface addition the README discloses.
                control_change::MODULATION_MSB => {
                    self.wheel[channel as usize % NUM_CHANNELS] = value;
                }
                // All sound off: immediate silence in every mode, and the HOLD latch.
                control_change::ALL_SOUND_OFF => self.voice.all_sound_off(),
                // All notes off: every key rising at once. Never a panic; HOLD plays on.
                control_change::ALL_NOTES_OFF => {
                    self.voice.all_notes_off();
                }
                _ => {}
            },

            _ => {}
        }
    }
}

impl Plugin for MxmMono02 {
    const NAME: &'static str = crate::NAME;
    const VENDOR: &'static str = "mxm";
    const URL: &'static str = "https://mxm.dk";
    const EMAIL: &'static str = "plugins@mxm.dk";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    /// Two configurations, stereo first. **No input in either** — the collection's rule for an
    /// instrument, and the external input's port is gone (module doc). Stereo output is dual mono —
    /// the voice is monophonic and there is no widening.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: None,
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    /// `MidiCCs` rather than `Basic`: pitch bend, CC 1, CC 120 and CC 123 are all needed.
    /// Declaring MIDI input is also what makes a host's panic actually clear a stuck note.
    const MIDI_INPUT: MidiConfig = MidiConfig::MidiCCs;

    /// Smoothers advance per sample inside each event-delimited block, which already removes zipper
    /// noise; splitting a second time buys little here.
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type Editor = editor::MxmMono02Editor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        editor::create(self.params.clone(), self.telemetry.clone())
    }

    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        // A new activation starts with no tempo and nothing resolved: the first callback reports
        // the tempo, so neither the audio nor an editor frame before it shows the last session's
        // divisions (`plans/plan-tempo-sync-controls.md`).
        self.telemetry.tempo.publish(None);
        self.synced_lfo_hz = None;
        // A rate the DSP's clamps cannot hold is refused before anything changes: a NaN, or one
        // below `MIN_SAMPLE_RATE`, crosses a `clamp` bound and panics on the audio thread.
        if !buffer_config.sample_rate.is_finite()
            || buffer_config.sample_rate < mxm_mono_02_dsp::MIN_SAMPLE_RATE
        {
            return false;
        }
        self.sample_rate = buffer_config.sample_rate;
        self.voice.set_sample_rate(self.sample_rate);
        self.voice.reset();
        self.telemetry.publish_sample_rate(self.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.voice.reset();
        self.bend = [0.0; NUM_CHANNELS];
        self.wheel = [0.0; NUM_CHANNELS];
        self.pressure = [0.0; NUM_CHANNELS];
    }

    /// **A project saved before the tempo syncs** restores each Off rather than keeping this
    /// instance's, and a loaded preset's baseline gains it, so the preset stays clean
    /// (`mxm_preset::add_switches_off`).
    fn filter_state(state: &mut PluginState) {
        mxm_preset::add_switches_off(state, crate::preset::TEMPO_SYNC_IDS);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let num_samples = buffer.samples();
        let mut next_event = context.next_event();
        let mut block_start = 0usize;

        // **Routing, once per buffer**: which routes are live changes only on a parameter event.
        let routed = self.begin_interval();
        // The LFO's tempo sync, once per buffer, and the tempo in force for the editor's reading.
        let tempo = context.transport().tempo;
        self.synced_lfo_hz = self.params.synced_lfo_rate(tempo);
        self.telemetry.tempo.publish(tempo);

        while block_start < num_samples {
            let mut block_end = (block_start + MAX_BLOCK_SIZE).min(num_samples);

            // Apply everything scheduled at or before this point, then shorten the block so the
            // next event lands exactly where it should.
            loop {
                match next_event {
                    Some(event) if (event.timing() as usize) <= block_start => {
                        self.handle_event(event);
                        next_event = context.next_event();
                    }
                    Some(event) if (event.timing() as usize) < block_end => {
                        block_end = event.timing() as usize;
                        break;
                    }
                    _ => break,
                }
            }

            {
                let output = buffer.as_slice();
                let mut block_peak = 0.0f32;
                // Once per block, not per sample: the scope is filled only while an editor exists
                // to read it (`plugins/AGENTS.md`: visualization work stops when hidden).
                let scope_wanted = self.telemetry.editor_open();
                for i in block_start..block_end {
                    let sample = self.render_sample(routed);
                    if scope_wanted {
                        self.telemetry.push_scope(self.voice.mixer_output());
                    }
                    block_peak = block_peak.max(sample.abs());
                    for channel in output.iter_mut() {
                        channel[i] = sample;
                    }
                }
                self.telemetry.publish_peak(block_peak);
                self.telemetry
                    .publish_voice(self.voice.cutoff_hz(), self.voice.envelope_level());
            }

            block_start = block_end;
        }

        // The voice derives activity from its output, never from the VCA switch — a HOLD drone is
        // active because it is audible, a silenced one is idle — with one exception: a key down
        // is live whatever the output, and answers KeepAlive (the DSP's AGENTS.md says why).
        match self.voice.tail_samples(
            self.params.release.value(),
            self.params.vca_mode.value().into(),
        ) {
            None => ProcessStatus::KeepAlive,
            Some(0) => ProcessStatus::Normal,
            Some(n) => ProcessStatus::Tail(n),
        }
    }
}

impl ClapPlugin for MxmMono02 {
    /// Permanent. Reverse DNS of a domain the project owns, so it survives moving between forges.
    const CLAP_ID: &'static str = CLAP_ID;
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("A two-oscillator monophonic synthesizer with one envelope and a resonant filter");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::Instrument,
        ClapFeature::Synthesizer,
        ClapFeature::Stereo,
        ClapFeature::Mono,
    ];
}

nice_export_clap!(MxmMono02);

/// The plugin's name, checked where it escapes this crate.
#[cfg(test)]
mod identity {
    use super::{CLAP_ID, NAME};

    #[test]
    fn the_id_is_the_name_under_the_project_domain() {
        assert_eq!(CLAP_ID, format!("dk.mxm.{NAME}"));
    }

    /// `bundler.toml` names the same instrument this crate does.
    #[test]
    fn the_bundle_is_named_after_this_plugin() {
        mxm_plugin_test::bundle::is_named(env!("CARGO_MANIFEST_DIR"), env!("CARGO_PKG_NAME"), NAME);
    }
}

#[cfg(test)]
mod init_patch {
    use super::params::{MxmMono02Params, VcaKind};
    use nice_plug::prelude::Params;

    /// **Pins the rule, not the taste.** A retune of any control survives this; making a depth
    /// non-zero because it sounded nice does not.
    #[test]
    fn every_amount_starts_at_zero() {
        let p = MxmMono02Params::default();
        for (name, value) in [
            ("resonance", p.resonance.value()),
            ("portamento", p.portamento.value()),
            ("LFO delay", p.lfo_delay.value()),
            ("sub", p.sub.value()),
            ("VCO-2 level", p.vco2.value()),
            ("tune", p.tune.value()),
        ] {
            assert_eq!(value, 0.0, "{name} is an amount and must start at zero");
        }
    }

    /// The two-oscillator line of the contract: they start slightly detuned.
    #[test]
    fn the_two_oscillators_start_detuned_and_one_source_sounds() {
        let p = MxmMono02Params::default();
        assert_ne!(p.vco2_tune.value(), 0.0, "VCO-2 starts off unison");
        assert!(p.vco2_tune.value().abs() < 0.1, "but only slightly");
        assert_eq!(p.vco1.value(), 1.0, "VCO-1 is the one plain source");
        assert_eq!(p.vco2.value(), 0.0);
    }

    /// Configurations start somewhere musically useful — and **Init is not in HOLD**.
    #[test]
    fn configurations_start_somewhere_useful() {
        let p = MxmMono02Params::default();
        assert!(p.cutoff.value() >= 0.5, "the filter starts open");
        assert!(
            p.cutoff.value() < 1.0,
            "short of the top, which would waste the control"
        );
        assert_eq!(p.pulse_width.value(), 0.5, "the width starts at square");
        assert_eq!(p.vca_mode.value(), VcaKind::Env, "Init is not in HOLD");
        assert!(
            p.lfo_rate.value() > 3.0 && p.lfo_rate.value() < 8.0,
            "a vibrato rate"
        );
    }

    /// Every id the control map and the presets will depend on exists, and is unique.
    #[test]
    fn parameter_ids_are_unique() {
        let ids: Vec<String> = MxmMono02Params::default()
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate ids in {ids:?}");
    }
}

/// The path from a host's events to the voice's owner — the part nothing in the DSP crate covers.
#[cfg(test)]
mod routing {
    use super::MxmMono02;
    use nice_plug::prelude::*;

    fn note_on(plugin: &mut MxmMono02, channel: u8, note: u8) {
        plugin.handle_event(NoteEvent::NoteOn {
            timing: 0,
            voice_id: None,
            channel,
            note,
            velocity: 0.8,
        });
    }

    fn note_off(plugin: &mut MxmMono02, channel: u8, note: u8) {
        plugin.handle_event(NoteEvent::NoteOff {
            timing: 0,
            voice_id: None,
            channel,
            note,
            velocity: 0.0,
        });
    }

    /// What the wrapper does in `activate`: a smoother reads zero until it is updated, so a test
    /// that reads a patch has to do it first (mxm-kit's `docs/adding-an-instrument.md` gotcha 13).
    fn activate_smoothers(plugin: &MxmMono02) {
        for (_, ptr, _) in plugin.params.param_map() {
            unsafe { ptr._internal_update_smoother(48_000.0, true) };
        }
    }

    fn bend(plugin: &mut MxmMono02, channel: u8, value: f32) {
        plugin.handle_event(NoteEvent::MidiPitchBend {
            timing: 0,
            channel,
            value,
        });
    }

    #[test]
    fn bend_and_wheel_follow_the_sounding_press_and_an_ignored_press_moves_nothing() {
        let mut plugin = MxmMono02::default();
        activate_smoothers(&plugin);
        bend(&mut plugin, 0, 1.0); // full up on channel 0
        bend(&mut plugin, 3, 0.0); // full down on channel 3
        note_on(&mut plugin, 0, 48);
        let up = plugin.next_patch().bend_semitones;
        assert!(up > 0.0, "the sounding press's channel bends up");
        // A higher press on another channel is ignored by the block, so the bend must not move.
        note_on(&mut plugin, 3, 60);
        assert_eq!(plugin.next_patch().bend_semitones, up);
        // Releasing the lower key falls back to the higher press, whose channel bends down.
        note_off(&mut plugin, 0, 48);
        assert!(plugin.next_patch().bend_semitones < 0.0);
        // And after the last release the owner stays: still channel 3.
        note_off(&mut plugin, 3, 60);
        assert!(plugin.next_patch().bend_semitones < 0.0);
    }

    /// **A range edit under a held bend ramps the pitch; it never steps it.** The channel keeps the
    /// bender's position and the range scales it per sample, so the range is a signal
    /// (mxm-kit's `docs/code-review-notes.md` §2). Verified against the defect: with
    /// `bend_range.value()` in `next_patch`, the first sample after the edit is already the whole
    /// new range.
    #[test]
    fn a_range_edit_under_a_held_bend_ramps_rather_than_steps() {
        let mut plugin = MxmMono02::default();
        activate_smoothers(&plugin);
        note_on(&mut plugin, 0, 48);
        bend(&mut plugin, 0, 1.0);
        let before = plugin.next_patch().bend_semitones;
        assert_eq!(before, 2.0, "a full bend at the default range");
        unsafe {
            let ptr = plugin.params.bend_range.as_ptr();
            let _ = ptr._internal_set_normalized_value(0.5);
            ptr._internal_update_smoother(48_000.0, false);
        }
        let target = plugin.params.bend_range.value();
        assert_eq!(target, 12.0);

        // The declared ramp is 20 ms, so at 48 kHz the move is spread over about 960 samples.
        let per_sample = (target - before) / (0.020 * 48_000.0);
        let mut previous = before;
        let mut largest_move = 0.0f32;
        let mut settled_at = None;
        for sample in 0..4_800 {
            let bend = plugin.next_patch().bend_semitones;
            assert!(
                bend >= previous && bend <= target,
                "sample {sample}: {previous} -> {bend} is not a monotonic ramp toward {target}"
            );
            largest_move = largest_move.max(bend - previous);
            if settled_at.is_none() && bend == target {
                settled_at = Some(sample);
            }
            previous = bend;
        }
        assert!(
            largest_move <= per_sample * 1.05,
            "a {largest_move} semitone move in one sample is a step, not a {per_sample} ramp"
        );
        let settled_at = settled_at.expect("the bend never reached the new range");
        assert!(
            settled_at >= 900,
            "the new range arrived after {settled_at} samples, faster than the declared ramp"
        );
    }

    #[test]
    fn a_tuning_expression_reaches_the_press_it_names_and_no_other() {
        let mut plugin = MxmMono02::default();
        note_on(&mut plugin, 0, 48);
        plugin.handle_event(NoteEvent::PolyTuning {
            timing: 0,
            voice_id: None,
            channel: 0,
            note: 48,
            tuning: -3.5,
        });
        assert_eq!(plugin.voice.owner().expression_semitones, -3.5);
        plugin.handle_event(NoteEvent::PolyTuning {
            timing: 0,
            voice_id: None,
            channel: 0,
            note: 55,
            tuning: 7.0,
        });
        assert_eq!(
            plugin.voice.owner().expression_semitones,
            -3.5,
            "another note's expression is dropped"
        );
        note_off(&mut plugin, 0, 48);
        assert_eq!(
            plugin.voice.owner().expression_semitones,
            -3.5,
            "a release keeps the offset"
        );
    }

    /// **A non-finite tuning is dropped at the event**, and the press keeps the offset it had. A
    /// NaN in the pitch sum reaches both VCOs' phases and the sub, which never recover. The
    /// reference is the same instance never sent it.
    #[test]
    fn a_non_finite_tuning_expression_is_dropped_and_the_pitch_stays_finite() {
        let tuning = |tuning: f32| NoteEvent::PolyTuning {
            timing: 0,
            voice_id: None,
            channel: 0,
            note: 48,
            tuning,
        };
        let plugin = || {
            let mut plugin = MxmMono02::default();
            activate_smoothers(&plugin);
            note_on(&mut plugin, 0, 48);
            plugin.handle_event(tuning(3.0));
            plugin
        };
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let (mut actual, mut reference) = (plugin(), plugin());
            actual.handle_event(tuning(bad));
            assert_eq!(actual.voice.owner().expression_semitones, 3.0, "{bad}");
            let (mut heard, mut expected) = ([0.0f32; 2048], [0.0f32; 2048]);
            actual.render_block_for_test(&mut heard);
            reference.render_block_for_test(&mut expected);
            assert!(heard.iter().all(|s| s.is_finite()), "{bad}");
            assert_eq!(heard, expected, "{bad}");
            assert!(heard.iter().any(|&s| s != 0.0), "the note sounds");
        }
    }

    #[test]
    fn all_sound_off_latches_hold_and_all_notes_off_does_not() {
        let mut plugin = MxmMono02::default();
        plugin.handle_event(NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc: control_change::ALL_NOTES_OFF,
            value: 0.0,
        });
        assert!(!plugin.voice.hold_latched());
        plugin.handle_event(NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc: control_change::ALL_SOUND_OFF,
            value: 0.0,
        });
        assert!(plugin.voice.hold_latched());
        note_on(&mut plugin, 0, 60);
        assert!(
            !plugin.voice.hold_latched(),
            "a note-on is the deliberate act"
        );
    }
}

/// The host's own callback: `process()` through the wrapper's buffers.
#[cfg(test)]
mod process_callback {
    use super::MxmMono02;
    use nice_plug::prelude::*;

    struct TestContext {
        transport: Transport,
        note_on: Option<NoteEvent<()>>,
    }

    impl ProcessContext<MxmMono02> for TestContext {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        fn execute_background(&self, _task: ()) {}
        fn execute_gui(&self, _task: ()) {}
        fn transport(&self) -> &Transport {
            &self.transport
        }
        fn next_event(&mut self) -> Option<NoteEvent<()>> {
            self.note_on.take()
        }
        fn send_event(&mut self, _event: NoteEvent<()>) {}
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }

    /// One stereo callback of `frames` samples, a note-on at its first sample when asked and the
    /// host transport reporting `tempo`; the left output returned.
    fn process_at(
        plugin: &mut MxmMono02,
        frames: usize,
        press: bool,
        tempo: Option<f64>,
    ) -> Vec<f32> {
        let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
        let mut buffer = Buffer::default();
        unsafe {
            buffer.set_slices(frames, |channels| {
                channels.clear();
                channels.push(left.as_mut_slice());
                channels.push(right.as_mut_slice());
            });
        }
        let mut inputs = [];
        let mut outputs = [];
        let mut aux = AuxiliaryBuffers {
            inputs: &mut inputs,
            outputs: &mut outputs,
        };
        let mut transport = Transport::new(48_000.0);
        transport.tempo = tempo;
        let mut context = TestContext {
            transport,
            note_on: press.then_some(NoteEvent::NoteOn {
                timing: 0,
                voice_id: None,
                channel: 0,
                note: 48,
                velocity: 0.8,
            }),
        };
        plugin.process(&mut buffer, &mut aux, &mut context);
        drop(buffer);
        left
    }

    fn plugin() -> MxmMono02 {
        let plugin = MxmMono02::default();
        for (_, ptr, _) in plugin.params.param_map() {
            unsafe { ptr._internal_update_smoother(48_000.0, true) };
        }
        plugin
    }

    /// **Through `process()`, the LFO sync follows the host's tempo and its modulation, and is inert
    /// otherwise** (`plans/plan-tempo-sync-controls.md`): with a tempo, the callback resolves the
    /// division the rate's *modulated* position picks; with sync off, or no tempo, the render is the
    /// free one to the bit.
    #[test]
    fn the_lfo_sync_resolves_in_the_callback_and_is_inert_without_it() {
        use nice_plug::params::InternalParamMut;

        // Modulated from the knob's 0.2 to 0.8, synced at 120: the division is the modulated one.
        let mut synced = plugin();
        unsafe {
            let _ = synced.params.lfo_sync._internal_set_normalized_value(1.0);
            let _ = synced.params.lfo_rate._internal_set_normalized_value(0.2);
            let _ = synced.params.lfo_rate._internal_modulate_value(0.6);
        }
        process_at(&mut synced, 512, true, Some(120.0));
        let (lo, hi) = (
            f64::from(synced.params.lfo_rate.preview_plain(0.0)),
            f64::from(synced.params.lfo_rate.preview_plain(1.0)),
        );
        let expected = crate::params::LFO_SYNC
            .value(crate::params::LFO_SYNC.division(0.8, 120.0, lo, hi), 120.0)
            as f32;
        assert_eq!(synced.synced_lfo_hz, Some(expected));

        // Sync off with a tempo, and sync on without one, both render exactly as free.
        let free = process_at(&mut plugin(), 512, true, None);
        assert_eq!(process_at(&mut plugin(), 512, true, Some(120.0)), free);
        let mut no_tempo = plugin();
        unsafe {
            let _ = no_tempo.params.lfo_sync._internal_set_normalized_value(1.0);
        }
        assert_eq!(process_at(&mut no_tempo, 512, true, None), free);
        assert_eq!(no_tempo.synced_lfo_hz, None);
    }
}

#[cfg(test)]
mod developer_channel_tests {
    use super::*;

    fn cc(plugin: &mut MxmMono02, cc: u8, raw: u8) {
        plugin.handle_event(NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc,
            value: f32::from(raw) / 127.0,
        });
    }

    /// The developer channel reaches the editor only when the instance was started with it; a
    /// host sending the same control change to an ordinary instance changes nothing.
    #[test]
    fn the_developer_channel_is_off_unless_the_environment_asked_for_it() {
        let mut plugin = MxmMono02 {
            dev_cc: false,
            ..Default::default()
        };
        cc(&mut plugin, DEV_VIEW_CC, 1);
        cc(&mut plugin, DEV_DISCLOSURE_CC, 127);
        cc(&mut plugin, DEV_BROWSER_CC, 127);
        cc(&mut plugin, DEV_THEME_CC, 1);
        assert_eq!(plugin.telemetry.take_view_request(), None);
        assert_eq!(plugin.telemetry.take_disclosure_request(), None);
        assert_eq!(plugin.telemetry.take_browser_request(), None);
        assert_eq!(plugin.telemetry.take_theme_request(), None);

        plugin.dev_cc = true;
        cc(&mut plugin, DEV_VIEW_CC, 1);
        cc(&mut plugin, DEV_DISCLOSURE_CC, 127);
        cc(&mut plugin, DEV_BROWSER_CC, 127);
        cc(&mut plugin, DEV_THEME_CC, 1);
        assert_eq!(plugin.telemetry.take_view_request(), Some(1));
        assert_eq!(plugin.telemetry.take_disclosure_request(), Some(true));
        assert_eq!(plugin.telemetry.take_browser_request(), Some(true));
        assert_eq!(
            plugin.telemetry.take_theme_request(),
            Some(1),
            "1 is dark, as mxm_ui::theme::from_index reads it"
        );
        // Light is index 0 — a request like any other, not the absence of one.
        cc(&mut plugin, DEV_THEME_CC, 0);
        assert_eq!(plugin.telemetry.take_theme_request(), Some(0));
        // View zero is a request too, not the absence of one.
        cc(&mut plugin, DEV_VIEW_CC, 0);
        assert_eq!(plugin.telemetry.take_view_request(), Some(0));
    }
}

/// **M0 of `plans/plan-mxm-mono-02-modulation.md`: what the routing conversion is measured against.**
///
/// Captured before the conversion and recorded in `BASELINE-M0.md`, because afterwards the old
/// renders cannot be produced. Measurements, not assertions, so every test is `#[ignore]`d:
///
/// ```bash
/// cargo test -p mxm-mono-02 --release baseline -- --ignored --nocapture --test-threads=1
/// ```
///
/// With `MXM_M0_DUMP=<dir>` the bank's renders are also written there as raw little-endian `f32`,
/// and `the_bank_against_the_m0_dump` compares a later build against them sample by sample — the
/// worst absolute and relative difference, which is what plan §4 says M4 reports.
#[cfg(test)]
mod baseline {
    use super::*;
    use std::time::Instant;

    const FS: f32 = 48_000.0;
    const BLOCK: usize = 64;

    /// A plugin with every smoother activated (mxm-kit's `docs/adding-an-instrument.md` gotcha 13)
    /// and the voice at the rate `activate` would give it.
    fn plugin() -> MxmMono02 {
        let mut plugin = MxmMono02::default();
        for (_, ptr, _) in plugin.params.param_map() {
            unsafe { ptr._internal_update_smoother(FS, true) };
        }
        plugin.sample_rate = FS;
        plugin.voice.set_sample_rate(FS);
        plugin.voice.reset();
        plugin
    }

    fn note(plugin: &mut MxmMono02, on: bool, note: u8) {
        plugin.handle_event(if on {
            NoteEvent::NoteOn {
                timing: 0,
                voice_id: None,
                channel: 0,
                note,
                velocity: 0.8,
            }
        } else {
            NoteEvent::NoteOff {
                timing: 0,
                voice_id: None,
                channel: 0,
                note,
                velocity: 0.0,
            }
        });
    }

    fn throughput(label: &str, plugin: &mut MxmMono02) {
        note(plugin, true, 48);
        let mut out = [0.0f32; BLOCK];
        // Warm the caches: the first blocks pay for page faults, which is not the question.
        for _ in 0..64 {
            plugin.render_block_for_test(&mut out);
        }
        let blocks = 40_000;
        let start = Instant::now();
        for _ in 0..blocks {
            plugin.render_block_for_test(&mut out);
        }
        let taken = start.elapsed().as_secs_f64();
        let samples = (blocks * BLOCK) as f64;
        println!(
            "  mxm-mono-02, {label}, held note: {:.3} ns/sample ({:.0} samples/s)",
            taken * 1e9 / samples,
            samples / taken
        );
    }

    /// Per-sample cost through the plugin's own path: on Init, on Init with nothing routed, and on one
    /// fixed routed patch. **A figure counts only from a quiet machine, measured alongside the M0
    /// revision's under the same conditions** (`BASELINE-M0.md`).
    ///
    /// **The routed patch is `Random arp`.** Every factory sound overrides at most two retiring
    /// controls; of those this one reaches two targets — the LFO on pitch and the envelope on the
    /// cutoff — with the LFO retriggering the envelope, so it is the one that exercises most of what
    /// the conversion rewrites.
    #[test]
    #[ignore = "a measurement, not an assertion; release only"]
    fn throughput_of_init_and_a_routed_patch() {
        use nice_plug::params::InternalParamMut;
        println!();
        throughput("Init", &mut plugin());
        // The old free case: even the machine's own routes absent, so no frame is opened and no sum
        // taken. Its distance from Init is what the init patch's zero-depth routes cost.
        let bare = plugin();
        for group in bare.params.routes.each() {
            for present in group.presence_params() {
                unsafe { present._internal_set_plain_value(false) };
            }
        }
        throughput("Init, nothing routed", &mut { bare });
        let mut routed = plugin();
        apply(&routed, factory("Random arp"));
        throughput("Random arp", &mut routed);
        println!();
    }

    /// FNV-1a over the raw bits, as mxm-mono-01's
    /// `plugins/mxm-mono-01/host-tests/tests/golden_audio.rs` computes it.
    fn digest(samples: &[f32]) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for sample in samples {
            for byte in sample.to_bits().to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        format!("{hash:016x}")
    }

    /// One note, held 1.5 s then released with 2.5 s of tail — the same gesture for every sound, so
    /// a digest change is the patch's and not the gesture's.
    fn render_one(plugin: &mut MxmMono02) -> Vec<f32> {
        let held = (FS * 1.5) as usize;
        let tail = (FS * 2.5) as usize;
        let mut out = vec![0.0f32; held + tail];
        note(plugin, true, 48);
        let (a, b) = out.split_at_mut(held);
        for block in a.chunks_mut(BLOCK) {
            plugin.render_block_for_test(block);
        }
        note(plugin, false, 48);
        for block in b.chunks_mut(BLOCK) {
            plugin.render_block_for_test(block);
        }
        out
    }

    fn factory(name: &str) -> &'static str {
        crate::preset::FACTORY_FILES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, text)| *text)
            .unwrap_or_else(|| panic!("no factory sound {name:?}"))
    }

    /// Applies a factory file's stored values by id — **only `v` is read**, as the preset system
    /// reads it — then snaps every smoother, as a fresh load would settle.
    fn apply(plugin: &MxmMono02, json: &str) -> usize {
        let map = plugin.params.param_map();
        let mut applied = 0;
        for (id, ptr, _) in &map {
            let key = format!("\"{id}\"");
            let Some(at) = json.find(&key) else { continue };
            let rest = &json[at + key.len()..];
            let Some(vpos) = rest.find("\"v\"") else {
                continue;
            };
            let number: String = rest[vpos + 3..]
                .chars()
                .skip_while(|c| *c == ':' || c.is_whitespace())
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == 'e')
                .collect();
            if let Ok(v) = number.parse::<f32>() {
                unsafe { ptr._internal_set_normalized_value(v) };
                applied += 1;
            }
        }
        for (_, ptr, _) in &map {
            unsafe { ptr._internal_update_smoother(FS, true) };
        }
        applied
    }

    fn slug(name: &str) -> String {
        name.to_lowercase().replace(' ', "-")
    }

    /// Init first, then the fifty in shipped order.
    fn bank() -> Vec<(&'static str, Vec<f32>, usize)> {
        let mut out = Vec::new();
        let mut init = plugin();
        out.push(("Init", render_one(&mut init), 0));
        for (name, json) in crate::preset::FACTORY_FILES {
            let mut p = plugin();
            let applied = apply(&p, json);
            out.push((name, render_one(&mut p), applied));
        }
        out
    }

    /// A digest and a peak for Init and every factory sound; with `MXM_M0_DUMP` set, the renders too.
    #[test]
    #[ignore = "a measurement, not an assertion; release only"]
    fn the_bank_digests() {
        let dump = std::env::var_os("MXM_M0_DUMP").map(std::path::PathBuf::from);
        println!();
        for (name, samples, applied) in bank() {
            let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            println!(
                "  | `{}` | `{}` | {peak:.4} | {applied} |",
                slug(name),
                digest(&samples)
            );
            if let Some(dir) = &dump {
                let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                std::fs::write(dir.join(format!("{}.f32", slug(name))), bytes).expect("dump");
            }
        }
        println!();
    }

    /// Sample-by-sample difference from the M0 renders in `MXM_M0_DUMP`: the worst absolute
    /// difference, and the worst relative to that sound's own peak.
    #[test]
    #[ignore = "a comparison against a local dump, not an assertion"]
    fn the_bank_against_the_m0_dump() {
        let Some(dir) = std::env::var_os("MXM_M0_DUMP").map(std::path::PathBuf::from) else {
            println!("set MXM_M0_DUMP to the directory the_bank_digests wrote");
            return;
        };
        println!();
        let (mut worst_abs, mut worst_rel, mut moved) = (0.0f32, 0.0f32, 0);
        for (name, samples, _) in bank() {
            let bytes = std::fs::read(dir.join(format!("{}.f32", slug(name)))).expect("dump");
            let before: Vec<f32> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            assert_eq!(before.len(), samples.len(), "{name}: length changed");
            let diff = before
                .iter()
                .zip(&samples)
                .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
            let peak = before.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let rel = if peak > 0.0 { diff / peak } else { diff };
            if diff > 0.0 {
                moved += 1;
            }
            worst_abs = worst_abs.max(diff);
            worst_rel = worst_rel.max(rel);
            println!(
                "  {:<18} max |diff| {diff:.3e}  relative {rel:.3e}",
                slug(name)
            );
        }
        println!(
            "  moved {moved} of 51; worst |diff| {worst_abs:.3e}, worst relative {worst_rel:.3e}\n"
        );
    }
}

/// The routing's plugin half: the performance inputs that became sources, and a route that arrives
/// mid-performance rendering the same whatever the host's buffers.
#[cfg(test)]
mod routing_path {
    use super::*;

    const FS: f32 = 48_000.0;

    fn plugin() -> MxmMono02 {
        let mut plugin = MxmMono02::default();
        for (_, ptr, _) in plugin.params.param_map() {
            unsafe { ptr._internal_update_smoother(FS, true) };
        }
        plugin.sample_rate = FS;
        plugin.voice.set_sample_rate(FS);
        plugin.voice.reset();
        plugin
    }

    fn key(plugin: &mut MxmMono02, on: bool, channel: u8, note: u8, velocity: f32) {
        plugin.handle_event(if on {
            NoteEvent::NoteOn {
                timing: 0,
                voice_id: None,
                channel,
                note,
                velocity,
            }
        } else {
            NoteEvent::NoteOff {
                timing: 0,
                voice_id: None,
                channel,
                note,
                velocity: 0.0,
            }
        });
    }

    fn pressure(plugin: &mut MxmMono02, channel: u8, pressure: f32) {
        plugin.handle_event(NoteEvent::MidiChannelPressure {
            timing: 0,
            channel,
            pressure,
        });
    }

    /// **Velocity and pressure reach the voice from the press it is sounding** — and when the voice
    /// falls back to an older held press, both follow it (mxm-kit's `docs/code-review-notes.md`
    /// §2, channel-owned expression refreshing on fallback).
    #[test]
    fn velocity_and_pressure_follow_the_sounding_press_through_a_fallback() {
        let mut plugin = plugin();
        key(&mut plugin, true, 3, 60, 0.9);
        key(&mut plugin, true, 0, 48, 0.3);
        assert_eq!(plugin.voice.owner().velocity, 0.3, "the lower press sounds");
        pressure(&mut plugin, 0, 0.7);
        pressure(&mut plugin, 3, 0.2);
        assert_eq!(
            plugin.next_patch().pressure,
            0.7,
            "the sounding press's channel"
        );
        key(&mut plugin, false, 0, 48, 0.0);
        assert_eq!(
            plugin.voice.owner().velocity,
            0.9,
            "the velocity follows the press the voice falls back to"
        );
        assert_eq!(plugin.next_patch().pressure, 0.2, "and so does the channel");
    }

    /// **A route that arrives after an idle span renders the same whatever the block size** — with
    /// its depth mid-ramp and a standing route's depth moving too.
    #[test]
    fn a_route_arriving_after_an_idle_span_is_block_partition_invariant() {
        use nice_plug::params::InternalParamMut;
        let render = |block: usize| -> Vec<f32> {
            let mut plugin = plugin();
            let mut out = vec![0.0f32; 96_000];
            let (idle, played) = out.split_at_mut(48_000);
            for chunk in idle.chunks_mut(block) {
                plugin.render_block_for_test(chunk);
            }
            unsafe {
                let r = &plugin.params.routes;
                r.cutoff.vco1_on._internal_set_plain_value(true);
                r.cutoff.vco1._internal_set_plain_value(0.3);
                r.cutoff.vco1._internal_update_smoother(FS, false);
                r.pitch.lfo._internal_set_plain_value(0.2);
                r.pitch.lfo._internal_update_smoother(FS, false);
            }
            key(&mut plugin, true, 0, 48, 0.8);
            for chunk in played.chunks_mut(block) {
                plugin.render_block_for_test(chunk);
            }
            out
        };
        let a = render(64);
        assert!(
            a[48_000..].iter().any(|s| s.abs() > 1e-3),
            "the premise: it plays"
        );
        assert_eq!(a, render(37), "64 against 37");
        assert_eq!(a, render(1024), "64 against 1024");
    }
}

/// The host's sample rate at activation: the floor the DSP's clamps are safe above.
#[cfg(test)]
mod sample_rate_floor {
    use super::*;
    use mxm_mono_02_dsp::MIN_SAMPLE_RATE;

    struct Activation;

    impl ActivateContext<MxmMono02> for Activation {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        fn execute(&self, _task: ()) {}
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }

    fn activate_at(plugin: &mut MxmMono02, sample_rate: f32) -> bool {
        plugin.activate(
            &MxmMono02::AUDIO_IO_LAYOUTS[0],
            &BufferConfig {
                sample_rate,
                min_buffer_size: Some(1),
                max_buffer_size: 4096,
                process_mode: ProcessMode::Realtime,
            },
            &mut Activation,
        )
    }

    fn render(plugin: &mut MxmMono02, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        plugin.render_block_for_test(&mut out);
        out
    }

    /// **The floor activates and plays, whatever the parameters say.** Every parameter at its
    /// default, then all at the bottom of their ranges, then all at the top — every route present
    /// at full — with a note held for four seconds at 1 kHz.
    #[test]
    fn the_rate_floor_activates_and_plays_at_every_parameter_extreme() {
        for extreme in [None, Some(0.0), Some(1.0)] {
            let mut plugin = MxmMono02::default();
            for (_, ptr, _) in plugin.params.param_map() {
                if let Some(value) = extreme {
                    let _ = unsafe { ptr._internal_set_normalized_value(value) };
                }
                unsafe { ptr._internal_update_smoother(MIN_SAMPLE_RATE, true) };
            }
            assert!(activate_at(&mut plugin, MIN_SAMPLE_RATE), "{extreme:?}");
            assert_eq!(plugin.sample_rate, MIN_SAMPLE_RATE);
            plugin.handle_event(NoteEvent::NoteOn {
                timing: 0,
                voice_id: None,
                channel: 0,
                note: 48,
                velocity: 0.8,
            });
            let out = render(&mut plugin, 4_000);
            assert!(out.iter().all(|s| s.is_finite()), "{extreme:?}");
        }
    }

    /// **A rate the DSP's clamps cannot hold is refused at activation.** `f32::clamp` panics on
    /// a NaN or inverted bound, so a NaN rate or one low enough to cross a corner's floor over its
    /// Nyquist fraction panicked on the audio thread. A refusal leaves the plugin as it was.
    #[test]
    fn activation_refuses_a_non_finite_rate_and_any_below_the_floor() {
        for unsupported in [
            MIN_SAMPLE_RATE.next_down(),
            100.0,
            20.0,
            1.0,
            0.0,
            -48_000.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            let mut refused = MxmMono02::default();
            assert!(
                !activate_at(&mut refused, unsupported),
                "accepted {unsupported} Hz"
            );
            assert_eq!(refused.sample_rate, 48_000.0, "{unsupported} Hz");
        }
    }
}

/// **A synced value reaches the patch** (`plans/plan-tempo-sync-controls.md`): what a sync resolved
/// for this callback is what the DSP is given, and with none the free value is.
#[cfg(test)]
mod tempo_sync_path {
    use super::*;

    #[test]
    fn the_synced_lfo_rate_is_the_patchs() {
        let mut plugin = MxmMono02::default();
        let free = plugin.next_patch().lfo_rate_hz;
        plugin.synced_lfo_hz = Some(free + 1.0);
        assert_eq!(plugin.next_patch().lfo_rate_hz, free + 1.0);
        plugin.synced_lfo_hz = None;
        assert_eq!(plugin.next_patch().lfo_rate_hz, free);
    }
}

/// **Activation forgets the last session's tempo and resolved syncs**: the first callback reports the
/// tempo, so nothing — the audio, or an editor frame before it — starts from the previous session's
/// divisions.
#[cfg(test)]
mod activation_forgets_the_tempo {
    use super::*;

    #[test]
    fn activation_forgets_the_last_tempo_and_resolved_syncs() {
        use nice_plug::prelude::Plugin as _;
        let mut plugin = MxmMono02::default();
        plugin.telemetry.tempo.publish(Some(120.0));
        plugin.synced_lfo_hz = Some(1.0);
        let layout = MxmMono02::AUDIO_IO_LAYOUTS[0];
        let config = BufferConfig {
            sample_rate: 48_000.0,
            min_buffer_size: None,
            max_buffer_size: 512,
            process_mode: ProcessMode::Realtime,
        };
        let _ = plugin.activate(&layout, &config, &mut NoInit);
        assert_eq!(plugin.telemetry.tempo.get(), None);
        assert_eq!(plugin.synced_lfo_hz, None);
    }

    /// An activation context that asks nothing of a host.
    struct NoInit;

    impl ActivateContext<MxmMono02> for NoInit {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        fn execute(&self, _task: <MxmMono02 as Plugin>::BackgroundTask) {}
        fn set_latency_samples(&self, _samples: u32) {}
        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }
}

/// What a player reads — on hover in the editor, and in a host's plugin browser — speaks to the
/// player about the sound, never about the machine or the code (`mxm_plugin_test::hover_text`).
#[cfg(test)]
mod speaks_to_the_player {
    #[test]
    fn hover_text() {
        mxm_plugin_test::hover_text::speaks_to_the_player(env!("CARGO_MANIFEST_DIR"));
    }

    #[test]
    fn host_description() {
        mxm_plugin_test::hover_text::host_description_speaks_to_the_player(env!(
            "CARGO_MANIFEST_DIR"
        ));
    }
}
