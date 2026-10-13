//! The **only** channel from the audio thread to the editor.
//!
//! Atomics, written once per block, read whenever the editor happens to look. No locks, no
//! allocation, and the UI may drop as many frames as it likes — a display that made the audio
//! thread wait would be a display that could cause a dropout.
//!
//! **The meter and the developer channel's requests are `mxm_telemetry`'s**, held as `meter` and
//! `dev`, with one policy for every plugin: a peak is max-combined and reset when the UI reads it,
//! a clip latches until acknowledged (design system §5.4), and a non-finite peak latches the clip
//! and is never the level. What follows here is this plugin's own.
//!
//! # What this instrument publishes beyond the meter
//!
//! **Where the cutoff actually is**, in hertz, once per block. The control shows a position and the
//! envelope dominates the sum so hard that the sweep clips at the ceiling (wart 12); the curve in the
//! Filter card draws the position, and this value lets it draw where the sum has taken it. Exact —
//! the value the DSP used — rather than re-derived in the editor.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// How many samples of the mixer's output the scope keeps: 2048 is 43 ms at 48 kHz, one and a
/// half cycles of the lowest 32' note and enough of everything above it to find a trigger.
pub const SCOPE_LEN: usize = 2048;

#[derive(Debug)]
pub struct Telemetry {
    /// The output level and its clip latch: the collection's one meter policy
    /// (`mxm_telemetry::PeakMeter`).
    pub meter: mxm_telemetry::PeakMeter,
    /// The developer channel's requests of the editor, each taken once
    /// (`mxm_telemetry::DevRequests`).
    pub dev: mxm_telemetry::DevRequests,
    /// The cutoff the DSP used on the block's last sample, in hertz.
    cutoff_hz: AtomicU32,
    /// The envelope's level on the block's last sample.
    envelope: AtomicU32,
    /// Published once in `activate`, because the filter curve is plotted against it and it changes
    /// only when the host reconfigures.
    sample_rate: AtomicU32,
    /// The mixer's output, every sample, as a ring: the one value written per sample rather than
    /// per block, because a waveform cannot be summarised. Relaxed stores of one word each; the
    /// editor reads a snapshot that may straddle a write, which on a scope is one sample of tear.
    scope: Box<[AtomicU32]>,
    /// Where the next sample goes. Wraps at `usize::MAX`, which is not a concern at any rate.
    scope_head: AtomicUsize,
    /// Whether an editor exists to read the ring. `plugins/AGENTS.md`: visualization work stops
    /// or throttles while the editor is hidden — the per-sample scope writes are the one cost
    /// here worth a branch, and they are skipped while this is false.
    editor_open: AtomicBool,
    /// The host tempo in force, so a synced LFO rate reads its division.
    pub tempo: mxm_tempo::TempoCell,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Telemetry {
    pub fn new() -> Self {
        Self {
            meter: mxm_telemetry::PeakMeter::new(),
            dev: mxm_telemetry::DevRequests::new(),
            cutoff_hz: AtomicU32::new(0),
            envelope: AtomicU32::new(0),
            sample_rate: AtomicU32::new(48_000f32.to_bits()),
            scope: (0..SCOPE_LEN).map(|_| AtomicU32::new(0)).collect(),
            scope_head: AtomicUsize::new(0),
            editor_open: AtomicBool::new(false),
            tempo: mxm_tempo::TempoCell::new(),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    // ---- audio thread ----

    /// One sample of the mixer's output into the ring.
    #[inline]
    pub fn push_scope(&self, sample: f32) {
        let index = self.scope_head.fetch_add(1, Ordering::Relaxed) % SCOPE_LEN;
        self.scope[index].store(sample.to_bits(), Ordering::Relaxed);
    }

    /// Read once per block: whether the scope is worth filling.
    pub fn editor_open(&self) -> bool {
        self.editor_open.load(Ordering::Relaxed)
    }

    // ---- editor ----

    /// The editor's lifecycle: set when it is built, cleared when it closes.
    pub fn set_editor_open(&self, open: bool) {
        self.editor_open.store(open, Ordering::Relaxed);
    }

    /// The ring, oldest sample first, into `out`.
    pub fn scope_snapshot(&self, out: &mut Vec<f32>) {
        out.clear();
        let head = self.scope_head.load(Ordering::Relaxed);
        out.extend((0..SCOPE_LEN).map(|k| {
            let index = head.wrapping_add(k) % SCOPE_LEN;
            f32::from_bits(self.scope[index].load(Ordering::Relaxed))
        }));
    }

    pub fn publish_voice(&self, cutoff_hz: f32, envelope: f32) {
        self.cutoff_hz.store(cutoff_hz.to_bits(), Ordering::Relaxed);
        self.envelope.store(envelope.to_bits(), Ordering::Relaxed);
    }

    pub fn publish_sample_rate(&self, rate: f32) {
        self.sample_rate.store(rate.to_bits(), Ordering::Relaxed);
    }

    // ---- editor thread ----

    pub fn cutoff_hz(&self) -> f32 {
        f32::from_bits(self.cutoff_hz.load(Ordering::Relaxed))
    }

    pub fn envelope(&self) -> f32 {
        f32::from_bits(self.envelope.load(Ordering::Relaxed))
    }

    pub fn sample_rate(&self) -> f32 {
        f32::from_bits(self.sample_rate.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_voice_values_and_the_sample_rate_survive_a_round_trip() {
        let t = Telemetry::new();
        t.publish_voice(1234.5, 0.25);
        t.publish_sample_rate(96_000.0);
        assert_eq!(t.cutoff_hz(), 1234.5);
        assert_eq!(t.envelope(), 0.25);
        assert_eq!(t.sample_rate(), 96_000.0);
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    #[test]
    fn the_scope_is_not_wanted_until_an_editor_exists() {
        let t = Telemetry::new();
        assert!(
            !t.editor_open(),
            "no editor: the per-sample writes are skipped"
        );
        t.set_editor_open(true);
        assert!(t.editor_open());
        t.set_editor_open(false);
        assert!(!t.editor_open());
    }

    #[test]
    fn the_ring_reads_back_oldest_first() {
        let t = Telemetry::new();
        for i in 0..(SCOPE_LEN + 5) {
            t.push_scope(i as f32);
        }
        let mut out = Vec::new();
        t.scope_snapshot(&mut out);
        assert_eq!(out.len(), SCOPE_LEN);
        assert_eq!(out[0], 5.0, "the oldest surviving sample comes first");
        assert_eq!(
            out[SCOPE_LEN - 1],
            (SCOPE_LEN + 4) as f32,
            "the newest last"
        );
    }
}
