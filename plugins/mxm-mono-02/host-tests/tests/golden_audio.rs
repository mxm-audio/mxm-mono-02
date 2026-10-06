//! Golden audio for mxm-mono-02 through the real MXM Player host path.
//!
//! **What the score exercises**, and why each is in it: both oscillators and the sub (VCO-1 on its
//! pulse, VCO-2 detuned against it), the shared pulse width swept by the modulator's un-delayed
//! sine, the envelope on the cutoff with resonance up, keyboard tracking, vibrato, the auto bend's
//! dip, portamento, and the keyboard block's low-note priority — a lower press taking the voice with
//! a trigger, and its release handing back to the held higher press with none — then the release
//! tail to silence.
//!
//! **Regenerating.** A deliberate DSP or shell change should fail this test. Then: rebuild the
//! bundle (`cargo xtask bundle mxm-mono-02 --release`), run this test, **listen to the WAV it
//! names**, review the DSP diff that moved it, and only then pin the new digest here with a line
//! saying why it moved. Never update the digest to make the test pass.
//!
//! **Pinned at M0 of `plans/plan-mxm-mono-02-modulation.md`** as `dfafd491b58bb2ed`, against the
//! bundle built before the routing conversion, and **provisionally repinned at its M4** as
//! `cc5225cc63e39d8d`. What moved it, measured against the M0 render: the largest sample difference
//! is 2.2e-5, 9.6e-5 of the score's peak (−80 dB), on 69 946 of 98 304 samples — rounding in size,
//! consistent with the routes multiplying in another order than the retired controls did and each
//! depth arriving as a signed amount. No human listening is claimed: the owner's listening pass at
//! the plan's M5 confirms this pin or replaces it.
//!
//! **The score's gestures are fixed; its parameter names were translated once**, at that conversion,
//! by the plan's §4 table: the PWM source switch and depth became `Pulse width from LFO to width`,
//! the envelope's depth `Cutoff from Envelope`, keyboard tracking `Cutoff from Key`, the modulator's
//! pitch depth `Pitch from LFO` and the auto bend `Pitch from Auto bend` — each a signed amount, so a
//! depth `d` is the fraction `(d + 1) / 2` of its range.

use mxm_player::events::input::Payload;
use mxm_player::session::{FRAMES_PER_BLOCK, Session};
use mxm_player_harness::app_harness;
use std::path::PathBuf;

const PLUGIN: &str = "dk.mxm.mxm-mono-02";
const GOLDEN_DIGEST: &str = "cc5225cc63e39d8d";
const GOLDEN_SAMPLES: usize = 96 * FRAMES_PER_BLOCK * 2;

fn bundle() -> Option<(PathBuf, PathBuf)> {
    let dir = app_harness::any_bundled_dir()?;
    let file = dir.join("mxm-mono-02.clap");
    file.exists().then_some((dir, file))
}

/// Sets a parameter by display name, as a fraction of its plain range.
fn set(s: &mut Session, name: &str, f: f64) {
    let p = s
        .state()
        .param(name)
        .unwrap_or_else(|| panic!("`{name}` is not a parameter"))
        .clone();
    s.app().engine_mut().push_gui_event(Payload::ParamValue {
        param_id: p.id,
        value: p.min + f * (p.max - p.min),
    });
}

/// The patch the score plays. Fixed forever.
fn patch(s: &mut Session) {
    set(s, "VCO-1 wave", 1.0);
    set(s, "VCO-2", 0.7);
    set(s, "VCO-2 tune", 0.53);
    set(s, "Sub", 0.4);
    set(s, "Cutoff", 0.4);
    set(s, "Resonance", 0.35);
    set(s, "Sustain", 0.5);
    set(s, "LFO rate", 0.4);
    set(s, "Portamento", 0.02);
    // Present at Init when the score was pinned; switched on here since Init stopped showing it
    // (2026-09-27), so the patch is the one that was pinned.
    set(s, "Pulse width from LFO to width on", 1.0);
    set(s, "Pulse width from LFO to width", 0.8);
    set(s, "Cutoff from Envelope", 0.75);
    set(s, "Cutoff from Key", 0.75);
    set(s, "Pitch from LFO", 0.55);
    set(s, "Pitch from Auto bend", 0.65);
}

/// Fixed forever: a press, a lower press taking the voice, its release handing back, the last
/// release and the tail. `extra` runs after the patch, so a sensitivity check plays the same score.
fn score(s: &mut Session) -> Result<(), String> {
    score_with(s, |_| {})
}

fn score_with(s: &mut Session, extra: impl FnOnce(&mut Session)) -> Result<(), String> {
    patch(s);
    extra(s);
    s.advance_blocks(4)?;
    s.app().note_on(57, 0.8);
    s.advance_blocks(20)?;
    s.app().note_on(45, 0.8);
    s.advance_blocks(16)?;
    s.app().note_off(45);
    s.advance_blocks(16)?;
    s.app().note_off(57);
    s.advance_blocks(40)
}

fn render(name: &str) -> Option<(Vec<f32>, PathBuf)> {
    let (dir, file) = bundle()?;
    let mut s = Session::scratch(name, vec![dir]);
    s.load(&file, PLUGIN);
    score(&mut s).unwrap();
    let samples = s.captured();
    let (wav, _) = s.write_artifacts(name).unwrap();
    Some((samples, wav))
}

#[test]
fn the_fixed_real_host_score_has_not_moved() {
    let Some((samples, wav)) = render("golden-mono-02") else {
        eprintln!("skipping: run `cargo xtask bundle mxm-mono-02 --release`");
        return;
    };
    assert_eq!(samples.len(), GOLDEN_SAMPLES);
    assert!(
        samples.iter().any(|x| x.abs() > 1e-4),
        "the score is silent"
    );
    let actual = digest(&samples);
    assert_eq!(
        actual,
        GOLDEN_DIGEST,
        "render moved; listen to {} and, if intended, pin {actual}",
        wav.display()
    );
}

/// The digest must move when a modulation depth the score sets moves, or it proves nothing about
/// the modulation it claims to exercise.
#[test]
fn the_reference_is_sensitive_to_the_vibrato_depth() {
    let Some((dir, file)) = bundle() else {
        eprintln!("skipping: run `cargo xtask bundle mxm-mono-02 --release`");
        return;
    };
    let mut s = Session::scratch("golden-mono-02-sensitive", vec![dir]);
    s.load(&file, PLUGIN);
    score_with(&mut s, |s| set(s, "Pitch from LFO", 0.95)).unwrap();
    let samples = s.captured();
    assert_eq!(
        samples.len(),
        GOLDEN_SAMPLES,
        "the same score, so the same length"
    );
    assert_ne!(digest(&samples), GOLDEN_DIGEST);
}

fn digest(samples: &[f32]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}
