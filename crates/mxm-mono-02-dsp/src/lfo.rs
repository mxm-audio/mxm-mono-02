//! The modulator: one LFO with three shapes and a delay that fades in the sine only.
//!
//! `research:instruments/sh-2.md` §6: IC24 is a textbook triangle relaxation oscillator whose
//! rate and amplitude are independent. The triangle goes through the same back-to-back
//! diode shaper VCO-1's sine uses (§6.2), so the panel's ∿ is a rounded triangle here
//! too; the comparator's square drives the LED, the sample-and-hold clock and the
//! envelope's LFO trigger mode; RANDOM is the noise generator sampled on the square's
//! edge onto a capacitor — stepped, not slewed, at the RATE slider's rate, with no
//! separate clock (§6.4).
//!
//! **The delay is a fade-in on the sine, and nothing else** (§6.3, wart 9). A JFET
//! attenuator on the sine path alone follows the charge on C66, which the DELAY slider
//! charges and a diode dumps on every trigger — so the fade is an RC charging curve,
//! not a linear ramp and not a wait-then-switch; the square and the random are never
//! delayed; and the PWM section takes its own tap of the sine **before** the
//! attenuator, regardless of the MODE switch (§3.1). All four signals leave this
//! module every sample and the voice picks what the MODE switch selects.
//!
//! **Free-running.** The circuit has no phase reset, only the delay's dump diode, so
//! a trigger restarts the fade and never the waveform. At idle the phase freezes and
//! the fade settles to its target, which is the voice's rule (plan §5.6).
//!
//! What is chosen rather than measured, each labelled at its constant: how the DELAY
//! control's seconds map to the RC's time constant, the shape of the S&H's held values,
//! and the sine's rounding knee, which is the VCO's.

use crate::oscillator::{SINE_KNEE, diode_round};

/// The RATE slider's span (`sh-2.md` §3.1). The 270 Ω in series with the slider is why
/// it never reaches zero.
pub const RATE_MIN_HZ: f32 = 0.2;
pub const RATE_MAX_HZ: f32 = 25.0;

/// The DELAY TIME slider's top (§3.1).
pub const DELAY_MAX_S: f32 = 1.5;

/// How the DELAY control's seconds become the RC's time constant: the control reads
/// the time at which the fade has all but arrived, so the time constant is a third of
/// it (`1 − e⁻³` ≈ 95 %). **Chosen** — the specification gives the control's span, not
/// what the number on it means, and the RC (C66 through the slider and 150 kΩ) sets
/// only the shape. One divisor to change against a measurement.
pub const DELAY_TAUS: f32 = 3.0;

/// The MODE switch (§3.1): what the two MOD sliders receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Sine,
    Square,
    Random,
}

/// Everything the modulator puts out in one sample.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Outputs {
    /// The rounded triangle through the delay's attenuator: what MODE ∿ delivers.
    pub sine: f32,
    /// The comparator's square, never delayed.
    pub square: f32,
    /// The sample-and-hold of the noise, never delayed.
    pub random: f32,
    /// The rounded triangle **before** the attenuator: the PWM section's own tap.
    pub pwm_sine: f32,
    /// Whether the square rose this sample — the envelope's LFO trigger.
    pub square_rose: bool,
}

impl Outputs {
    /// What the MODE switch selects, for the two MOD sliders.
    #[inline]
    pub fn selected(&self, mode: Mode) -> f32 {
        match mode {
            Mode::Sine => self.sine,
            Mode::Square => self.square,
            Mode::Random => self.random,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Modulator {
    phase: f32,
    /// The charge on the delay capacitor, `0..=1`: the sine path's gain.
    fade: f32,
    /// The value the S&H holds.
    held: f32,
    /// The square's level last sample, to find its rising edge.
    square_was_high: bool,
}

impl Default for Modulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Modulator {
    pub const fn new() -> Self {
        Self {
            phase: 0.0,
            fade: 1.0,
            held: 0.0,
            square_was_high: true,
        }
    }

    /// Zero the phase, the hold and the fade. `reset()` only — the modulator free-runs
    /// across notes.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// The trigger: the dump diode empties the delay capacitor. The phase is untouched.
    pub fn trigger(&mut self) {
        self.fade = 0.0;
    }

    /// The delay capacitor's charge, for tests and telemetry.
    pub fn fade(&self) -> f32 {
        self.fade
    }

    /// Idle: the fade settles to its target (plan §5.6 — state with a target settles,
    /// state without one freezes). The phase and the hold stay where they are.
    pub fn settle(&mut self) {
        self.fade = 1.0;
    }

    /// Advance one sample. `noise` is the instrument's one noise source, which the
    /// S&H samples on the square's rising edge.
    #[inline]
    pub fn process(&mut self, rate_hz: f32, delay_s: f32, noise: f32, sample_rate: f32) -> Outputs {
        let p = self.phase;
        let triangle = if p < 0.25 {
            4.0 * p
        } else if p < 0.75 {
            2.0 - 4.0 * p
        } else {
            4.0 * p - 4.0
        };
        let rounded = diode_round(triangle, SINE_KNEE) / diode_round(1.0, SINE_KNEE);

        // The comparator: high while the triangle rises.
        let square_high = p < 0.5;
        let square_rose = square_high && !self.square_was_high;
        self.square_was_high = square_high;
        if square_rose {
            self.held = noise;
        }

        self.phase += rate_hz.clamp(RATE_MIN_HZ, RATE_MAX_HZ) / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // The delay: an RC charge towards full gain, dumped by the trigger. A zero
        // delay is transparent — the control at its bottom must not soften anything.
        let delay_s = delay_s.clamp(0.0, DELAY_MAX_S);
        if delay_s <= 0.0 {
            self.fade = 1.0;
        } else if self.fade < 1.0 {
            let tau = delay_s / DELAY_TAUS;
            let coef = (-1.0 / (tau * sample_rate)).exp();
            self.fade = 1.0 + (self.fade - 1.0) * coef;
            // Snap the last hundredth of a decibel: the exponential would otherwise
            // never arrive, and an arrived fade is what lets idle be exact.
            if self.fade > 0.999 {
                self.fade = 1.0;
            }
        }

        Outputs {
            sine: rounded * self.fade,
            square: if square_high { 1.0 } else { -1.0 },
            random: self.held,
            pwm_sine: rounded,
            square_rose,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn run(m: &mut Modulator, secs: f32, rate: f32, delay: f32) -> Vec<Outputs> {
        let mut noise = crate::oscillator::Noise::new();
        (0..(FS * secs) as usize)
            .map(|_| m.process(rate, delay, noise.sample(), FS))
            .collect()
    }

    #[test]
    fn the_rate_is_what_was_asked_for_and_the_square_rises_once_per_cycle() {
        let mut m = Modulator::new();
        let out = run(&mut m, 10.0, 3.0, 0.0);
        let rises = out.iter().filter(|o| o.square_rose).count();
        assert_eq!(rises, 30, "3 Hz over 10 s");
        // And the rate is clamped to the slider's span.
        let mut m = Modulator::new();
        let fast = run(&mut m, 2.0, 1_000.0, 0.0);
        let rises = fast.iter().filter(|o| o.square_rose).count();
        let expected = (RATE_MAX_HZ * 2.0) as usize;
        // Two seconds is a whole number of cycles, so the last wrap may fall on either
        // side of the window's end.
        assert!(
            (expected - 1..=expected).contains(&rises),
            "{rises} rises at the top rate"
        );
    }

    #[test]
    fn the_sine_is_the_rounded_triangle_and_stays_bipolar() {
        let mut m = Modulator::new();
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for o in run(&mut m, 1.0, 2.0, 0.0) {
            assert!((-1.0..=1.0).contains(&o.sine));
            lo = lo.min(o.sine);
            hi = hi.max(o.sine);
        }
        assert!(lo < -0.99 && hi > 0.99, "range {lo}..{hi}");
    }

    #[test]
    fn random_is_a_stepped_hold_of_the_noise_clocked_by_the_square() {
        let mut m = Modulator::new();
        let out = run(&mut m, 5.0, 4.0, 0.0);
        let mut changes = 0usize;
        for w in out.windows(2) {
            if w[1].random != w[0].random {
                changes += 1;
                assert!(
                    w[1].square_rose,
                    "the hold changed off the square's rising edge"
                );
            }
        }
        // Twenty rises in five seconds at 4 Hz, each a new value; five seconds is a
        // whole number of cycles, so the last rise may fall just outside the window.
        assert!(
            (19..=20).contains(&changes),
            "one new value per cycle, held in between: {changes}"
        );
        let distinct = out
            .iter()
            .map(|o| o.random.to_bits())
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        assert!(
            distinct >= 20,
            "the held values are the noise's, not a pattern"
        );
    }

    #[test]
    fn the_delay_fades_in_the_sine_only_on_an_rc_curve() {
        let delay = 0.6;
        let mut m = Modulator::new();
        m.trigger();
        let out = run(&mut m, 2.0, 5.0, delay);
        // The square and the random are at full level from the first sample.
        assert!(out[0].square.abs() == 1.0);
        assert!(out.iter().all(|o| o.square.abs() == 1.0));
        // The PWM tap is un-delayed: it equals the undelayed rounded triangle, which the
        // delayed sine only reaches once the fade has arrived.
        assert!(out[1].sine.abs() < out[1].pwm_sine.abs() || out[1].pwm_sine == 0.0);
        // The fade follows 1 - e^(-t/tau) with tau = delay / DELAY_TAUS.
        let tau = delay / DELAY_TAUS;
        for &secs in &[0.05f32, 0.2, 0.4] {
            let i = (secs * FS) as usize;
            let expected = 1.0 - (-secs / tau).exp();
            let ratio = out[i].sine / out[i].pwm_sine;
            assert!(
                (ratio - expected).abs() < 0.02 || out[i].pwm_sine.abs() < 0.05,
                "at {secs} s the fade is {ratio:.3}, expected {expected:.3}"
            );
        }
        // And it arrives: by the control's own time the sine is all but full.
        let i = (delay * FS) as usize;
        let ratio = out[i].sine / out[i].pwm_sine;
        assert!(
            ratio > 0.94 || out[i].pwm_sine.abs() < 0.05,
            "at the delay time: {ratio:.3}"
        );
        assert_eq!(m.fade(), 1.0, "and snaps to exactly full");
    }

    #[test]
    fn a_zero_delay_is_transparent_and_a_trigger_does_not_reset_the_phase() {
        let mut m = Modulator::new();
        m.trigger();
        let o = m.process(5.0, 0.0, 0.0, FS);
        assert_eq!(m.fade(), 1.0);
        assert_eq!(o.sine, o.pwm_sine);
        for _ in 0..1_000 {
            m.process(2.0, 0.0, 0.0, FS);
        }
        let before = m.phase;
        m.trigger();
        assert_eq!(
            m.phase, before,
            "the dump diode empties the capacitor, not the phase"
        );
        assert_eq!(m.fade(), 0.0);
    }

    #[test]
    fn settling_arrives_at_full_depth_without_moving_the_phase_or_the_hold() {
        let mut m = Modulator::new();
        run(&mut m, 0.3, 4.0, 0.0);
        m.trigger();
        let (phase, held) = (m.phase, m.held);
        m.settle();
        assert_eq!(m.fade(), 1.0);
        assert_eq!((m.phase, m.held), (phase, held));
    }
}
