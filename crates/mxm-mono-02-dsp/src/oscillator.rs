//! The SH-2's VCO, its sub-oscillator and its noise.
//!
//! Both VCOs are the same circuit (`research:instruments/sh-2.md` §4): an exponential
//! converter charging a 1000 pF polystyrene capacitor that a JFET resets — a
//! **sawtooth core** — with a second comparator making the pulse, and on VCO-1 only
//! a switched-inversion stage making a triangle that a pair of diodes then rounds
//! into the panel's "sine". Each VCO has a four-position **selector**, one waveform
//! at a time: VCO-1 offers sine, saw, square and pulse; VCO-2 offers noise in place
//! of the sine. The sub is a T flip-flop clocked from VCO-1's reset — always a
//! square one octave down, locked to VCO-1, and blind to the pulse width (§4.6).
//!
//! Band-limiting is PolyBLEP on the saw's reset, the pulse's two edges and the sub's
//! toggles, copied from `mxm-mono-01-dsp`; the triangle needs none and is taken from
//! the phase, copied from `mxm-mono-00-dsp` (which landed its VCO first, as the plan's
//! §5.2 says the second of the two should note). Three things are this machine's own:
//!
//! - **The saw's corner is a discharge, not an edge** (wart 18's waveform half). The
//!   JFET discharges the integrator through its on-resistance, so the reset takes
//!   real time; [`RESET_S`] is that time, **chosen** because the schematic does not
//!   give it, and the corner is rendered as two half-steps that far apart. For any
//!   plausible duration the spectral change sits at the very top of the band or
//!   above it; `the_saws_corner_is_a_discharge_and_the_test_says_where_it_lands`
//!   measures rather than assumes.
//! - **The sine is a diode-rounded triangle** (wart 15). [`SINE_KNEE`] is the rounding
//!   stage's knee, chosen; the residual harmonics are measured, not claimed pure.
//! - **The noise is one seeded source**, white, because its spectrum is unmeasured
//!   (`sh-2.md` §4.7, §11) and an unmarked shaping would be a guess. The modulator's
//!   sample-and-hold reads the same source, as the machine's does.
//!
//! **Phasors free-run.** They are never reset on note-on — a VCO does not do that
//! either — and at idle they freeze, which is the voice's rule (plan §5.6).
//!
//! **The waveforms' relative polarity and phase are inherited and chosen, not the
//! machine's** (plan §5.2): `sh-2.md` §11 leaves the saw's direction and the fixed
//! square's source open; the ramp here rises, the pulse starts high, the sub's edge
//! sits on the reset and the sine's seam where the reset was. Presets and the golden
//! score will make these durable; they are labelled here before they do.

use crate::{Rng, flush};

/// Lowest frequency the oscillator will produce. The 32' range at the bottom of the
/// keyboard with the bender down reaches well under 20 Hz; below this the PolyBLEP
/// correction is meaningless.
pub const FREQ_MIN_HZ: f32 = 4.0;

/// Highest frequency, as a fraction of the sample rate. The 2' range with bend and
/// modulation on top can otherwise exceed Nyquist, and PolyBLEP does not make an
/// invalid phase increment safe.
pub const NYQUIST_FRACTION: f32 = 0.45;

/// How long the JFET takes to discharge the integrator at each reset, in seconds.
///
/// **Chosen, not measured.** `sh-2.md` §4.2 establishes that the reset takes real
/// time and that the corner is a short discharge rather than a vertical edge; §11
/// lists the duration as unverified. A 2SK30A discharging 1000 pF through its
/// on-resistance is a matter of a microsecond or two, and two is taken. The spectral
/// consequence — a sinc envelope on the harmonics with its first null at `1/RESET_S`
/// — is measured by the test named in the module doc; whether it is audible is not
/// claimed either way.
pub const RESET_S: f32 = 2.0e-6;

/// The knee of the diode pair that rounds VCO-1's triangle into the "sine", in the
/// triangle's own units (its peak is 1).
///
/// **Chosen, not measured.** `sh-2.md` §4.5: 220 kΩ and 82 kΩ scale the triangle
/// onto back-to-back diodes to ground, and the second half of IC19 brings it back up;
/// the residual harmonics were not measured, and the manual's "absolutely pure"
/// overstates a mechanism that leaves a few percent of third harmonic. This knee
/// gives that — `the_sine_is_a_rounded_triangle_with_a_few_percent_of_third_harmonic`
/// measures it — and the two bench measurements that would replace it are the
/// shaper's input level against the diodes' forward voltage.
pub const SINE_KNEE: f32 = 0.9;

/// PolyBLEP residual for a step discontinuity.
///
/// `t` is the phase in `0..1` and `dt` the per-sample phase increment. The residual
/// corrects the two samples either side of a discontinuity, which removes most of
/// the aliasing energy for the cost of a few operations.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// A free-running phase accumulator.
#[derive(Debug, Clone, Copy, Default)]
pub struct Phasor {
    phase: f32,
    inc: f32,
}

impl Phasor {
    pub const fn new() -> Self {
        Self {
            phase: 0.0,
            inc: 0.0,
        }
    }

    /// Set the increment from a frequency, clamping so the increment stays valid.
    #[inline]
    pub fn set_freq(&mut self, hz: f32, sample_rate: f32) {
        let hz = hz.clamp(FREQ_MIN_HZ, NYQUIST_FRACTION * sample_rate);
        self.inc = hz / sample_rate;
    }

    #[inline]
    pub fn set_inc(&mut self, inc: f32) {
        self.inc = inc.clamp(0.0, NYQUIST_FRACTION);
    }

    #[inline]
    pub fn phase(&self) -> f32 {
        self.phase
    }

    #[inline]
    pub fn inc(&self) -> f32 {
        self.inc
    }

    /// Advance, and say whether the phase wrapped — the reset the sub is clocked from.
    #[inline]
    pub fn advance(&mut self) -> bool {
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            true
        } else {
            false
        }
    }

    /// Zero the phase. Called from `reset()`, never from note-on.
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }
}

/// Band-limited rising sawtooth whose reset is a discharge of [`RESET_S`].
///
/// The −2 step at the wrap is rendered as two −1 steps `delta` apart in phase — the
/// discharge's duration at this increment — each with its own half residual. At every
/// plausible duration `delta` is a fraction of a sample, and the pair's spectrum is
/// the step's times `cos(π f RESET_S)`: a gentle roll-off whose first 0.1 dB the test
/// locates.
#[inline]
pub fn saw(p: &Phasor, sample_rate: f32) -> f32 {
    let (t, dt) = (p.phase(), p.inc());
    let delta = RESET_S * sample_rate * dt;
    // The naive waveform: two half-steps at the reset instead of one full step — the
    // ramp sits half-way for the discharge's duration — each corrected by half a
    // residual. A staircase, not a slope: PolyBLEP corrects steps, and a sample
    // landing inside the discharge (rare, at a fraction of a sample) reads the
    // mid level the residuals are computed against.
    let naive = if t < delta { 2.0 * t } else { 2.0 * t - 1.0 };
    let second = {
        let x = t - delta;
        if x < 0.0 { x + 1.0 } else { x }
    };
    naive - 0.5 * poly_blep(t, dt) - 0.5 * poly_blep(second, dt)
}

/// Band-limited pulse of the given width, both edges corrected. The machine's
/// square is this at 50 %.
#[inline]
pub fn pulse(p: &Phasor, width: f32) -> f32 {
    let (t, dt) = (p.phase(), p.inc());
    let w = clamp_pulse_width(width, dt);
    let mut y = if t < w { 1.0 } else { -1.0 };
    y += poly_blep(t, dt);
    let second = {
        let x = t - w;
        if x < 0.0 { x + 1.0 } else { x }
    };
    y -= poly_blep(second, dt);
    y
}

/// Keep the two PolyBLEP corrections from overlapping.
///
/// Each correction spans `dt` either side of an edge, so if the pulse is narrower
/// than `2*dt` the two would overlap and produce nonsense. At extreme frequencies
/// where no valid width exists, fall back to a square. The one-sidedness of the
/// SH-2's PWM — narrowing from 50 %, never past it — is the PWM section's rule
/// (the voice's), not this function's: the oscillator renders whatever width it is
/// given, and the width it is given never exceeds a half.
#[inline]
pub fn clamp_pulse_width(width: f32, dt: f32) -> f32 {
    let lo = 0.05f32.max(2.0 * dt);
    let hi = 0.95f32.min(1.0 - 2.0 * dt);
    if lo > hi { 0.5 } else { width.clamp(lo, hi) }
}

/// The machine's triangle: the saw through switched inversion, taken from the phase.
///
/// Copied from `mxm-mono-00-dsp`, which landed the same circuit first: continuous in
/// value and discontinuous only in slope, so it needs no PolyBLEP — its harmonics
/// fall at `1/n²` and the naive form aliases quietly. Built from the raw phase
/// rather than by folding the band-limited saw, because the PolyBLEP residual takes
/// the saw *through zero* at every reset and folding that produces a full-scale
/// spike per period. Its seam is where the reset was.
#[inline]
pub fn triangle(p: &Phasor) -> f32 {
    4.0 * (p.phase() - 0.5).abs() - 1.0
}

/// Back-to-back diodes to ground as a curve, `f(x) = x / (1 + |x/knee|³)^(1/3)`:
/// the same shape the filter's resonance clamp uses, bounded by the knee, monotonic
/// and odd — so it adds odd harmonics only, no even ones and no DC.
#[inline]
pub(crate) fn diode_round(x: f32, knee: f32) -> f32 {
    let a = x / knee;
    let a3 = (a * a * a).abs();
    x / (1.0 + a3).cbrt()
}

/// VCO-1's "sine": the triangle through the diode rounding stage, brought back to a
/// peak of 1 as the second half of IC19 does.
#[inline]
pub fn sine(p: &Phasor) -> f32 {
    diode_round(triangle(p), SINE_KNEE) / diode_round(1.0, SINE_KNEE)
}

/// VCO-1's selector (`sh-2.md` §3.3). One waveform at a time; the sine is only here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wave1 {
    Sine,
    #[default]
    Saw,
    Square,
    Pulse,
}

/// VCO-2's selector (§3.4). Noise replaces the oscillator; there is no sine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wave2 {
    Noise,
    #[default]
    Saw,
    Square,
    Pulse,
}

/// The sawtooth core, shared by both VCOs.
#[derive(Debug, Clone, Default)]
pub struct Vco {
    phasor: Phasor,
    wrapped: bool,
}

impl Vco {
    pub const fn new() -> Self {
        Self {
            phasor: Phasor::new(),
            wrapped: false,
        }
    }

    pub fn reset(&mut self) {
        self.phasor.reset();
        self.wrapped = false;
    }

    /// The core's phase, for the sub and for tests.
    #[inline]
    pub fn phasor(&self) -> &Phasor {
        &self.phasor
    }

    /// Whether the core reset on the last sample — the clock the sub divides.
    #[inline]
    pub fn just_reset(&self) -> bool {
        self.wrapped
    }

    /// Tune the core. Separate from rendering so both VCOs can be tuned before the
    /// sub reads VCO-1's increment.
    #[inline]
    pub fn set_freq(&mut self, freq_hz: f32, sample_rate: f32) {
        self.phasor.set_freq(freq_hz, sample_rate);
    }

    /// Render one sample of VCO-1's selected waveform and advance.
    #[inline]
    pub fn process_1(&mut self, wave: Wave1, width: f32, sample_rate: f32) -> f32 {
        let y = match wave {
            Wave1::Sine => sine(&self.phasor),
            Wave1::Saw => saw(&self.phasor, sample_rate),
            Wave1::Square => pulse(&self.phasor, 0.5),
            Wave1::Pulse => pulse(&self.phasor, width),
        };
        self.wrapped = self.phasor.advance();
        flush(y)
    }

    /// Render one sample of VCO-2's selected waveform and advance. `noise` is the
    /// instrument's one noise source, read here when the selector says so.
    #[inline]
    pub fn process_2(&mut self, wave: Wave2, width: f32, noise: f32, sample_rate: f32) -> f32 {
        let y = match wave {
            Wave2::Noise => noise,
            Wave2::Saw => saw(&self.phasor, sample_rate),
            Wave2::Square => pulse(&self.phasor, 0.5),
            Wave2::Pulse => pulse(&self.phasor, width),
        };
        self.wrapped = self.phasor.advance();
        flush(y)
    }
}

/// The sub-oscillator: a T flip-flop on VCO-1's reset (`sh-2.md` §4.6), always a
/// square one octave down, blind to the pulse width. The toggle is band-limited by
/// the same residual as a pulse edge — `mxm-poly-06-dsp`'s divider, which found
/// that a flip-flop without one aliases as badly as a naive square.
#[derive(Debug, Clone)]
pub struct Sub {
    state: f32,
}

impl Default for Sub {
    fn default() -> Self {
        Self::new()
    }
}

impl Sub {
    pub const fn new() -> Self {
        Self { state: -1.0 }
    }

    pub fn reset(&mut self) {
        self.state = -1.0;
    }

    /// The sub's level right now. Call **before** `clock`, with the VCO-1 the sub is
    /// divided from, so the residual is placed against the phase this sample sees.
    #[inline]
    pub fn process(&self, vco1: &Vco) -> f32 {
        let (t, dt) = (vco1.phasor.phase(), vco1.phasor.inc());
        let s = self.state;
        // Just after a toggle the state is the new one and the residual pulls the
        // sample back towards the old; just before, the reverse.
        flush(s + if t < dt { s } else { -s } * poly_blep(t, dt))
    }

    /// Divide: toggle if VCO-1 just reset. Call **after** VCO-1 has advanced.
    #[inline]
    pub fn clock(&mut self, vco1: &Vco) {
        if vco1.just_reset() {
            self.state = -self.state;
        }
    }
}

/// The instrument's one noise source: a 2SC828 in reverse breakdown, amplified
/// (`sh-2.md` §4.7). Serves VCO-2's NOISE position and the modulator's sample-and-hold
/// alike. White, seeded, deterministic — the shaping network's effect is unmeasured
/// and is not invented.
#[derive(Debug, Clone)]
pub struct Noise {
    rng: Rng,
}

impl Default for Noise {
    fn default() -> Self {
        Self::new()
    }
}

/// Fixed seed, restored on `reset()`, so a rendered take is bit-identical when
/// replayed — what makes a golden score possible.
const NOISE_SEED: u32 = 0x0517_0202;

impl Noise {
    pub const fn new() -> Self {
        Self {
            rng: Rng::new(NOISE_SEED),
        }
    }

    pub fn reset(&mut self) {
        self.rng = Rng::new(NOISE_SEED);
    }

    #[inline]
    pub fn sample(&mut self) -> f32 {
        self.rng.next_bipolar()
    }
}

/// One-pole DC blocker: an AC-coupling stage, at the corner the caller derives.
///
/// A pulse wave is not symmetric unless its width is exactly 50%, so pulse-width
/// modulation swings the DC offset around, and this does the job the hardware's
/// coupling capacitor does. `mxm-mono-01-dsp`'s stage with the corner moved out of the
/// type: mono-01 chose 15 Hz for its own keyboard, and this machine's corner is a
/// capacitor on the schematic (`voice::VCA_COUPLING_HZ`), so a shared stage can carry
/// neither number.
#[derive(Debug, Clone, Copy, Default)]
pub struct DcBlocker {
    x1: f32,
    y1: f32,
    r: f32,
}

impl DcBlocker {
    pub const fn new() -> Self {
        Self {
            x1: 0.0,
            y1: 0.0,
            r: 0.0,
        }
    }

    /// The corner and the rate together. The coefficient is the one-pole's pole,
    /// `1 - 2π f / fs`, which puts the −3 dB point within a fraction of a percent of
    /// `cutoff_hz` at any audio rate.
    pub fn set_corner(&mut self, cutoff_hz: f32, sample_rate: f32) {
        self.r = 1.0 - (std::f32::consts::TAU * cutoff_hz / sample_rate);
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = flush(y);
        self.y1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATES: [f32; 4] = [44_100.0, 48_000.0, 96_000.0, 192_000.0];

    /// Frequency estimated from positive-going zero crossings of VCO-1's output.
    fn measure_freq(wave: Wave1, freq: f32, fs: f32, secs: f32) -> f32 {
        let mut v = Vco::new();
        let n = (fs * secs) as usize;
        // Period from the span between the first and last upward crossing, so the
        // window's edges cannot add a partial period: a waveform that starts at a
        // rising edge would otherwise count one crossing too many.
        let (mut first, mut last, mut crossings, mut prev) = (None, 0usize, 0usize, f32::NAN);
        for i in 0..n {
            v.set_freq(freq, fs);
            let y = v.process_1(wave, 0.3, fs);
            if prev <= 0.0 && y > 0.0 {
                if first.is_none() {
                    first = Some(i);
                }
                last = i;
                crossings += 1;
            }
            prev = y;
        }
        let first = first.expect("no crossings");
        (crossings - 1) as f32 * fs / (last - first) as f32
    }

    #[test]
    fn every_waveform_is_in_tune_to_a_cent() {
        for fs in RATES {
            for wave in [Wave1::Sine, Wave1::Saw, Wave1::Square, Wave1::Pulse] {
                for freq in [55.0f32, 440.0, 4_000.0] {
                    let measured = measure_freq(wave, freq, fs, 2.0);
                    let cents = 1200.0 * (measured / freq).log2();
                    assert!(
                        cents.abs() < 1.0,
                        "{wave:?} {freq} Hz at {fs}: measured {measured}, off by {cents:.3} cents"
                    );
                }
            }
        }
    }

    /// Harmonic amplitudes of an exactly periodic buffer, by direct DFT. See
    /// `mxm-mono-01-dsp`'s alias test for why an exactly periodic window needs no
    /// window function and separates aliases from harmonics without leakage.
    fn spectrum(x: &[f64], periods: usize) -> (Vec<f64>, f64) {
        let n = x.len();
        let half = n / 2;
        let mut harmonics = Vec::new();
        let mut alias = 0.0f64;
        for bin in 1..half {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &v) in x.iter().enumerate() {
                let ang = -2.0 * std::f64::consts::PI * bin as f64 * i as f64 / n as f64;
                re += v * ang.cos();
                im += v * ang.sin();
            }
            let power = re * re + im * im;
            if bin % periods == 0 {
                harmonics.push(power.sqrt());
            } else {
                alias += power;
            }
        }
        (harmonics, alias)
    }

    fn render<F: FnMut(&Phasor) -> f32>(n: usize, inc: f32, mut f: F) -> Vec<f64> {
        let mut p = Phasor::new();
        p.set_inc(inc);
        (0..n)
            .map(|_| {
                let y = f(&p);
                p.advance();
                y as f64
            })
            .collect()
    }

    fn alias_to_signal_db(x: &[f64], periods: usize) -> f64 {
        let (h, alias) = spectrum(x, periods);
        let wanted: f64 = h.iter().map(|a| a * a).sum();
        10.0 * (alias / wanted.max(1e-30)).max(1e-30).log10()
    }

    #[test]
    fn sawtooth_and_pulse_aliasing_stay_far_below_the_trivial_waveform() {
        const N: usize = 2048;
        const PERIODS: usize = 21;
        let fs = 44_100.0f32;
        let inc = PERIODS as f32 / N as f32;

        let mut phase = 0.0f32;
        let trivial: Vec<f64> = (0..N)
            .map(|_| {
                let y = 2.0 * phase - 1.0;
                phase += inc;
                if phase >= 1.0 {
                    phase -= 1.0;
                }
                y as f64
            })
            .collect();
        let trivial_db = alias_to_signal_db(&trivial, PERIODS);
        let shipped_db = alias_to_signal_db(&render(N, inc, |p| saw(p, fs)), PERIODS);
        assert!(shipped_db < -30.0, "sawtooth aliasing {shipped_db:.1} dB");
        assert!(
            shipped_db < trivial_db - 10.0,
            "band limiting bought only {:.1} dB",
            trivial_db - shipped_db
        );
        for width in [0.05f32, 0.25, 0.5] {
            let db = alias_to_signal_db(&render(N, inc, |p| pulse(p, width)), PERIODS);
            assert!(db < -25.0, "pulse width {width}: aliasing {db:.1} dB");
        }
    }

    /// Wart 18's waveform half, measured: the discharge corner rolls the top of the
    /// spectrum off along a sinc, and the test says where that starts. Replace the
    /// discharge with a step (`RESET_S` = 0) and the last assertion goes red.
    #[test]
    fn the_saws_corner_is_a_discharge_and_the_test_says_where_it_lands() {
        const N: usize = 4096;
        const PERIODS: usize = 21;
        let fs = 192_000.0f32; // where the top harmonic is far above the audio band
        let inc = PERIODS as f32 / N as f32;
        let f0 = inc * fs;

        let (discharge, _) = spectrum(&render(N, inc, |p| saw(p, fs)), PERIODS);
        // The same generator with an instantaneous reset: a plain PolyBLEP saw.
        let (step, _) = spectrum(
            &render(N, inc, |p| {
                2.0 * p.phase() - 1.0 - poly_blep(p.phase(), p.inc())
            }),
            PERIODS,
        );
        let mut first_off = None;
        for (k, (d, s)) in discharge.iter().zip(&step).enumerate() {
            let harmonic = k + 1;
            let ratio_db = 20.0 * (d / s).log10();
            let hz = harmonic as f32 * f0;
            if hz < 20_000.0 {
                assert!(
                    ratio_db.abs() < 0.1,
                    "harmonic {harmonic} at {hz:.0} Hz differs by {ratio_db:.3} dB, inside the audio band"
                );
            }
            if first_off.is_none() && ratio_db < -0.1 {
                first_off = Some(hz);
            }
        }
        // The derived expectation for two half-steps RESET_S apart: their sum carries
        // cos(pi f RESET_S), which reaches -0.1 dB near 0.048/RESET_S — 24 kHz at the
        // chosen two microseconds.
        let expected = 0.048 / RESET_S;
        let first_off = first_off.expect("the discharge must roll the top off somewhere");
        println!(
            "discharge corner: first harmonic more than 0.1 dB down at {first_off:.0} Hz (derived {expected:.0} Hz)"
        );
        assert!(
            first_off > 20_000.0,
            "the corner reaches into the audio band: {first_off:.0} Hz"
        );
        let last = discharge.len() - 1;
        let top_db = 20.0 * (discharge[last] / step[last]).log10();
        assert!(
            top_db < -0.2,
            "the top harmonic must show the discharge: {top_db:.3} dB against a step"
        );
    }

    #[test]
    fn the_sine_is_a_rounded_triangle_with_a_few_percent_of_third_harmonic() {
        const N: usize = 2048;
        const PERIODS: usize = 21;
        let inc = PERIODS as f32 / N as f32;
        let (h, _) = spectrum(&render(N, inc, sine), PERIODS);
        let third = h[2] / h[0];
        let second = h[1] / h[0];
        println!(
            "sine: 3rd harmonic {:.2} %, 5th {:.2} %",
            third * 100.0,
            h[4] / h[0] * 100.0
        );
        assert!(
            second < 1e-3,
            "an odd shaper adds no even harmonics: 2nd at {second}"
        );
        assert!(
            (0.01..0.10).contains(&third),
            "a diode-rounded triangle carries a few percent of third harmonic, measured {:.2} %",
            third * 100.0
        );
        // And it is not merely the triangle: a bare triangle's third harmonic is 1/9.
        let (t, _) = spectrum(&render(N, inc, triangle), PERIODS);
        assert!(
            third < t[2] / t[0] - 0.005,
            "the rounding must reduce the triangle's third"
        );
        let peak = render(N, inc, sine)
            .iter()
            .fold(0.0f64, |m, v| m.max(v.abs()));
        assert!(
            (peak - 1.0).abs() < 1e-3,
            "the sine is brought back to a peak of 1: {peak}"
        );
    }

    #[test]
    fn the_sub_is_one_octave_below_locked_to_vco1_and_blind_to_the_pulse_width() {
        let fs = 48_000.0;
        let mut v = Vco::new();
        let mut sub = Sub::new();
        let secs = 2.0;
        let n = (fs * secs) as usize;
        let mut render_sub = |width: f32| {
            v.reset();
            sub.reset();
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                // The pitch moves: the sub must stay exactly an octave down throughout.
                let freq = 220.0 * (1.0 + (i as f32 / n as f32) * 0.5);
                v.set_freq(freq, fs);
                let s = sub.process(&v);
                let vco = v.process_1(Wave1::Pulse, width, fs);
                sub.clock(&v);
                out.push((s, vco));
            }
            out
        };
        let wide = render_sub(0.5);
        let narrow = render_sub(0.1);
        // Blind to PWM: the sub's samples are bit-identical at any width.
        assert!(
            wide.iter().zip(&narrow).all(|(a, b)| a.0 == b.0),
            "the sub must not change with the pulse width"
        );
        // Locked: the sub toggles exactly when VCO-1 resets, so its crossings are
        // half the ramp's over the whole sweep.
        let mut v2 = Vco::new();
        let mut sub2 = Sub::new();
        let (mut sub_edges, mut resets) = (0usize, 0usize);
        for i in 0..n {
            let freq = 220.0 * (1.0 + (i as f32 / n as f32) * 0.5);
            v2.set_freq(freq, fs);
            let before = sub2.state;
            v2.process_1(Wave1::Saw, 0.5, fs);
            sub2.clock(&v2);
            if v2.just_reset() {
                resets += 1;
                assert_ne!(sub2.state, before, "the sub toggles at the reset");
            } else {
                assert_eq!(sub2.state, before, "and only at the reset");
            }
            if sub2.state > before {
                sub_edges += 1;
            }
        }
        assert_eq!(
            sub_edges * 2,
            resets + (resets % 2),
            "one sub cycle per two resets"
        );
    }

    #[test]
    fn the_subs_toggle_is_band_limited() {
        const N: usize = 4096;
        const PERIODS: usize = 21; // ramp periods; the sub completes half as many
        let inc = 2.0 * PERIODS as f32 / N as f32;
        let mut v = Vco::new();
        let mut sub = Sub::new();
        v.phasor.set_inc(inc);
        let x: Vec<f64> = (0..N)
            .map(|_| {
                let s = sub.process(&v);
                v.process_1(Wave1::Saw, 0.5, 48_000.0);
                sub.clock(&v);
                s as f64
            })
            .collect();
        let db = alias_to_signal_db(&x, PERIODS);
        assert!(
            db < -25.0,
            "sub: aliasing {db:.1} dB, expected below -25 dB"
        );
    }

    #[test]
    fn output_is_bounded_and_finite_at_extreme_pitch() {
        for fs in RATES {
            let mut v = Vco::new();
            let mut sub = Sub::new();
            let mut noise = Noise::new();
            for freq in [0.0f32, 1.0, 20_000.0, 40_000.0, 1e9, -100.0] {
                for width in [0.0f32, 0.05, 0.5, 1.0] {
                    for _ in 0..2_000 {
                        v.set_freq(freq, fs);
                        let s = sub.process(&v);
                        let a = v.process_1(Wave1::Saw, width, fs);
                        sub.clock(&v);
                        let b = v.process_2(Wave2::Pulse, width, noise.sample(), fs);
                        let c = v.process_1(Wave1::Sine, width, fs);
                        for y in [s, a, b, c] {
                            assert!(
                                y.is_finite(),
                                "non-finite at {freq} Hz, width {width}, {fs} Hz"
                            );
                            assert!(y.abs() <= 1.5, "unbounded: {y}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn noise_is_deterministic_and_shared() {
        let mut a = Noise::new();
        let mut b = Noise::new();
        for _ in 0..1_000 {
            assert_eq!(a.sample(), b.sample());
        }
        a.reset();
        let mut c = Noise::new();
        assert_eq!(a.sample(), c.sample(), "reset restores the seed");
    }

    #[test]
    fn changing_frequency_never_resets_the_phase() {
        let fs = 48_000.0;
        let mut p = Phasor::new();
        p.set_freq(220.0, fs);
        for _ in 0..100 {
            p.advance();
        }
        let before = p.phase();
        p.set_freq(880.0, fs);
        assert_eq!(p.phase(), before, "set_freq moved the phase");
    }
}
