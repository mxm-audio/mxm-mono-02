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
//! What this crate copied from its siblings and proved identical is the kit's now: the
//! generator is `mxm-rng`'s, and `flush`, the saturators, the PolyBLEP residual and the ADSR
//! are `mxm-dsp`'s. Its NOTES.md keeps the evidence row.

#[cfg(any(test, feature = "conformance"))]
pub mod conformance;
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
