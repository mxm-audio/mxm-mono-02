//! The routing conversion's obligations, each a test that fails without the thing it names.
//!
//! `plans/plan-mxm-mono-02-modulation.md` §6, which is mxm-kit's `docs/code-review-notes.md` §7
//! made specific to this machine. Every assertion here was run against the defect it names; the
//! plan's §13 records each mutation and what it did.

// `let mut p = Params::default(); p.x = …` reads as the patch it is, as in the voice's own tests.
#![allow(clippy::field_reassign_with_default)]

use mxm_mono_02_dsp::keyboard::NoteId;
use mxm_mono_02_dsp::lfo::Mode;
use mxm_mono_02_dsp::oscillator::Wave1;
use mxm_mono_02_dsp::routing::{
    self, FULL_SCALE, Graph, Routing, SOURCES, TARGETS, source, target,
};
use mxm_mono_02_dsp::voice::{
    self, CUTOFF_FLOOR_HZ, CUTOFF_RANGE_OCTAVES, OUTPUT_BOUND, Params, VcaMode, Voice,
};

const FS: f32 = 48_000.0;
const NONE: Routing = Routing::new();

fn id(key: u8) -> NoteId {
    NoteId {
        voice_id: None,
        channel: 0,
        key,
    }
}

/// A routing with exactly these pairs present, at these amounts.
fn wired(pairs: &[(usize, usize, f32)]) -> Routing {
    let mut r = Routing::new();
    for &(t, s, a) in pairs {
        r.present[t][s] = true;
        r.amounts[t][s] = a;
    }
    r.compact();
    r
}

fn render(v: &mut Voice, p: &Params, r: &Routing, samples: usize) -> Vec<f32> {
    v.set_topology(r);
    (0..samples).map(|_| v.process(p, r)).collect()
}

/// **A source that becomes needed starts from silence, not from an old phrase.** While nothing
/// reads VCO-1 nothing publishes it, so its slot keeps the value from the last time something did;
/// a backward route added afterwards would read that value for one sample. Both kinds of gap: one
/// where another route keeps the frame running, and one with nothing routed at all.
#[test]
fn a_source_that_becomes_needed_starts_from_silence_not_from_an_old_phrase() {
    for keep_running in [true, false] {
        let mut graph = Graph::new();
        let reading = wired(&[(target::PITCH, source::VCO1, 1.0)]);
        graph.set_topology(&reading);
        graph.begin_sample();
        graph.write(source::VCO1, 0.8);
        assert_eq!(
            graph.sum(target::PITCH, &reading),
            (1.0 * 0.8f32) * FULL_SCALE[target::PITCH][source::VCO1],
            "the premise: the route reads what was published"
        );

        let gap = if keep_running {
            wired(&[(target::CUTOFF, source::ENVELOPE, 1.0)])
        } else {
            Routing::new()
        };
        graph.set_topology(&gap);
        for _ in 0..48_000 {
            graph.begin_sample();
            graph.write(source::ENVELOPE, 0.5);
            // Unread, so unpublished: the slot keeps 0.8.
            graph.write(source::VCO1, 0.3);
        }

        // Re-added as a backward route, and read before this sample has published it.
        graph.set_topology(&reading);
        graph.begin_sample();
        assert_eq!(
            graph.sum(target::PITCH, &reading),
            0.0,
            "keep_running {keep_running}: a newly read source must read silence, not the old phrase"
        );
    }
}

/// **The auto bend's dip runs whether routed or not.** It is not gated on a route reading it, so it
/// owes no reset when a route arrives: the value a new route reads is the one the circuit has been
/// producing all along.
#[test]
fn the_auto_bend_runs_whether_routed_or_not() {
    let p = Params::default();
    let routed = wired(&[(target::PITCH, source::AUTO_BEND, 0.5)]);
    let mut a = Voice::new();
    let mut b = Voice::new();
    a.set_topology(&routed);
    b.set_topology(&NONE);
    a.note_on(id(60), p.trigger_mode, 1.0);
    b.note_on(id(60), p.trigger_mode, 1.0);
    for i in 0..FS as usize {
        a.process(&p, &routed);
        b.process(&p, &NONE);
        if i == 0 {
            assert!(b.auto_bend_shape() > 0.9, "the premise: the dip charged");
        }
        assert_eq!(
            a.auto_bend_shape(),
            b.auto_bend_shape(),
            "sample {i}: the dip"
        );
    }
}

/// **A route at zero depth counts for nothing in any predicate.** An idle voice whose resting-cutoff
/// routes are all present at zero neither wakes nor shows a different cutoff — and the same route
/// at depth does move the display, so the display is reading the routing and not ignoring it. And
/// the settle: a HOLD drone at zero volume is silent by its own setting, so it idles, and an
/// amplitude route present at zero depth must not be what holds it awake.
#[test]
fn a_zero_depth_route_neither_wakes_an_idle_voice_nor_moves_its_resting_cutoff() {
    let mut p = Params::default();
    p.cutoff = 0.5;
    p.bend_position = 1.0;
    p.mod_wheel = 1.0;
    p.pressure = 1.0;
    let zero = wired(&[
        (target::CUTOFF, source::BEND, 0.0),
        (target::CUTOFF, source::WHEEL, 0.0),
        (target::CUTOFF, source::PRESSURE, 0.0),
        (target::CUTOFF, source::KEY, 0.0),
        (target::AMPLITUDE, source::WHEEL, 0.0),
    ]);
    let mut routed = Voice::new();
    let mut bare = Voice::new();
    let x = render(&mut routed, &p, &zero, 4_800);
    render(&mut bare, &p, &NONE, 4_800);
    assert!(
        x.iter().all(|s| *s == 0.0),
        "an idle voice renders exact zeros"
    );
    assert!(!routed.is_active(), "zero-depth routes woke nothing");
    assert_eq!(
        routed.cutoff_hz(),
        bare.cutoff_hz(),
        "zero-depth routes moved the resting cutoff"
    );

    let deep = wired(&[(target::CUTOFF, source::BEND, 0.5)]);
    let mut moved = Voice::new();
    render(&mut moved, &p, &deep, 64);
    assert!(
        (moved.cutoff_hz() / bare.cutoff_hz() - 2.0).abs() < 1e-3,
        "the premise: half the Bend route's two octaves, with the lever at full, is one octave: {} against {}",
        moved.cutoff_hz(),
        bare.cutoff_hz()
    );

    let mut p = Params::default();
    p.vca_mode = VcaMode::Hold;
    p.volume = 0.0;
    let amp_zero = wired(&[(target::AMPLITUDE, source::LFO, 0.0)]);
    let mut routed = Voice::new();
    let mut bare = Voice::new();
    routed.set_topology(&amp_zero);
    bare.set_topology(&NONE);
    let mut idled = false;
    for i in 0..(FS * 2.0 * voice::POST_TAIL_S) as usize {
        routed.process(&p, &amp_zero);
        bare.process(&p, &NONE);
        assert_eq!(
            routed.is_active(),
            bare.is_active(),
            "sample {i}: a zero-depth amplitude route changed when a silent drone idles"
        );
        idled |= !bare.is_active();
    }
    assert!(idled, "the premise: a HOLD drone at zero volume idles");
}

/// **HOLD under a deep tremolo does not idle.** An amplitude route at −100 % from a slow square holds
/// a drone silent for longer than the settle; the silence is the route's, and idling there would
/// reset the filter and freeze the modulator that is about to open it again.
#[test]
fn hold_under_a_deep_tremolo_does_not_idle() {
    let mut p = Params::default();
    p.vca_mode = VcaMode::Hold;
    p.lfo_mode = Mode::Square;
    p.lfo_rate_hz = 0.2;
    let tremolo = wired(&[(target::AMPLITUDE, source::LFO, -1.0)]);
    let mut v = Voice::new();
    v.set_topology(&tremolo);
    let (mut run, mut longest) = (0usize, 0usize);
    for i in 0..(FS as usize * 8) {
        let y = v.process(&p, &tremolo);
        assert!(
            v.is_active(),
            "sample {i}: a drone held silent by a route went idle"
        );
        if y == 0.0 {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    assert!(
        longest as f32 > voice::POST_TAIL_S * FS,
        "the premise: the route held the drone silent past the settle, {longest} samples"
    );
}

/// **The signed laws are continuous, and they keep the machine's limits.** The envelope's reach into
/// the cutoff is continuous through zero and its inverted half is the ratio (wart 13); the width
/// never passes square nor goes under the narrowest (wart 4), whatever sign reaches it; the
/// amplifier can be closed or doubled and never inverted.
#[test]
fn the_signed_laws_are_continuous_and_keep_the_machines_limits() {
    use routing::reach;
    assert_eq!(reach(target::CUTOFF, source::ENVELOPE, 0.0), 0.0);
    for eps in [1e-3f32, 1e-6] {
        let up = reach(target::CUTOFF, source::ENVELOPE, eps);
        let down = reach(target::CUTOFF, source::ENVELOPE, -eps);
        assert!(
            up > 0.0 && down < 0.0 && up - down < (10.0 + 2.75 + 0.1) * eps,
            "discontinuous at {eps}"
        );
    }

    for amount in [-1.0f32, 1.0] {
        for from in [source::LFO_WIDTH, source::ENVELOPE, source::VCO1] {
            for manual in [0.05f32, 0.5] {
                let mut p = Params::default();
                p.pulse_width = manual;
                p.vco1_wave = Wave1::Pulse;
                p.lfo_rate_hz = 7.0;
                p.attack_s = 0.2;
                let r = wired(&[(target::PULSE_WIDTH, from, amount)]);
                let mut v = Voice::new();
                v.set_topology(&r);
                v.note_on(id(60), p.trigger_mode, 1.0);
                for _ in 0..FS as usize / 2 {
                    v.process(&p, &r);
                    let w = v.pulse_width();
                    assert!(
                        (0.05..=0.5).contains(&w),
                        "amount {amount} from {from} over {manual}: width {w}"
                    );
                }
            }
        }
    }

    let mut p = Params::default();
    p.vca_mode = VcaMode::Hold;
    p.lfo_rate_hz = 5.0;
    let push = wired(&[
        (target::AMPLITUDE, source::LFO, 1.0),
        (target::AMPLITUDE, source::VCO1, 1.0),
        (target::AMPLITUDE, source::NOISE, -1.0),
    ]);
    let mut routed = Voice::new();
    let mut bare = Voice::new();
    let a = render(&mut routed, &p, &push, FS as usize);
    let b = render(&mut bare, &p, &NONE, FS as usize);
    let mut closed = 0;
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        assert!(
            x * y >= 0.0,
            "sample {i}: the amplifier inverted, {x} against {y}"
        );
        assert!(
            x.abs() <= 2.0 * y.abs() + 1e-6,
            "sample {i}: more than double, {x} against {y}"
        );
        if *x == 0.0 && *y != 0.0 {
            closed += 1;
        }
    }
    assert!(
        closed > 0,
        "the premise: the routes closed the amplifier somewhere"
    );
}

/// **A route at full amount reads the machine's own number** — what the control it replaced reached
/// at its top, in the target's unit — so a player reading a route reads the machine.
#[test]
fn a_route_at_full_amount_reads_the_machines_own_number() {
    use routing::reach;
    assert_eq!(reach(target::PITCH, source::LFO, 1.0), 7.0);
    assert_eq!(reach(target::PITCH, source::AUTO_BEND, 1.0), -12.0);
    assert_eq!(
        reach(target::PULSE_WIDTH, source::LFO_WIDTH, 1.0),
        -(0.5 - 0.05)
    );
    assert_eq!(
        reach(target::PULSE_WIDTH, source::ENVELOPE, 1.0),
        -(0.5 - 0.05)
    );
    assert_eq!(reach(target::CUTOFF, source::ENVELOPE, 1.0), 10.0);
    assert!((reach(target::CUTOFF, source::ENVELOPE, -1.0) + 2.75).abs() < 1e-5);
    assert_eq!(reach(target::CUTOFF, source::LFO, 1.0), 4.0);
    assert_eq!(reach(target::CUTOFF, source::BEND, 1.0), 2.0);

    // The key, through its source's unit: 1.2 octaves of cutoff per octave of key, as the KYBD
    // slider's over-tracking top.
    let r = wired(&[(target::CUTOFF, source::KEY, 1.0)]);
    let mut graph = Graph::new();
    graph.set_topology(&r);
    graph.begin_sample();
    graph.write(source::KEY, routing::key_source(72.0));
    assert!((graph.sum(target::CUTOFF, &r) - 1.2).abs() < 1e-5);
}

/// **The declared evaluation order, run rather than restated.** VCO-1 is published after the
/// oscillators and before the cutoff, so a route from it is on time into the cutoff and one sample
/// late into pitch — read off what the voice actually did, sample by sample.
#[test]
fn a_route_from_vco1_is_a_sample_late_into_pitch_and_on_time_into_cutoff() {
    let mut p = Params::default();
    p.cutoff = 0.4;
    let n = 256;

    // The reference: nothing routed and VCO-1 alone, so the mixer's output is VCO-1 exactly halved.
    let mut bare = Voice::new();
    bare.note_on(id(60), p.trigger_mode, 1.0);
    let osc: Vec<f32> = (0..n)
        .map(|_| {
            bare.process(&p, &NONE);
            bare.mixer_output() * 2.0
        })
        .collect();

    let cutoff_for = |value: f32| {
        let sum =
            0.0f32 + (0.1 * value.clamp(-1.0, 1.0)) * FULL_SCALE[target::CUTOFF][source::VCO1];
        let octaves = p.cutoff.clamp(0.0, 1.0) * CUTOFF_RANGE_OCTAVES + sum;
        CUTOFF_FLOOR_HZ * octaves.clamp(0.0, CUTOFF_RANGE_OCTAVES).exp2()
    };
    let into_cutoff = wired(&[(target::CUTOFF, source::VCO1, 0.1)]);
    let mut v = Voice::new();
    v.set_topology(&into_cutoff);
    v.note_on(id(60), p.trigger_mode, 1.0);
    let mut late_would_match = 0;
    for i in 0..n {
        v.process(&p, &into_cutoff);
        assert_eq!(
            v.cutoff_hz(),
            cutoff_for(osc[i]),
            "sample {i}: VCO-1 into the cutoff is on time"
        );
        if i > 0 && v.cutoff_hz() == cutoff_for(osc[i - 1]) {
            late_would_match += 1;
        }
    }
    assert!(
        late_would_match < n / 4,
        "the premise: on time and late are distinguishable"
    );

    let pitch_for = |value: f32| {
        let pitch =
            0.0f32 + (0.01 * value.clamp(-1.0, 1.0)) * FULL_SCALE[target::PITCH][source::VCO1];
        let base = 60.0f32 + 0.0 / 100.0 + 0.0 + pitch;
        let key = base + 12.0 * 0.0 + 0.0;
        440.0 * ((key - 69.0) / 12.0).exp2()
    };
    let into_pitch = wired(&[(target::PITCH, source::VCO1, 0.01)]);
    let mut w = Voice::new();
    w.set_topology(&into_pitch);
    w.note_on(id(60), p.trigger_mode, 1.0);
    let mut previous = 0.0f32;
    let mut on_time_would_match = 0;
    for i in 0..n {
        w.process(&p, &into_pitch);
        let now = w.mixer_output() * 2.0;
        assert_eq!(
            w.pitch_hz(),
            pitch_for(previous),
            "sample {i}: VCO-1 into pitch is one sample late"
        );
        if i > 0 && w.pitch_hz() == pitch_for(now) {
            on_time_would_match += 1;
        }
        previous = now;
    }
    assert!(
        on_time_would_match < n / 4,
        "the premise: late and on time are distinguishable"
    );
}

/// **Every pair at extreme amounts stays finite and bounded**, from 1 kHz to 768 kHz — all 52 routes
/// present at once at full, then at full negative, HOLD open and resonance up.
#[test]
fn every_pair_at_extreme_amounts_stays_finite_and_bounded() {
    for fs in [1_000.0f32, 8_000.0, 44_100.0, 96_000.0, 768_000.0] {
        for amount in [1.0f32, -1.0] {
            let mut r = Routing::new();
            for t in 0..TARGETS {
                for s in 0..SOURCES {
                    r.present[t][s] = true;
                    r.amounts[t][s] = amount;
                }
            }
            r.compact();
            let mut p = Params::default();
            p.vco2_level = 1.0;
            p.sub_level = 1.0;
            p.resonance = 1.0;
            p.vca_mode = VcaMode::Hold;
            p.bend_position = amount;
            p.mod_wheel = 1.0;
            p.pressure = 1.0;
            let mut v = Voice::new();
            v.set_sample_rate(fs);
            v.set_topology(&r);
            v.note_on(id(60), p.trigger_mode, 1.0);
            for i in 0..(fs * 0.5) as usize {
                let y = v.process(&p, &r);
                assert!(y.is_finite(), "{fs} Hz at {amount}: non-finite at {i}");
                assert!(
                    y.abs() <= OUTPUT_BOUND,
                    "{fs} Hz at {amount}: {y} past the bound at {i}"
                );
            }
        }
    }
}

/// **A player-made cycle stays finite**: VCO-1 into its own pitch and width at full, the sub against
/// it, for ten seconds.
#[test]
fn a_player_made_cycle_stays_finite() {
    let mut p = Params::default();
    p.vco1_wave = Wave1::Pulse;
    let r = wired(&[
        (target::PITCH, source::VCO1, 1.0),
        (target::PULSE_WIDTH, source::VCO1, 1.0),
        (target::PITCH, source::SUB, -1.0),
    ]);
    let mut v = Voice::new();
    v.set_topology(&r);
    v.note_on(id(48), p.trigger_mode, 1.0);
    for i in 0..(FS as usize * 10) {
        let y = v.process(&p, &r);
        assert!(y.is_finite() && y.abs() <= OUTPUT_BOUND, "sample {i}: {y}");
        assert!(v.pitch_hz().is_finite(), "sample {i}: the pitch");
    }
}

/// **Audio-rate routes into the cutoff, resonance past the onset, from silence.** The resonance loop
/// is the one place a moving coefficient can pump energy (`mxm-mono-00`'s phaser is the warning), so
/// the corner is swept by three oscillators and the noise at full while the filter sings on its own
/// with every level down — and must stay bounded and must not grow.
#[test]
fn audio_rate_routes_into_the_cutoff_at_full_resonance_stay_bounded_from_silence() {
    for fs in [48_000.0f32, 192_000.0] {
        let mut p = Params::default();
        p.vco1_level = 0.0;
        p.resonance = 1.0;
        p.vca_mode = VcaMode::Hold;
        p.vco2_tune_cents = 700.0;
        let r = wired(&[
            (target::CUTOFF, source::VCO1, 1.0),
            (target::CUTOFF, source::VCO2, 1.0),
            (target::CUTOFF, source::SUB, 1.0),
            (target::CUTOFF, source::NOISE, 1.0),
        ]);
        let mut v = Voice::new();
        v.set_sample_rate(fs);
        v.set_topology(&r);
        let second = fs as usize;
        let mut rms = Vec::new();
        for _ in 0..5 {
            let mut energy = 0.0f64;
            for i in 0..second {
                let y = v.process(&p, &r);
                assert!(
                    y.is_finite() && y.abs() <= OUTPUT_BOUND,
                    "{fs} Hz, {i}: {y}"
                );
                energy += f64::from(y) * f64::from(y);
            }
            rms.push((energy / second as f64).sqrt());
        }
        assert!(
            rms[0] > 1e-3,
            "{fs} Hz: the premise, the filter sings, {rms:?}"
        );
        assert!(
            rms[4] <= rms[1] * 1.5 + 1e-3,
            "{fs} Hz: the loop grows, {rms:?}"
        );
    }
}

/// **`reset` clears both halves of the frame.** A voice that has routed VCO-1 backward into pitch,
/// then been reset, renders what a fresh voice renders from its first sample.
#[test]
fn reset_leaves_no_routed_tail() {
    let p = Params::default();
    let r = wired(&[(target::PITCH, source::VCO1, 1.0)]);
    let mut used = Voice::new();
    used.set_topology(&r);
    used.note_on(id(60), p.trigger_mode, 1.0);
    render(&mut used, &p, &r, 14_400);
    used.reset();
    used.note_on(id(60), p.trigger_mode, 1.0);
    let a = render(&mut used, &p, &r, 256);
    let mut fresh = Voice::new();
    fresh.note_on(id(60), p.trigger_mode, 1.0);
    let b = render(&mut fresh, &p, &r, 256);
    assert_eq!(a, b, "reset left the frame's history behind");
}

/// **The machine's own routes at zero render bit-identically to nothing routed** — all eight, and
/// Init's six. The init patch is the instrument as it was before routing existed, to the bit, while
/// every source it reads moves: the modulator, the envelope, the key through a glide and the
/// bender; and taking the pulse width's two off Init (2026-09-27) changed nothing heard.
#[test]
fn the_machines_own_routes_at_zero_render_bit_identically_to_nothing_routed() {
    let mut p = Params::default();
    p.vco2_level = 0.6;
    p.lfo_delay_s = 0.2;
    p.cutoff = 0.6;
    p.resonance = 0.4;
    p.portamento_s = 0.05;
    p.bend_position = 0.5;
    p.bend_semitones = 1.0;
    p.mod_wheel = 0.3;
    let init = Routing::init();
    let machine = Routing::machine();
    let mut out = Vec::new();
    for r in [&init, &machine, &NONE] {
        let mut v = Voice::new();
        v.set_topology(r);
        v.note_on(id(48), p.trigger_mode, 1.0);
        let mut x: Vec<f32> = (0..24_000).map(|_| v.process(&p, r)).collect();
        v.note_on(id(43), p.trigger_mode, 1.0);
        x.extend((0..24_000).map(|_| v.process(&p, r)));
        out.push(x);
    }
    assert_eq!(out[0], out[2], "the init routes at zero are not neutral");
    assert_eq!(
        out[1], out[2],
        "the machine's routes at zero are not neutral"
    );
}
