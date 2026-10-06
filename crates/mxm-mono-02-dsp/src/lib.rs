//! DSP for mxm-mono-02.
//!
//! A monophonic two-oscillator voice inspired by the Roland SH-2, built in the order
//! `plans/plan-mxm-mono-02.md` §11 gives: the keyboard block first, because nothing in
//! the collection has its shape and most of what the machine *feels* like is that
//! block's logic rather than a tone control.
//!
//! Deliberately free of any plugin-framework types: everything here takes plain
//! values and a sample rate, so the whole voice is testable with `cargo test` and
//! no host involved.
//!
//! `flush` and `Rng` are the fifth honest copies of `mxm-mono-01-dsp`'s, byte for
//! byte, as the plan's §7.2 requires; the extraction plan owes them one evidence
//! row and gets it from this crate's NOTES.md.

#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
pub mod envelope;
pub mod filter;
pub mod keyboard;
pub mod lfo;
pub mod oscillator;
pub mod routing;
pub mod voice;

/// The lowest host rate the plugin activates at; a non-finite rate is refused with it.
///
/// `f32::clamp` panics when its lower bound is above its upper one or either is NaN, and the
/// ladder's cutoff is clamped to `10 Hz ..= 0.45 × rate`, which crosses below 22.2 Hz. 1 kHz is far
/// clear of that, and no higher than the lowest rate clap-validator (1234.57 Hz) or the player's
/// robustness sweeps (1 kHz) ask for.
pub const MIN_SAMPLE_RATE: f32 = 1_000.0;

/// Flush a recursive state toward zero before it can become denormal.
///
/// Denormal arithmetic can cost orders of magnitude more than normal arithmetic,
/// which in a feedback filter shows up as a CPU spike exactly when a note decays
/// into silence. We do this in the DSP rather than relying on a framework FTZ
/// guard: the guard may be a no-op unless an opt-in feature is enabled, and
/// flushing here is also what keeps digital silence *exactly* zero.
///
/// `1e-20` is far above the f32 denormal threshold (~1.18e-38) and about -400 dB,
/// so nothing audible is lost.
#[inline(always)]
pub fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// Small xorshift PRNG. Allocation-free, deterministic, and seeded explicitly so
/// every noise source in the synth is bit-repeatable for a given seed — which is
/// what makes the filter's self-oscillation excitation testable.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u32,
}

impl Rng {
    pub const fn new(seed: u32) -> Self {
        // A zero state is a fixed point for xorshift, so forbid it.
        Self {
            state: if seed == 0 { 0x9E37_79B9 } else { seed },
        }
    }

    /// Next uniform sample in `[-1, 1)`.
    #[inline]
    pub fn next_bipolar(&mut self) -> f32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        // Map the top 24 bits into [-1, 1) so the result is exactly representable.
        ((self.state >> 8) as f32 / 8_388_608.0) - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_preserves_audible_values_and_kills_tiny_ones() {
        assert_eq!(flush(0.0), 0.0);
        assert_eq!(flush(1e-30), 0.0);
        assert_eq!(flush(-1e-30), 0.0);
        assert_eq!(flush(0.5), 0.5);
        assert_eq!(flush(-1e-6), -1e-6);
    }

    #[test]
    fn rng_is_deterministic_and_bounded() {
        let mut a = Rng::new(0x1234_5678);
        let mut b = Rng::new(0x1234_5678);
        for _ in 0..10_000 {
            let x = a.next_bipolar();
            assert_eq!(x, b.next_bipolar(), "same seed must give same sequence");
            assert!((-1.0..1.0).contains(&x), "out of range: {x}");
        }
    }

    #[test]
    fn rng_never_gets_stuck_at_zero_state() {
        let mut r = Rng::new(0);
        let first = r.next_bipolar();
        let second = r.next_bipolar();
        assert_ne!(first, second);
    }
}
