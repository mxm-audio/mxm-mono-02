//! A listening harness: the mechanisms the tests measure, played one after another.
//!
//! Not an assertion — the tests are the assertions. This exists so a person can *hear*
//! what the numbers describe: two oscillators beating, the low-note keyboard's
//! asymmetric legato, the auto bend, the delayed vibrato, the shared PWM narrowing, the
//! envelope clipping the cutoff, and a HOLD drone with the random modulator on the
//! filter. Mono, because the machine is.
//!
//! Run with: `cargo run -p mxm-mono-02-dsp --release --example sh2_demo`
//! Writes `mxm-mono-02-demo.wav` in the working directory.

#![allow(clippy::field_reassign_with_default)] // patches read as `let mut p = Params::default(); p.x = …`

/// Writes a listening demo, applying this collection's demo headroom law **at the call site**.
///
/// `mxm_audio_file` encodes what it is given and applies no gain — normalisation is a judgement
/// about the material and the file crate carries no policy. The law here is the one the
/// six hand-written writers all applied internally: leave 2 % of headroom, and scale down further if
/// the material is over full scale.
fn write_demo(path: &str, interleaved: &[f32], channels: u16, rate: u32) {
    let peak = mxm_measure::level::peak(interleaved)
        .expect("a rendered demo is finite; a NaN here is a DSP defect, not a level");
    let gain = if peak > 1.0 { 0.98 / peak } else { 0.98 };
    let scaled: Vec<f32> = interleaved.iter().map(|s| s * gain).collect();
    mxm_audio_file::write(
        path,
        &scaled,
        channels,
        rate,
        mxm_audio_file::Target::Wav(mxm_audio_file::Bits::Sixteen),
    )
    .expect("the demo is written");
}

/// **Where this demo's channel count and sample rate are decided — once, for `main` and for the
/// test below.** Both call this, so a change to either constant changes both paths and the test's
/// literal expectations catch it. With the two supplied separately at each site, a `main` passing
/// the wrong channel count left the test perfectly green.
const DEMO_CHANNELS: u16 = 1;

fn write_demo_file(path: &str, interleaved: &[f32]) {
    write_demo(path, interleaved, DEMO_CHANNELS, FS as u32);
}

use mxm_mono_02_dsp::keyboard::NoteId;
use mxm_mono_02_dsp::lfo::Mode;
use mxm_mono_02_dsp::oscillator::{Wave1, Wave2};
use mxm_mono_02_dsp::routing::{Routing, source, target};
use mxm_mono_02_dsp::voice::{Params, TriggerMode, VcaMode, Voice};

const FS: f32 = 48_000.0;

fn id(key: u8) -> NoteId {
    NoteId {
        voice_id: None,
        channel: 0,
        key,
    }
}

struct Take {
    voice: Voice,
    routing: Routing,
    out: Vec<f32>,
}

impl Take {
    fn new() -> Self {
        let mut voice = Voice::new();
        voice.set_sample_rate(FS);
        Self {
            voice,
            routing: Routing::new(),
            out: Vec::new(),
        }
    }

    /// Sets one route, present, at this depth — what turning a depth up on the panel does.
    fn route(&mut self, target: usize, source: usize, amount: f32) {
        self.routing.present[target][source] = true;
        self.routing.amounts[target][source] = amount;
        self.routing.compact();
    }

    fn clear_routes(&mut self) {
        self.routing = Routing::new();
    }

    fn run(&mut self, p: &Params, secs: f32) {
        self.voice.set_topology(&self.routing);
        for _ in 0..(FS * secs) as usize {
            self.out.push(self.voice.process(p, &self.routing));
        }
    }

    fn note(&mut self, p: &Params, key: u8, secs: f32) {
        self.voice.note_on(id(key), p.trigger_mode, 1.0);
        self.run(p, secs);
        self.voice.note_off(None, 0, key);
    }

    /// The player's tie: the new note before the old one's release.
    fn tie(&mut self, p: &Params, from: u8, to: u8, secs: f32) {
        self.voice.note_on(id(to), p.trigger_mode, 1.0);
        self.voice.note_off(None, 0, from);
        self.run(p, secs);
    }
}

fn main() {
    let mut t = Take::new();

    // 1. Two oscillators beating: VCO-2 a few cents off, then the sub under both.
    let mut p = Params::default();
    p.vco2_level = 1.0;
    p.vco2_tune_cents = 9.0;
    p.release_s = 0.3;
    p.cutoff = 0.75;
    t.note(&p, 48, 1.5);
    p.sub_level = 0.8;
    t.note(&p, 48, 1.5);
    t.run(&p, 0.4);

    // 2. The keyboard's legato, asymmetric in GATE+TRIG: up is legato, down re-attacks.
    p.attack_s = 0.08;
    t.route(target::CUTOFF, source::ENVELOPE, 0.6);
    p.cutoff = 0.4;
    p.decay_s = 0.4;
    p.sustain = 0.3;
    t.voice.note_on(id(48), p.trigger_mode, 1.0);
    t.run(&p, 0.5);
    t.tie(&p, 48, 55, 0.5); // up: no re-attack
    t.tie(&p, 55, 60, 0.5); // up again
    t.tie(&p, 60, 43, 0.6); // down: a trigger
    t.voice.note_off(None, 0, 43);
    t.run(&p, 0.5);

    // 3. Auto bend and a delayed vibrato on a held note, then the same on a lower key.
    t.route(target::PITCH, source::AUTO_BEND, 0.6);
    p.lfo_delay_s = 0.6;
    t.route(target::PITCH, source::LFO, 0.25);
    p.lfo_rate_hz = 5.5;
    p.sustain = 0.8;
    t.voice.note_on(id(60), p.trigger_mode, 1.0);
    t.run(&p, 1.6);
    t.voice.note_on(id(67), p.trigger_mode, 1.0); // higher: nothing happens
    t.run(&p, 0.6);
    t.voice.note_on(id(53), p.trigger_mode, 1.0); // lower: bend and delay restart
    t.run(&p, 1.6);
    t.voice.all_notes_off();
    t.run(&p, 0.5);

    // 4. The shared PWM narrowing under the LFO on both pulses at once.
    let mut q = Params::default();
    q.vco1_wave = Wave1::Pulse;
    q.vco2_wave = Wave2::Pulse;
    q.vco2_level = 1.0;
    q.vco2_tune_cents = -6.0;
    t.clear_routes();
    t.route(target::PULSE_WIDTH, source::LFO_WIDTH, 1.0);
    q.lfo_rate_hz = 0.7;
    q.cutoff = 0.7;
    q.release_s = 0.2;
    t.note(&q, 43, 3.0);
    t.run(&q, 0.3);

    // 5. The envelope dominating the cutoff: full depth clips the sweep at the ceiling.
    let mut r = Params::default();
    r.cutoff = 0.35;
    t.clear_routes();
    t.route(target::CUTOFF, source::ENVELOPE, 1.0);
    r.attack_s = 0.4;
    r.decay_s = 0.5;
    r.sustain = 0.0;
    r.resonance = 0.55;
    r.release_s = 0.2;
    t.note(&r, 40, 1.4);
    t.route(target::CUTOFF, source::ENVELOPE, -1.0);
    r.cutoff = 0.8;
    t.note(&r, 40, 1.4);
    t.run(&r, 0.3);

    // 6. HOLD: a drone with the random modulator on the filter and the sine on pitch,
    //    then All Sound Off — and nothing until a key.
    let mut h = Params::default();
    h.vca_mode = VcaMode::Hold;
    h.vco1_wave = Wave1::Saw;
    h.vco2_wave = Wave2::Square;
    h.vco2_level = 0.7;
    h.vco2_tune_cents = 700.0;
    h.lfo_mode = Mode::Random;
    h.lfo_rate_hz = 6.0;
    t.clear_routes();
    t.route(target::CUTOFF, source::LFO, 0.6);
    h.cutoff = 0.45;
    h.resonance = 0.7;
    h.trigger_mode = TriggerMode::Gate;
    t.voice.note_on(id(36), h.trigger_mode, 0.0);
    t.voice.note_off(None, 0, 36);
    t.run(&h, 2.5);
    t.voice.all_sound_off();
    t.run(&h, 0.5);
    t.voice.note_on(id(43), h.trigger_mode, 0.0);
    t.voice.note_off(None, 0, 43);
    t.run(&h, 1.2);
    h.vca_mode = VcaMode::Env;
    t.run(&h, 0.5);

    let peak = t.out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let rms = (t.out.iter().map(|v| v * v).sum::<f32>() / t.out.len() as f32).sqrt();
    write_demo_file("mxm-mono-02-demo.wav", &t.out);
    println!(
        "wrote mxm-mono-02-demo.wav: {:.1} s, peak {peak:.3}, rms {rms:.3}",
        t.out.len() as f32 / FS
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The demo's own write path, exercised through the same wrapper `main` uses.
    ///
    /// The shared encoder is proved in `mxm-measure` against fixed header and payload bytes. What
    /// that cannot see is *this* file later writing the wrong channel count or rate, so the
    /// expectations here are **literals** — the facts about this instrument — rather than the
    /// constants under test.
    #[test]
    fn the_demo_write_path_produces_a_playable_file() {
        let frames = 256;
        let samples: Vec<f32> = (0..frames * DEMO_CHANNELS as usize)
            .map(|i| {
                let t = i as f32 / 48_000 as f32;
                // Past full scale, so the headroom branch is taken rather than skipped.
                1.6 * (std::f32::consts::TAU * 220.0 * t).sin()
            })
            .collect();

        let mut path = std::env::temp_dir();
        path.push(format!("sh2-demo-demo-{}.wav", std::process::id()));
        write_demo_file(path.to_str().expect("a utf-8 path"), &samples);

        let read = mxm_audio_file_decode::decode_file(
            &path,
            &mxm_audio_file_decode::Limits::new(
                usize::MAX,
                mxm_audio_file_decode::AtLimit::Refuse,
                mxm_audio_file_decode::Keep::AllUpTo(2),
            ),
        )
        .expect("the demo file parses");
        assert_eq!(read.channels, 1, "the demo wrote the wrong channel count");
        assert_eq!(
            read.sample_rate, 48_000,
            "the demo wrote the wrong sample rate"
        );
        assert_eq!(read.frames(), frames, "the demo dropped or invented frames");

        // The headroom law, asserted rather than assumed: a source at 1.6 comes back just under
        // full scale, not clipped to it and not left loud.
        let peak = mxm_measure::level::peak(&read.interleaved).expect("a finite file");
        assert!(
            (0.97..=0.985).contains(&peak),
            "the 0.98 headroom law did not run: peak {peak}"
        );
        std::fs::remove_file(&path).ok();
    }

    /// **The whole production path, `main` included.** This is what a writer test cannot otherwise
    /// reach: the render itself, the buffer `main` chooses, and the channel count and rate it hands
    /// over. An empty or truncated render fails here and nowhere else.
    ///
    /// `#[ignore]`d because it renders the demo in full, which is tens of seconds of audio; run it
    /// with `cargo test --all-targets -- --ignored` when the demo or its write path changes.
    #[test]
    #[ignore = "renders the whole demo; run with --ignored"]
    fn the_whole_demo_renders_and_writes_a_playable_file() {
        main();
        let read = mxm_audio_file_decode::decode_file(
            "mxm-mono-02-demo.wav",
            &mxm_audio_file_decode::Limits::new(
                usize::MAX,
                mxm_audio_file_decode::AtLimit::Refuse,
                mxm_audio_file_decode::Keep::AllUpTo(2),
            ),
        )
        .expect("the demo file parses");
        assert_eq!(read.channels, 1, "the demo wrote the wrong channel count");
        assert_eq!(
            read.sample_rate, 48000,
            "the demo wrote the wrong sample rate"
        );
        assert!(
            read.frames() > 48000,
            "the demo rendered under a second of audio"
        );
        let peak = mxm_measure::level::peak(&read.interleaved).expect("a finite render");
        assert!(peak > 0.1, "the demo rendered near-silence: peak {peak}");

        // `main` writes into the working directory, which under `cargo test` is the crate root.
        // Leaving it there drops an untracked WAV into the tree every time this runs.
        std::fs::remove_file("mxm-mono-02-demo.wav").ok();
    }
}
