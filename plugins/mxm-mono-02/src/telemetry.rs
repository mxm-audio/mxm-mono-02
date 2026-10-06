//! The **only** channel from the audio thread to the editor.
//!
//! Atomics, written once per block, read whenever the editor happens to look. No locks, no
//! allocation, and the UI may drop as many frames as it likes — a display that made the audio
//! thread wait would be a display that could cause a dropout.
//!
//! Two rules carried from `plugins/mxm-mono-01/src/telemetry.rs`, both of which exist because the
//! obvious implementation loses information:
//!
//! - **A peak is max-combined and reset when the UI reads it.** Overwriting each block means a
//!   transient that landed between two frames is simply gone; combining means the value is always
//!   *loudest since you last looked*.
//! - **A clip latches until acknowledged.** A meter that quietly forgets it clipped is worse than
//!   no meter, and design system §5.4 requires the indication to persist.
//!
//! # What this instrument publishes beyond the meter
//!
//! **Where the cutoff actually is**, in hertz, once per block. The control shows a position and the
//! envelope dominates the sum so hard that the sweep clips at the ceiling (wart 12); the curve in the
//! Filter card draws the position, and this value lets it draw where the sum has taken it. Exact —
//! the value the DSP used — rather than re-derived in the editor.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};

/// How many samples of the mixer's output the scope keeps: 2048 is 43 ms at 48 kHz, one and a
/// half cycles of the lowest 32' note and enough of everything above it to find a trigger.
pub const SCOPE_LEN: usize = 2048;

#[derive(Debug)]
pub struct Telemetry {
    /// Peak of the samples produced, max-combined, reset on read.
    peak: AtomicU32,
    /// Sticky: set when a sample reaches full scale, cleared only by the user.
    clipped: AtomicBool,
    /// The cutoff the DSP used on the block's last sample, in hertz.
    cutoff_hz: AtomicU32,
    /// The envelope's level on the block's last sample.
    envelope: AtomicU32,
    /// Published once in `activate`, because the filter curve is plotted against it and it changes
    /// only when the host reconfigures.
    sample_rate: AtomicU32,
    /// The developer channel's requests of the editor: a view to show, and whether the expander
    /// is open. `NO_REQUEST` when nothing is asked. See `plugins/AGENTS.md`.
    dev_view: AtomicU8,
    dev_disclosure: AtomicU8,
    /// The developer channel's request to open or close the preset browser, or `NO_REQUEST`.
    dev_browser: AtomicU8,
    /// The developer channel's request to show a theme, by index, or `NO_REQUEST`. Theme is
    /// interface state, so this reaches the editor and nothing else; the DSP never sees it.
    dev_theme: AtomicU8,
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
            peak: AtomicU32::new(0),
            clipped: AtomicBool::new(false),
            cutoff_hz: AtomicU32::new(0),
            envelope: AtomicU32::new(0),
            sample_rate: AtomicU32::new(48_000f32.to_bits()),
            scope: (0..SCOPE_LEN).map(|_| AtomicU32::new(0)).collect(),
            scope_head: AtomicUsize::new(0),
            editor_open: AtomicBool::new(false),
            tempo: mxm_tempo::TempoCell::new(),
            dev_view: AtomicU8::new(u8::MAX),
            dev_disclosure: AtomicU8::new(u8::MAX),
            dev_browser: AtomicU8::new(u8::MAX),
            dev_theme: AtomicU8::new(u8::MAX),
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

    /// Publish a block's peak. **Combined, not overwritten**: see the module doc.
    pub fn publish_peak(&self, peak: f32) {
        let mut current = self.peak.load(Ordering::Relaxed);
        loop {
            let combined = f32::from_bits(current).max(peak);
            match self.peak.compare_exchange_weak(
                current,
                combined.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(seen) => current = seen,
            }
        }
        if peak >= 1.0 {
            self.clipped.store(true, Ordering::Relaxed);
        }
    }

    pub fn publish_voice(&self, cutoff_hz: f32, envelope: f32) {
        self.cutoff_hz.store(cutoff_hz.to_bits(), Ordering::Relaxed);
        self.envelope.store(envelope.to_bits(), Ordering::Relaxed);
    }

    pub fn publish_sample_rate(&self, rate: f32) {
        self.sample_rate.store(rate.to_bits(), Ordering::Relaxed);
    }

    // ---- editor thread ----

    /// The loudest sample since this was last called, **and resets**.
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    pub fn clipped(&self) -> bool {
        self.clipped.load(Ordering::Relaxed)
    }

    /// Acknowledge the clip indication. The user's act, never a timeout.
    pub fn clear_clip(&self) {
        self.clipped.store(false, Ordering::Relaxed);
    }

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
    fn a_peak_is_combined_and_reset_on_read() {
        let t = Telemetry::new();
        t.publish_peak(0.4);
        t.publish_peak(0.9);
        t.publish_peak(0.2);
        assert_eq!(t.take_peak(), 0.9, "the loudest of the three, not the last");
        assert_eq!(t.take_peak(), 0.0, "and reading resets it");
    }

    #[test]
    fn a_clip_latches_until_acknowledged() {
        let t = Telemetry::new();
        assert!(!t.clipped());
        t.publish_peak(1.0);
        assert!(t.clipped());
        for _ in 0..100 {
            t.publish_peak(0.1);
        }
        assert!(t.clipped(), "a meter that forgets is worse than no meter");
        t.clear_clip();
        assert!(!t.clipped());
    }

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

/// Nothing requested on a developer-channel slot.
const NO_REQUEST: u8 = u8::MAX;

/// The developer channel's requests of the editor — `plugins/AGENTS.md`, *A developer channel in
/// every editor*. Each is taken once; the DSP reads nothing.
impl Telemetry {
    /// Developer category address (0–5), or Parameters (127); never a derived tab index.
    pub fn request_view(&self, view: u8) {
        self.dev_view
            .store(view.min(NO_REQUEST - 1), Ordering::Relaxed);
    }

    /// The developer channel asks the editor to open or close the preset browser.
    pub fn request_browser(&self, open: bool) {
        self.dev_browser.store(u8::from(open), Ordering::Relaxed);
    }

    /// Whether the developer channel asked the browser open or closed since the editor last
    /// looked, if it did.
    pub fn take_browser_request(&self) -> Option<bool> {
        match self.dev_browser.swap(NO_REQUEST, Ordering::Relaxed) {
            NO_REQUEST => None,
            open => Some(open != 0),
        }
    }

    /// The developer channel asks the editor for a theme, by index — 0 light, 1 dark, 2 system,
    /// as `mxm_ui::theme::from_index` reads it.
    pub fn request_theme(&self, theme: u8) {
        self.dev_theme
            .store(theme.min(NO_REQUEST - 1), Ordering::Relaxed);
    }

    /// The theme the developer channel asked for since the editor last looked, if any.
    pub fn take_theme_request(&self) -> Option<u8> {
        match self.dev_theme.swap(NO_REQUEST, Ordering::Relaxed) {
            NO_REQUEST => None,
            theme => Some(theme),
        }
    }

    /// The developer channel asks the editor to open or close its expander.
    pub fn request_disclosure(&self, open: bool) {
        self.dev_disclosure.store(u8::from(open), Ordering::Relaxed);
    }

    /// The view the developer channel asked for since the editor last looked, if any.
    pub fn take_view_request(&self) -> Option<usize> {
        match self.dev_view.swap(NO_REQUEST, Ordering::Relaxed) {
            NO_REQUEST => None,
            view => Some(usize::from(view)),
        }
    }

    /// Whether the developer channel asked the expander open or closed since the editor last
    /// looked, if it did.
    pub fn take_disclosure_request(&self) -> Option<bool> {
        match self.dev_disclosure.swap(NO_REQUEST, Ordering::Relaxed) {
            NO_REQUEST => None,
            open => Some(open != 0),
        }
    }
}

#[cfg(test)]
mod developer_channel_tests {
    use super::*;

    #[test]
    fn a_developer_request_is_taken_once() {
        let t = Telemetry::new();
        assert_eq!(
            t.take_view_request(),
            None,
            "nothing asked on a fresh instance"
        );
        t.request_view(1);
        assert_eq!(t.take_view_request(), Some(1));
        assert_eq!(t.take_view_request(), None, "and taking it clears it");
        t.request_disclosure(true);
        assert_eq!(t.take_disclosure_request(), Some(true));
        t.request_browser(true);
        assert_eq!(t.take_browser_request(), Some(true));
        assert_eq!(t.take_browser_request(), None, "taken once");
        assert_eq!(t.take_disclosure_request(), None);
    }
}
