//! **Every state the nine retired controls could reach is still reachable** — decision 1.13 of
//! `plans/plan-modulation-routing.md`, demonstrated as a property rather than asserted
//! (`plans/plan-mxm-mono-02-modulation.md` §8, `mxm-mono-pr1`'s shape).
//!
//! The legacy laws below are the voice's own arithmetic as it stood at `ba3bd30`, before the
//! conversion: the modulator on pitch with the wheel's push, the auto bend's dip, the pulse-width
//! section's three positions, the envelope's up and down positions on the cutoff, the modulator on
//! the cutoff, keyboard tracking and the bender's cutoff sensitivity. Each randomised legacy patch is
//! translated by plan §4's table into routes, and the two are compared **at the values the targets
//! receive** — semitones of pitch, the width, octaves of cutoff — with the sources the voice would
//! publish.
//!
//! **One retired position is no longer reachable, deliberately**: the envelope switch's ENV FOL'R,
//! which put the external input's follower on the cutoff. The external input was removed (the owner,
//! 2026-09-26), and the Follower source and its route with it.

use mxm_mono_02_dsp::Rng;
use mxm_mono_02_dsp::routing::{self, Graph, Routing, source, target};
use mxm_mono_02_dsp::voice::{
    AUTO_BEND_SEMITONES, BEND_FILTER_OCTAVES, FILTER_ENV_OCTAVES, FILTER_LFO_OCTAVES,
    INVERTED_RATIO, KEY_TRACK_CENTRE, KEY_TRACK_MAX, PWM_SWING, VCO_LFO_SEMITONES,
};

#[derive(Clone, Copy, Debug)]
enum Pwm {
    Env,
    Manual,
    Lfo,
}

#[derive(Clone, Copy, Debug)]
enum Env {
    Up,
    Down,
}

/// The nine retired controls, plus the two surviving ones their meaning depended on.
#[derive(Clone, Copy, Debug)]
struct Legacy {
    vcolfo: f32,
    autobend: f32,
    pulsewidth: f32,
    pwmdepth: f32,
    pwmmode: Pwm,
    envamount: f32,
    envpolarity: Env,
    vcflfo: f32,
    keytrack: f32,
    bendfilter: f32,
    bendrange: f32,
}

/// What the voice would publish on one sample.
#[derive(Clone, Copy, Debug)]
struct Sources {
    lfo: f32,
    pwm_sine: f32,
    envelope: f32,
    dip: f32,
    glided: f32,
    lever: f32,
    wheel: f32,
}

fn legacy_pitch(l: &Legacy, s: &Sources) -> f32 {
    (l.vcolfo + s.wheel).clamp(0.0, 1.0) * VCO_LFO_SEMITONES * s.lfo
        - l.autobend * AUTO_BEND_SEMITONES * s.dip
}

fn legacy_width(l: &Legacy, s: &Sources) -> f32 {
    match l.pwmmode {
        Pwm::Manual => l.pulsewidth.clamp(0.05, 0.5),
        Pwm::Lfo => 0.5 - l.pwmdepth * PWM_SWING * 0.5 * (1.0 + s.pwm_sine),
        Pwm::Env => 0.5 - l.pwmdepth * PWM_SWING * s.envelope,
    }
}

/// The cutoff's modulation in octaves, above the slider. The retired bender sensitivity scaled the
/// bend **in semitones** at a scale of one octave per octave of bend.
fn legacy_cutoff(l: &Legacy, s: &Sources) -> f32 {
    let env = match l.envpolarity {
        Env::Up => l.envamount * FILTER_ENV_OCTAVES * s.envelope,
        Env::Down => -l.envamount * FILTER_ENV_OCTAVES * INVERTED_RATIO * s.envelope,
    };
    l.keytrack * KEY_TRACK_MAX * (s.glided - KEY_TRACK_CENTRE) / 12.0
        + l.vcflfo * FILTER_LFO_OCTAVES * s.lfo
        + env
        + l.bendfilter * (s.lever * l.bendrange) / 12.0
}

/// Plan §4's translation: the machine's routes, at the amounts the table gives, and the width the
/// slider takes. From [`Routing::machine`], not Init: the pulse width's two are offered rather than
/// present on a fresh instance since 2026-09-27, and a translated patch switches them on.
fn translate(l: &Legacy) -> (Routing, f32) {
    let mut r = Routing::machine();
    let mut set = |t: usize, s: usize, a: f32| {
        assert!(
            (-1.0..=1.0).contains(&a),
            "{l:?}: the translated amount {a} into ({t}, {s}) is outside what a route can hold"
        );
        r.amounts[t][s] = a;
    };
    set(target::PITCH, source::LFO, l.vcolfo);
    set(target::PITCH, source::AUTO_BEND, l.autobend);
    let width = match l.pwmmode {
        Pwm::Manual => l.pulsewidth,
        Pwm::Lfo => {
            set(target::PULSE_WIDTH, source::LFO_WIDTH, l.pwmdepth);
            0.5
        }
        Pwm::Env => {
            set(target::PULSE_WIDTH, source::ENVELOPE, l.pwmdepth);
            0.5
        }
    };
    match l.envpolarity {
        Env::Up => set(target::CUTOFF, source::ENVELOPE, l.envamount),
        Env::Down => set(target::CUTOFF, source::ENVELOPE, -l.envamount),
    }
    set(target::CUTOFF, source::LFO, l.vcflfo);
    set(target::CUTOFF, source::KEY, l.keytrack);
    // At the patch's own bend range: the lever at full reached `bendfilter × range / 12` octaves.
    set(
        target::CUTOFF,
        source::BEND,
        l.bendfilter * l.bendrange / (12.0 * BEND_FILTER_OCTAVES),
    );
    (r, width)
}

/// The routes, through the graph the voice uses, reading the sources the voice would publish.
fn routed(r: &Routing, width: f32, s: &Sources) -> (f32, f32, f32) {
    let mut r = *r;
    // The plugin's legacy path: the wheel pushes the vibrato route's amount.
    let vibrato = &mut r.amounts[target::PITCH][source::LFO];
    *vibrato = (*vibrato + s.wheel).clamp(-1.0, 1.0);

    let mut graph = Graph::new();
    graph.set_topology(&r);
    graph.begin_sample();
    graph.write(source::LFO, s.lfo);
    graph.write(source::LFO_WIDTH, 0.5 * (1.0 + s.pwm_sine));
    graph.write(source::ENVELOPE, s.envelope);
    graph.write(source::AUTO_BEND, s.dip);
    graph.write(source::KEY, routing::key_source(s.glided));
    graph.write(source::WHEEL, s.wheel);
    graph.write(source::BEND, s.lever);
    (
        graph.sum(target::PITCH, &r),
        (width + graph.sum(target::PULSE_WIDTH, &r)).clamp(0.05, 0.5),
        graph.sum(target::CUTOFF, &r),
    )
}

#[test]
fn every_legacy_state_is_reachable_by_routes() {
    let mut rng = Rng::new(0x5eed_0002);
    let mut unit = move || 0.5 * (rng.next_bipolar() + 1.0);
    let (mut worst_pitch, mut worst_width, mut worst_cutoff) = (0.0f32, 0.0f32, 0.0f32);
    for trial in 0..20_000 {
        let pick = |u: f32, n: usize| ((u * n as f32) as usize).min(n - 1);
        let l = Legacy {
            vcolfo: unit(),
            autobend: unit(),
            pulsewidth: 0.05 + unit() * 0.45,
            pwmdepth: unit(),
            pwmmode: [Pwm::Env, Pwm::Manual, Pwm::Lfo][pick(unit(), 3)],
            envamount: unit(),
            envpolarity: [Env::Up, Env::Down][pick(unit(), 2)],
            vcflfo: unit(),
            keytrack: unit(),
            bendfilter: unit(),
            bendrange: unit() * 24.0,
        };
        let s = Sources {
            lfo: 2.0 * unit() - 1.0,
            pwm_sine: 2.0 * unit() - 1.0,
            envelope: unit(),
            dip: unit(),
            glided: unit() * 127.0,
            lever: 2.0 * unit() - 1.0,
            wheel: if trial % 3 == 0 { unit() } else { 0.0 },
        };
        let (r, width) = translate(&l);
        let (pitch, w, cutoff) = routed(&r, width, &s);
        let dp = (pitch - legacy_pitch(&l, &s)).abs();
        let dw = (w - legacy_width(&l, &s)).abs();
        let dc = (cutoff - legacy_cutoff(&l, &s)).abs();
        assert!(
            dp < 1e-4,
            "{trial}: pitch {pitch} against {} for {l:?} {s:?}",
            legacy_pitch(&l, &s)
        );
        assert!(
            dw < 1e-5,
            "{trial}: width {w} against {} for {l:?} {s:?}",
            legacy_width(&l, &s)
        );
        assert!(
            dc < 1e-4,
            "{trial}: cutoff {cutoff} against {} for {l:?} {s:?}",
            legacy_cutoff(&l, &s)
        );
        worst_pitch = worst_pitch.max(dp);
        worst_width = worst_width.max(dw);
        worst_cutoff = worst_cutoff.max(dc);
    }
    println!(
        "20 000 legacy patches: worst pitch {worst_pitch:.2e} st, width {worst_width:.2e}, cutoff {worst_cutoff:.2e} oct"
    );
}
