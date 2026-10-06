//! Parameter definitions.
//!
//! Every `#[id]` here is **permanent**. Changing one breaks every saved project that used the
//! plugin, so ids are part of the public interface.
//!
//! # The panel is the hardware's controls, in its signal flow
//!
//! Modulator · VCO · Mixer · VCF · VCA · ENV · Voice — every control the SH-2 has, and nothing it
//! does not (`research:instruments/sh-2.md` §3, §10). The interface improvements the plan permits are
//! the ones the circuit does not forbid: **every depth the machine hard-wired is a route**
//! (`crate::routes`), present in the init patch at zero, and the bender's range is disclosed rather
//! than hidden on the left cheek.
//!
//! # What is one parameter because the circuit makes it one
//!
//! One pulse width for **both** oscillators: the SH-2 has one comparator reference, and two pulses on
//! it never differ in duty (wart 3). Its LFO and envelope positions are two routes into it now.
//!
//! # Two collection contracts
//!
//! **Every amount starts at zero**: every route's depth, resonance, portamento, the delay —
//! **Every configuration starts somewhere useful**: ranges at 8', both
//! waveforms saw, the width at square, the trigger at GATE+TRIG, the VCA on the envelope, the
//! modulator at a vibrato rate. **Two oscillators start detuned**: VCO-2's tune is off centre with
//! VCO-2's level at zero, so raising VCO-2 beats at once.
//!
//! **Smooth signals, not coefficients.** Depths, levels, the cutoff, resonance, the width and the
//! tunes are smoothed; the envelope's times, the portamento's time, the modulator's rate and every
//! switch are not — they set state-machine behaviour, and smoothing them makes it impossible to
//! reason about.

use mxm_mono_02_dsp::lfo::{self, Mode};
use mxm_mono_02_dsp::oscillator::{Wave1, Wave2};
use mxm_mono_02_dsp::voice::{Range, TriggerMode, VcaMode};
use nice_plug::prelude::*;
use std::sync::{Arc, RwLock};

/// The RANGE rotary: exact octave steps.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeKind {
    #[id = "32"]
    #[name = "32'"]
    Ft32,
    #[id = "16"]
    #[name = "16'"]
    Ft16,
    #[id = "8"]
    #[name = "8'"]
    Ft8,
    #[id = "4"]
    #[name = "4'"]
    Ft4,
    #[id = "2"]
    #[name = "2'"]
    Ft2,
}

impl From<RangeKind> for Range {
    fn from(r: RangeKind) -> Self {
        match r {
            RangeKind::Ft32 => Range::Ft32,
            RangeKind::Ft16 => Range::Ft16,
            RangeKind::Ft8 => Range::Ft8,
            RangeKind::Ft4 => Range::Ft4,
            RangeKind::Ft2 => Range::Ft2,
        }
    }
}

/// VCO-1's selector: one waveform at a time, and the sine is only here.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wave1Kind {
    #[id = "sine"]
    #[name = "Sine"]
    Sine,
    #[id = "saw"]
    #[name = "Sawtooth"]
    Saw,
    #[id = "square"]
    #[name = "Square"]
    Square,
    #[id = "pulse"]
    #[name = "Pulse"]
    Pulse,
}

impl From<Wave1Kind> for Wave1 {
    fn from(w: Wave1Kind) -> Self {
        match w {
            Wave1Kind::Sine => Wave1::Sine,
            Wave1Kind::Saw => Wave1::Saw,
            Wave1Kind::Square => Wave1::Square,
            Wave1Kind::Pulse => Wave1::Pulse,
        }
    }
}

/// VCO-2's selector: noise replaces the oscillator, and there is no sine.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wave2Kind {
    #[id = "noise"]
    #[name = "Noise"]
    Noise,
    #[id = "saw"]
    #[name = "Sawtooth"]
    Saw,
    #[id = "square"]
    #[name = "Square"]
    Square,
    #[id = "pulse"]
    #[name = "Pulse"]
    Pulse,
}

impl From<Wave2Kind> for Wave2 {
    fn from(w: Wave2Kind) -> Self {
        match w {
            Wave2Kind::Noise => Wave2::Noise,
            Wave2Kind::Saw => Wave2::Saw,
            Wave2Kind::Square => Wave2::Square,
            Wave2Kind::Pulse => Wave2::Pulse,
        }
    }
}

/// VCO-2's TUNE RANGE switch: how far the tune knob reaches.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuneRange {
    #[id = "narrow"]
    #[name = "Narrow"]
    Narrow,
    #[id = "wide"]
    #[name = "Wide"]
    Wide,
}

impl TuneRange {
    /// The reach in cents either side of unison (`sh-2.md` §3.4).
    pub fn cents(self) -> f32 {
        match self {
            TuneRange::Narrow => 200.0,
            TuneRange::Wide => 1750.0,
        }
    }
}

/// The modulator's MODE switch.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoKind {
    #[id = "sine"]
    #[name = "Sine"]
    Sine,
    #[id = "square"]
    #[name = "Square"]
    Square,
    #[id = "random"]
    #[name = "Random"]
    Random,
}

impl From<LfoKind> for Mode {
    fn from(k: LfoKind) -> Self {
        match k {
            LfoKind::Sine => Mode::Sine,
            LfoKind::Square => Mode::Square,
            LfoKind::Random => Mode::Random,
        }
    }
}

/// The VCA switch.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcaKind {
    #[id = "hold"]
    #[name = "Hold"]
    Hold,
    #[id = "env"]
    #[name = "Envelope"]
    Env,
    #[id = "gate"]
    #[name = "Gate"]
    Gate,
}

impl From<VcaKind> for VcaMode {
    fn from(k: VcaKind) -> Self {
        match k {
            VcaKind::Hold => VcaMode::Hold,
            VcaKind::Env => VcaMode::Env,
            VcaKind::Gate => VcaMode::Gate,
        }
    }
}

/// The envelope's trigger switch.
#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    #[id = "gatetrig"]
    #[name = "Gate + trig"]
    GateTrig,
    #[id = "gate"]
    #[name = "Gate"]
    Gate,
    #[id = "lfo"]
    #[name = "LFO"]
    Lfo,
}

impl From<TriggerKind> for TriggerMode {
    fn from(k: TriggerKind) -> Self {
        match k {
            TriggerKind::GateTrig => TriggerMode::GateTrig,
            TriggerKind::Gate => TriggerMode::Gate,
            TriggerKind::Lfo => TriggerMode::Lfo,
        }
    }
}

/// Formats a parameter value for display.
type ValueToString = Arc<dyn Fn(f32) -> String + Send + Sync>;
/// Parses a typed-in value, returning `None` if it cannot be understood.
type StringToValue = Arc<dyn Fn(&str) -> Option<f32> + Send + Sync>;

/// Format seconds as milliseconds below a second, seconds above.
///
/// **The unit is chosen from what the millisecond text would round to, not from the raw value.**
/// A host parses the text and normalises it before printing it again, so the value it prints lands a
/// hair either side of where it started: switching at the raw second printed `0.9996 s` as
/// `1000 ms`, which parses to one second and prints `1.00 s`, and `clap-validator`'s
/// `param-conversions` fails whenever its values land there. Anything that would print `1000 ms`
/// prints seconds instead.
fn v2s_time() -> ValueToString {
    Arc::new(|s| {
        if (s * 1_000.0).round() >= 1_000.0 {
            format!("{s:.2} s")
        } else {
            format!("{:.0} ms", s * 1000.0)
        }
    })
}

/// A number's text **without a negative zero**: `-0` becomes `0`, and every other text is left as
/// it is.
///
/// A host parses `-0` to zero and prints `0`, so a reading that shows it is not idempotent through
/// the host's conversion. Decided from the text rather than from the value, because `{:.0}` rounds
/// an exact `-0.5` to the even `-0` where `f32::round` takes it to `-1` — which is how nice-plug's
/// `v2s_f32_rounded(0)` still printed `-0 cents` on Tune.
fn without_negative_zero(text: String) -> String {
    match text.strip_prefix('-') {
        Some(digits) if digits.bytes().all(|b| b == b'0' || b == b'.') => digits.to_owned(),
        _ => text,
    }
}

/// Whole units, for a range that crosses zero — [`without_negative_zero`].
fn v2s_whole() -> ValueToString {
    Arc::new(|v| without_negative_zero(format!("{v:.0}")))
}

fn s2v_time() -> StringToValue {
    Arc::new(|text| {
        let t = text.trim().to_lowercase();
        let (number, scale) = if let Some(rest) = t.strip_suffix("ms") {
            (rest, 0.001)
        } else if let Some(rest) = t.strip_suffix('s') {
            (rest, 1.0)
        } else {
            (t.as_str(), 0.001)
        };
        number.trim().parse::<f32>().ok().map(|v| v * scale)
    })
}

/// Show a `0..=1` control as a percentage — never a negative zero, which VCO-2 tune, centred on
/// unison, reached either side of it.
fn v2s_percent() -> ValueToString {
    Arc::new(|v| format!("{} %", without_negative_zero(format!("{:.0}", v * 100.0))))
}

fn s2v_percent() -> StringToValue {
    Arc::new(|text| {
        text.trim()
            .trim_end_matches('%')
            .trim()
            .parse::<f32>()
            .ok()
            .map(|v| v / 100.0)
    })
}

/// An amount, `0..=1`, smoothed, shown as a percentage.
fn amount(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_smoother(SmoothingStyle::Linear(10.0))
        .with_value_to_string(v2s_percent())
        .with_string_to_value(s2v_percent())
}

/// An envelope time: unsmoothed, formatted in ms or s, the hardware's range.
fn envelope_time(name: &str, default: f32, min: f32, max: f32) -> FloatParam {
    FloatParam::new(
        name,
        default,
        FloatRange::Skewed {
            min,
            max,
            factor: FloatRange::skew_factor(-2.0),
        },
    )
    .with_value_to_string(v2s_time())
    .with_string_to_value(s2v_time())
}

/// **The LFO rate's tempo sync** (`plans/plan-tempo-sync-controls.md`): every LFO's ladder, 1/32 to
/// four bars, the top the fastest.
pub const LFO_SYNC: mxm_tempo::Ladder =
    mxm_tempo::Ladder::new(mxm_tempo::Span::LFO, mxm_tempo::Direction::Rate);

#[derive(Params)]
pub struct MxmMono02Params {
    // ---- LFO ----
    #[id = "lforate"]
    pub lfo_rate: FloatParam,
    /// The LFO rate's tempo sync: its position picks a division of the host's tempo.
    #[id = "lfosync"]
    pub lfo_sync: BoolParam,
    /// The fade-in of the sine, and of nothing else. An amount: zero is transparent.
    #[id = "lfodelay"]
    pub lfo_delay: FloatParam,
    #[id = "lfomode"]
    pub lfo_mode: EnumParam<LfoKind>,

    // ---- VCO, shared ----
    /// The width both pulses start from: square at the top of its range, narrowest at the bottom.
    /// One width for both pulses; its routes move it.
    #[id = "pulsewidth"]
    pub pulse_width: FloatParam,
    /// TOTAL TUNE: both VCOs, in cents.
    #[id = "tune"]
    pub tune: FloatParam,

    // ---- VCO-1 ----
    #[id = "vco1range"]
    pub vco1_range: EnumParam<RangeKind>,
    #[id = "vco1wave"]
    pub vco1_wave: EnumParam<Wave1Kind>,
    /// The BENDER ON/OFF switch: off, and VCO-2 alone bends — the manual's route to a bendable
    /// interval.
    #[id = "vco1bender"]
    pub vco1_bender: BoolParam,

    // ---- VCO-2 ----
    #[id = "vco2range"]
    pub vco2_range: EnumParam<RangeKind>,
    #[id = "vco2wave"]
    pub vco2_wave: EnumParam<Wave2Kind>,
    /// The TUNE knob's position, `-1..=1`, with unison at the centre in either range.
    #[id = "vco2tune"]
    pub vco2_tune: FloatParam,
    /// NARROW or WIDE: ±200 or ±1750 cents. Kept as a switch because a 7-bit controller on the
    /// tune knob gets usable resolution only in the narrow range.
    #[id = "vco2tunerange"]
    pub vco2_tune_range: EnumParam<TuneRange>,

    // ---- Mixer ----
    #[id = "sub"]
    pub sub: FloatParam,
    #[id = "vco1"]
    pub vco1: FloatParam,
    #[id = "vco2"]
    pub vco2: FloatParam,

    // ---- VCF ----
    /// A **position**, not a frequency: the machine's slider over its range.
    #[id = "cutoff"]
    pub cutoff: FloatParam,
    #[id = "resonance"]
    pub resonance: FloatParam,

    // ---- VCA ----
    #[id = "vcamode"]
    pub vca_mode: EnumParam<VcaKind>,

    // ---- ENV ----
    #[id = "attack"]
    pub attack: FloatParam,
    #[id = "decay"]
    pub decay: FloatParam,
    #[id = "sustain"]
    pub sustain: FloatParam,
    #[id = "release"]
    pub release: FloatParam,
    #[id = "trigger"]
    pub trigger: EnumParam<TriggerKind>,

    // ---- Voice ----
    /// Portamento time. An **amount**: always in circuit, zero is the only off.
    #[id = "portamento"]
    pub portamento: FloatParam,
    #[id = "volume"]
    pub volume: FloatParam,

    // ================= Disclosed =================
    /// The bender's VCO sensitivity, in semitones. A configuration.
    #[id = "bendrange"]
    pub bend_range: FloatParam,

    /// Every modulation route: a presence and a signed amount per *(target, source)* pair. The
    /// machine's own eight paths are present at Init, at zero.
    #[nested(group = "Modulation")]
    pub routes: crate::routes::Routes,

    /// Which preset is loaded, and what it looked like when it was.
    ///
    /// **Persisted with the patch, not beside it.** nice-plug carries non-parameter state through
    /// the `Params` derive's `#[persist]`, so it belongs here rather than as a field on the plugin
    /// struct — and it has its own version number, because nice-plug's state version is
    /// `Plugin::VERSION`, which moves for unrelated reasons.
    #[persist = "preset"]
    pub preset: RwLock<mxm_preset::PresetIdentity>,
}

impl Default for MxmMono02Params {
    /// The init patch, which is also the set of CLAP `default_value`s.
    ///
    /// They are allowed to be equal and must not be two concepts: a host shows `default_value` as a
    /// control's detent and uses it for "reset this parameter", so a divergence means the Init
    /// button and the host disagree about the same sound.
    fn default() -> Self {
        Self {
            // ---- LFO: a vibrato rate, so it is vibrato the moment a depth is raised ----
            lfo_rate: FloatParam::new(
                "LFO rate",
                5.0,
                FloatRange::Skewed {
                    min: lfo::RATE_MIN_HZ,
                    max: lfo::RATE_MAX_HZ,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            lfo_sync: BoolParam::new("LFO sync", false),

            lfo_delay: FloatParam::new(
                "LFO delay",
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: lfo::DELAY_MAX_S,
                },
            )
            // Not smoothed: it sets a time constant, not a signal.
            .with_value_to_string(v2s_time())
            .with_string_to_value(s2v_time()),

            lfo_mode: EnumParam::new("LFO shape", LfoKind::Sine),

            // ---- VCO, shared ----
            pulse_width: FloatParam::new(
                "Pulse width",
                0.5,
                FloatRange::Linear {
                    min: 0.05,
                    max: 0.5,
                },
            )
            .with_smoother(SmoothingStyle::Linear(10.0))
            .with_value_to_string(v2s_percent())
            .with_string_to_value(s2v_percent()),
            tune: FloatParam::new(
                "Master tune",
                0.0,
                FloatRange::Linear {
                    min: -200.0,
                    max: 200.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" cents")
            .with_value_to_string(v2s_whole()),

            // ---- VCO-1 ----
            vco1_range: EnumParam::new("VCO-1 range", RangeKind::Ft8),
            vco1_wave: EnumParam::new("VCO-1 wave", Wave1Kind::Saw),
            vco1_bender: BoolParam::new("VCO-1 bender", true),

            // ---- VCO-2: off unison, so raising it beats at once ----
            vco2_range: EnumParam::new("VCO-2 range", RangeKind::Ft8),
            vco2_wave: EnumParam::new("VCO-2 wave", Wave2Kind::Saw),
            vco2_tune: FloatParam::new(
                "VCO-2 tune",
                0.035,
                FloatRange::Linear {
                    min: -1.0,
                    max: 1.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(v2s_percent())
            .with_string_to_value(s2v_percent()),
            vco2_tune_range: EnumParam::new("VCO-2 tune range", TuneRange::Narrow),

            // ---- Mixer: one plain source sounds ----
            sub: amount("Sub", 0.0),
            vco1: amount("VCO-1", 1.0),
            vco2: amount("VCO-2", 0.0),

            // ---- VCF: open, short of the top ----
            cutoff: amount("Cutoff", 0.9),
            resonance: amount("Resonance", 0.0),

            // ---- VCA ----
            vca_mode: EnumParam::new("VCA", VcaKind::Env),

            // ---- ENV: the hardware's ranges, `sh-2.md` §3.8 ----
            attack: envelope_time("Attack", 0.002, 0.001, 2.5),
            decay: envelope_time("Decay", 0.3, 0.002, 10.0),
            sustain: amount("Sustain", 1.0),
            release: envelope_time("Release", 0.1, 0.002, 10.0),
            trigger: EnumParam::new("Trigger", TriggerKind::GateTrig),

            // ---- Voice ----
            portamento: FloatParam::new(
                "Portamento",
                0.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 5.0,
                    factor: FloatRange::skew_factor(-1.5),
                },
            )
            // Not smoothed: a time constant, not a signal.
            .with_value_to_string(v2s_time())
            .with_string_to_value(s2v_time()),

            volume: FloatParam::new(
                "Volume",
                util::db_to_gain(-6.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(0.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 0.0),
                },
            )
            // Stored as linear gain, formatted as dB: `SmoothingStyle::Logarithmic` over a dB range
            // spanning zero is mathematically invalid and trips a debug assertion.
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            // ---- Disclosed ----
            bend_range: FloatParam::new(
                "Bend range",
                2.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 24.0,
                },
            )
            // Smoothed because it scales a held bend into the pitch: a range edit under a held bend
            // would otherwise be a pitch step (mxm-kit's `docs/code-review-notes.md` §2).
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_unit(" st")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            routes: crate::routes::Routes::new(),

            preset: RwLock::new(mxm_preset::PresetIdentity::none()),
        }
    }
}

impl MxmMono02Params {
    /// The LFO rate while its sync follows the host, or `None` for its free rate: the modulated
    /// position picks a division on [`LFO_SYNC`]. Resolved once a buffer by the plugin.
    pub fn synced_lfo_rate(&self, tempo: Option<f64>) -> Option<f32> {
        let rate = &self.lfo_rate;
        LFO_SYNC
            .resolve(
                self.lfo_sync.value(),
                tempo,
                rate.modulated_normalized_value(),
                f64::from(rate.preview_plain(0.0)),
                f64::from(rate.preview_plain(1.0)),
            )
            .map(|hz| hz as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The LFO sync picks a division and is inert without a tempo**
    /// (`plans/plan-tempo-sync-controls.md`): off, or with no tempo, the knob's own hertz stand; on
    /// at 120 bpm the ends are the ladder's ends that the rate's range can hold, the top the fastest.
    /// Four bars is 0.125 Hz there, under this LFO's 0.2 Hz floor, so the bottom is two bars.
    #[test]
    fn lfo_sync_picks_a_division_and_is_inert_without_a_tempo() {
        use nice_plug::params::InternalParamMut;
        fn set<P: InternalParamMut>(param: &P, normalized: f32) {
            unsafe {
                let _ = param._internal_set_normalized_value(normalized);
            }
        }
        let p = MxmMono02Params::default();
        set(&p.lfo_rate, 1.0);
        assert_eq!(p.synced_lfo_rate(Some(120.0)), None, "off is the free rate");
        set(&p.lfo_sync, 1.0);
        assert_eq!(p.synced_lfo_rate(None), None, "no tempo is the free rate");

        let top = p.synced_lfo_rate(Some(120.0)).expect("synced at a tempo");
        set(&p.lfo_rate, 0.0);
        let bottom = p.synced_lfo_rate(Some(120.0)).expect("synced at a tempo");
        let (lo, hi) = (
            f64::from(p.lfo_rate.preview_plain(0.0)),
            f64::from(p.lfo_rate.preview_plain(1.0)),
        );
        assert!(
            top > bottom,
            "the top of a rate is the fastest: {bottom} to {top}"
        );
        let reach = LFO_SYNC.reachable(120.0, lo, hi).divisions();
        let fastest = reach[0].hz(120.0) as f32;
        let slowest = reach[reach.len() - 1].hz(120.0) as f32;
        assert_eq!(reach[reach.len() - 1], mxm_tempo::Division::TwoBars);
        assert!((top - fastest).abs() < 1e-4, "{top} against {fastest}");
        assert!(
            (bottom - slowest).abs() < 1e-4,
            "{bottom} against {slowest}"
        );
    }

    /// **Every parameter reads the same after the host's own round trip**: printed with its unit,
    /// parsed, and printed again, it is the same text (mxm-kit's `docs/code-review-notes.md` §6).
    ///
    /// The host never hands a formatter a plain value. The CLAP wrapper's `value_to_text` and
    /// `text_to_value` carry a normalised value in `f64`, scaled by the step count, so a parsed number
    /// goes through the range's normalisation and back before it is printed again — and a formatter
    /// that picks its unit or its precision from the *raw* value flips branch when that trip lands a
    /// hair the other side of the switch: `0.9996 s` printed `1000 ms`, which parses to one second
    /// and prints `1.00 s`. `clap-validator`'s `param-conversions` fails only when its values land in
    /// that sliver, so one clean run proves nothing.
    ///
    /// So this walks the whole parameter map, as the wrapper converts, at the validator's grids, at
    /// plain values either side of every branch point this file's and the routes' formatters have — a
    /// second, zero where a range crosses it, 0 dB — and at every representable normalised value
    /// near each of those points.
    #[test]
    fn every_parameter_reads_the_same_after_the_hosts_round_trip() {
        let params = MxmMono02Params::default();
        let map = params.param_map();
        // `clap-validator` 0.4.1 spends 4000 conversions across the parameters, 5 to 100 each.
        let installed = 4000usize.div_ceil(map.len()).clamp(5, 100);

        let mut probes: Vec<f32> = vec![
            // Seconds, printed `{:.0} ms` below one second: the millisecond text reaches `1000` at
            // 0.9995 s.
            0.9994, 0.999_49, 0.9995, 0.999_51, 0.9996, 0.9999, 1.0, 1.000_01, 1.004, 1.005, 1.006,
        ];
        // Either side of zero, where a plain `{:.N}` prints a negative zero.
        probes.push(0.0);
        for decade in [1e-7, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 1e-1] {
            for multiple in [1.0, 4.0, 5.0, 6.0] {
                probes.extend([decade * multiple, -decade * multiple]);
            }
        }
        // Either side of 0 dB, for a gain shown in decibels.
        for db in [1e-4, 1e-3, 0.04, 0.05, 0.06] {
            probes.extend([util::db_to_gain(db), util::db_to_gain(-db)]);
        }
        // The branch points themselves, where every nearby normalised value is tried too.
        let edges = [0.0, 0.9995, 1.0];

        let mut failures = Vec::new();
        for (id, param, _) in &map {
            // SAFETY: `params` owns every parameter these pointers name and outlives the loop; this
            // is the access the wrapper makes.
            unsafe {
                let steps = param.step_count();
                let scale = steps.unwrap_or(1) as f64;
                let mut values: Vec<f64> = (0..=19)
                    .map(|i| scale * f64::from(i) / 19.0)
                    .chain((0..installed).map(|i| scale * i as f64 / (installed - 1) as f64))
                    .collect();
                if steps.is_none() {
                    let (low, high) = (param.preview_plain(0.0), param.preview_plain(1.0));
                    values.extend(
                        probes
                            .iter()
                            .map(|&plain| f64::from(param.preview_normalized(plain))),
                    );
                    for &edge in edges.iter().filter(|&&edge| low <= edge && edge <= high) {
                        let mut up = param.preview_normalized(edge);
                        let mut down = up;
                        for _ in 0..=64 {
                            values.extend([f64::from(up), f64::from(down)]);
                            up = up.next_up().min(1.0);
                            down = down.next_down().max(0.0);
                        }
                    }
                }
                // `ext_params_value_to_text` and `ext_params_text_to_value`, as the wrapper has them.
                let text_of = |value: f64| {
                    param.normalized_value_to_string(value as f32 / scale as f32, true)
                };
                for value in values {
                    let first = text_of(value);
                    let Some(back) = param.string_to_normalized_value(&first) else {
                        failures.push(format!("{id}: {first:?} does not parse"));
                        continue;
                    };
                    let second = text_of(f64::from(back) * scale);
                    if second != first {
                        failures.push(format!("{id}: {first:?} parses and reads {second:?}"));
                    }
                }
            }
        }
        failures.sort();
        failures.dedup();
        assert!(
            failures.is_empty(),
            "{} texts changed through the host's conversion:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
