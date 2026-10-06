//! mxm-mono-02 on rendered audio, through the player's own hosting path.
//!
//! What no unit test in either crate can see: that the shipped `.clap` loads, that a note played
//! through the player comes out at its pitch, that a parameter written by the host reaches the
//! sound, that **the keyboard block's low-note priority survives the host's event path**, and
//! that HOLD keeps the voice sounding with no key down — the `KeepAlive` path — and that leaving
//! HOLD ends it in exact silence.
//!
//! Skips, with the reason, when the bundle is not built: run
//! `cargo xtask bundle mxm-mono-02 --release` first.

use mxm_player_harness::app_harness;

use mxm_player::events::input::Payload;
use mxm_player::session::{FRAMES_PER_BLOCK, Session};
use std::path::PathBuf;

const PLUGIN: &str = "dk.mxm.mxm-mono-02";
const SAMPLE_RATE: f64 = 48_000.0;
const SKIP: &str = "skipping: run `cargo xtask bundle mxm-mono-02 --release`";

fn bundle() -> Option<(PathBuf, PathBuf)> {
    let dir = app_harness::bundled_dir_with("mxm-mono-02")?;
    let file = dir.join("mxm-mono-02.clap");
    file.exists().then_some((dir, file))
}

fn session(name: &str) -> Option<Session> {
    let (dir, file) = bundle()?;
    let mut s = Session::scratch(name, vec![dir]);
    s.load(&file, PLUGIN);
    Some(s)
}

// --- measuring ---------------------------------------------------------------------------------

use mxm_measure::channels::left;

/// Peak magnitude of a capture.
///
/// **A shim over `mxm-measure`, and the `expect` is the point.** The shared ruler reports absence for
/// a **non-finite** buffer rather than the largest number in it, because `f32::max` would otherwise
/// let a render that is half NaN measure as perfectly healthy — and then pass every "is it quiet?"
/// assertion below. Panicking here is the loud failure that behaviour deserves.
fn peak(samples: &[f32]) -> f32 {
    mxm_measure::level::peak(samples).expect("the capture is finite")
}

/// How much of one frequency is in a captured window — **a relative figure, not an amplitude.**
///
/// Two reasons it is relative, and both matter to anyone quoting a number from these tests:
///
/// - **The capture length is the session's, not ours.** `component_amplitude` reads a component's
///   true amplitude only over a whole number of cycles; these windows are whole blocks, so the
///   reading carries spectral leakage. Comparing one pitch against another in the same window is
///   sound — the leakage is common to both — and calling the result an absolute amplitude is not.
/// - **The absolute value moved by 6 dB with the migration to the shared probe**, which is a
///   correction rather than a regression: every local copy of this helper computed `|X|/N`, half a
///   component's amplitude, and the shared probe reports the amplitude. A figure quoted from an
///   older run of these tests is 6 dB low.
///
/// **Absence panics rather than reading as zero.** The probe declines for two reasons — an empty
/// window, which cannot happen here, and a **non-finite render**, which can. Folding that into `0.0`
/// would let a NaN-producing plugin sail through every "quieter than" and "silent" assertion below,
/// which is the precise failure the shared crate's result-form contract exists to prevent.
fn magnitude_at(samples: &[f32], hz: f64) -> f64 {
    mxm_measure::spectrum::component_amplitude(samples, hz, SAMPLE_RATE)
        .expect("the capture is non-empty and finite")
}

use mxm_measure::convert::note_hz;

/// A crude high-frequency measure: mean absolute sample-to-sample difference. Relative only.
fn brightness(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (samples.len() - 1) as f32
}

// --- driving -----------------------------------------------------------------------------------

/// Sets a parameter by display name, as a position in its normalised range, and lets it settle.
fn set_param(session: &mut Session, name: &str, fraction: f64) -> String {
    let param = session
        .state()
        .param(name)
        .unwrap_or_else(|| panic!("`{name}` is not a parameter"))
        .clone();
    let value = param.min + fraction * (param.max - param.min);
    session
        .app()
        .engine_mut()
        .push_gui_event(Payload::ParamValue {
            param_id: param.id,
            value,
        });
    session.advance_blocks(4).expect("the session advances");
    session
        .state()
        .param(name)
        .map(|p| p.text.clone())
        .unwrap_or_default()
}

/// Renders `blocks` of whatever is sounding and returns the left channel after the first few
/// blocks, so an attack or a retrigger is not in the measurement.
fn capture(session: &mut Session, blocks: u64) -> Vec<f32> {
    session.clear_capture();
    session.advance_blocks(blocks).expect("advances");
    let audio = session.captured();
    let skip = (FRAMES_PER_BLOCK * 2 * 3).min(audio.len());
    left(&audio[skip..])
}

/// Holds `note`, renders `blocks`, releases it, and returns the held part after the attack.
fn note(session: &mut Session, note: u8, blocks: u64) -> Vec<f32> {
    session.app().note_on(note, 100.0 / 127.0);
    let held = capture(session, blocks);
    session.app().note_off(note);
    session.advance_blocks(80).expect("advances");
    held
}

// --- the tests ---------------------------------------------------------------------------------

#[test]
fn at_rest_it_is_exactly_silent() {
    let Some(mut s) = session("mono02-rest") else {
        eprintln!("{SKIP}");
        return;
    };
    s.advance_blocks(20).expect("advances");
    assert_eq!(
        peak(&s.captured()),
        0.0,
        "an idle synth must render exact zeros, not merely something quiet"
    );
}

#[test]
fn a_note_sounds_at_its_pitch_and_the_release_ends_in_exact_silence() {
    let Some(mut s) = session("mono02-note") else {
        eprintln!("{SKIP}");
        return;
    };
    let held = note(&mut s, 57, 40);
    assert!(
        peak(&held) > 0.05,
        "the note is audible: peak {}",
        peak(&held)
    );

    // The init patch is VCO-1's sawtooth alone: its fundamental stands above the neighbouring
    // semitones.
    let hz = note_hz(57.0);
    let at = magnitude_at(&held, hz);
    let below = magnitude_at(&held, hz / 2f64.powf(1.0 / 12.0));
    let above = magnitude_at(&held, hz * 2f64.powf(1.0 / 12.0));
    assert!(
        at > 2.0 * below && at > 2.0 * above,
        "A3 should dominate its neighbours: {at:.4} against {below:.4} and {above:.4}"
    );

    // After the release and the settle, exact zeros: the voice reported itself inert.
    s.clear_capture();
    s.advance_blocks(40).expect("advances");
    assert_eq!(
        peak(&s.captured()),
        0.0,
        "the tail must end in exact silence"
    );
}

#[test]
fn the_cutoff_written_by_the_host_darkens_the_note() {
    let Some(mut s) = session("mono02-cutoff") else {
        eprintln!("{SKIP}");
        return;
    };
    let open = note(&mut s, 45, 40);
    let text = set_param(&mut s, "Cutoff", 0.25);
    let closed = note(&mut s, 45, 40);
    assert!(
        brightness(&closed) < 0.5 * brightness(&open),
        "closing the filter (now {text}) should darken the note: {} against {}",
        brightness(&closed),
        brightness(&open)
    );
}

#[test]
fn the_lower_key_wins_and_the_higher_returns_when_it_is_released() {
    let Some(mut s) = session("mono02-priority") else {
        eprintln!("{SKIP}");
        return;
    };
    let (low, high) = (note_hz(57.0), note_hz(69.0));

    // A4 alone: nothing at A3, an octave below its fundamental.
    s.app().note_on(69, 100.0 / 127.0);
    let alone = capture(&mut s, 30);
    assert!(
        magnitude_at(&alone, low) < 0.3 * magnitude_at(&alone, high),
        "A4 alone has no energy an octave below it"
    );

    // A3 pressed while A4 is held: the lower key takes the voice, so the fundamental moves down
    // and stays there while both are held.
    s.app().note_on(57, 100.0 / 127.0);
    let both = capture(&mut s, 30);
    assert!(
        magnitude_at(&both, low) > magnitude_at(&both, high),
        "the lower key should be sounding: A3 {:.4} against A4 {:.4}",
        magnitude_at(&both, low),
        magnitude_at(&both, high)
    );

    // Releasing A3 falls back to the key still held — no new press, the voice never stops.
    s.app().note_off(57);
    let back = capture(&mut s, 30);
    assert!(
        magnitude_at(&back, low) < 0.3 * magnitude_at(&back, high),
        "the higher key should return: A3 {:.4} against A4 {:.4}",
        magnitude_at(&back, low),
        magnitude_at(&back, high)
    );
    assert!(
        peak(&back) > 0.05,
        "and the voice keeps sounding across the change"
    );
    s.app().note_off(69);
}

#[test]
fn hold_keeps_the_voice_sounding_with_no_key_down_and_the_envelope_ends_it() {
    let Some(mut s) = session("mono02-hold") else {
        eprintln!("{SKIP}");
        return;
    };
    // The amplifier's three-way switch: HOLD leaves the VCA open whatever the keyboard does, so
    // one press and release is a drone — and the host must keep calling the plugin.
    let text = set_param(&mut s, "VCA", 0.0);
    assert_eq!(text, "Hold", "the switch reads back its position");

    s.app().note_on(48, 100.0 / 127.0);
    s.advance_blocks(10).expect("advances");
    s.app().note_off(48);
    s.advance_blocks(60).expect("advances");

    let drone = capture(&mut s, 200);
    assert!(
        peak(&drone) > 0.05,
        "HOLD should keep sounding with no key down: peak {}",
        peak(&drone)
    );

    // Back on the envelope, with no key down, the release runs out and the voice goes exactly
    // silent.
    let text = set_param(&mut s, "VCA", 0.5);
    assert_eq!(text, "Envelope");
    s.advance_blocks(120).expect("advances");
    s.clear_capture();
    s.advance_blocks(40).expect("advances");
    assert_eq!(
        peak(&s.captured()),
        0.0,
        "on the envelope with no key down, it is exactly silent"
    );
}

/// **The headline gesture, through the real callback**: the pulse-width section's LFO position — a
/// source switch and a depth on the machine, the route `mod_width_lfowidth` now — switched on and
/// raised by the host. Init shows no pulse-width route since 2026-09-27, so the host adds it first.
///
/// A pulse at square has no second harmonic, and a narrowing one grows it, so the gesture is heard as
/// the second harmonic moving window to window. Before the route is raised the pulse sits at square
/// and the harmonic is absent in every window; after, it swings. A route wired to the wrong target,
/// or a parameter that reaches nothing, leaves the second half looking like the first.
#[test]
fn raising_the_pulse_width_lfo_route_sweeps_the_pulse() {
    let Some(mut s) = session("mono02-pwm-route") else {
        eprintln!("{SKIP}");
        return;
    };
    assert_eq!(set_param(&mut s, "VCO-1 wave", 1.0), "Pulse");
    // About 1.3 Hz on the skewed range: one whole cycle inside the sixteen windows.
    set_param(&mut s, "LFO rate", 0.33);
    let hz = note_hz(57.0);

    let second_harmonic = |s: &mut Session| -> Vec<f64> {
        let audio = capture(s, 83);
        audio
            .as_chunks::<{ 5 * FRAMES_PER_BLOCK }>()
            .0
            .iter()
            .map(|w| magnitude_at(w, 2.0 * hz) / magnitude_at(w, hz))
            .collect()
    };

    s.app().note_on(57, 100.0 / 127.0);
    s.advance_blocks(8).expect("advances");
    let still = second_harmonic(&mut s);
    set_param(&mut s, "Pulse width from LFO to width on", 1.0);
    let text = set_param(&mut s, "Pulse width from LFO to width", 1.0);
    assert!(
        text.contains("-45"),
        "the route reads the section's full swing: {text}"
    );
    let swept = second_harmonic(&mut s);
    s.app().note_off(57);

    let spread = |r: &[f64]| {
        let lo = r.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = r.iter().copied().fold(0.0f64, f64::max);
        (lo, hi)
    };
    let (_, still_hi) = spread(&still);
    let (swept_lo, swept_hi) = spread(&swept);
    assert!(
        still_hi < 0.1,
        "at square the second harmonic is absent: {still:?}"
    );
    assert!(
        swept_hi > 0.4 && swept_hi - swept_lo > 0.3,
        "the route sweeps the width: {swept:?}"
    );
}

/// The plugin's own reading of one parameter, straight from the loaded instance. `PlayerState`
/// corrects its readback to the player's remembered patch, so it cannot show what a state load
/// restored.
fn reading(session: &mut Session, name: &str) -> String {
    session
        .app()
        .engine_mut()
        .read_params()
        .params
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("`{name}` is not a parameter"))
        .text
}

/// **A project saved with the Follower's routes still loads** (the owner, 2026-09-26). The external
/// input and its Follower source were removed and their eight route ids retired; a host's state
/// written before that still names them. The wrapper skips an id it does not have and restores the
/// rest, so the cutoff it carried comes back. The state is this bundle's own, dumped through the
/// CLAP state extension, with the retired ids written into its parameter map.
#[test]
fn a_state_naming_the_retired_follower_routes_still_loads() {
    let Some(mut s) = session("mono02-retired-state") else {
        eprintln!("{SKIP}");
        return;
    };
    set_param(&mut s, "Cutoff", 0.25);
    let saved = reading(&mut s, "Cutoff");
    assert!(s.app().run_cli_command("dumpstate").contains("\"ok\""));

    // nice-plug's CLAP state: the JSON's length as eight little-endian bytes, then the JSON.
    let path = s.dir().join("preset.clapstate");
    let bytes = std::fs::read(&path).expect("the dumped state");
    let json = String::from_utf8(bytes[8..].to_vec()).expect("the state after its length is JSON");
    assert!(
        json.contains("\"params\":{"),
        "an uncompressed state: {json:.80}"
    );
    // The entries written below take the shape the state's own entries have.
    assert!(
        json.contains("\"cutoff\":{\"f32\":"),
        "a float is stored as {{\"f32\": value}}: {json:.200}"
    );
    let retired: String = ["pitch", "width", "cutoff", "amp"]
        .iter()
        .map(|t| {
            format!(
                "\"mod_{t}_follower\":{{\"f32\":0.6}},\"mod_{t}_followeron\":{{\"bool\":true}},"
            )
        })
        .collect();
    let json = json.replacen("\"params\":{", &format!("\"params\":{{{retired}"), 1);
    let mut state = (json.len() as u64).to_le_bytes().to_vec();
    state.extend_from_slice(json.as_bytes());
    std::fs::write(&path, state).expect("the state is written back");

    set_param(&mut s, "Cutoff", 0.9);
    assert_ne!(
        reading(&mut s, "Cutoff"),
        saved,
        "the premise: the cutoff moved"
    );
    let answer = s.app().run_cli_command("loadstate");
    assert!(
        answer.contains("\"ok\""),
        "a state naming retired ids loads: {answer}"
    );
    assert_eq!(
        reading(&mut s, "Cutoff"),
        saved,
        "the rest of the state is restored"
    );
}
