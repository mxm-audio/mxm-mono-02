//! mxm-mono-02's routing as the collection's modulation standard checks it
//! (`mxm_modulation::conformance`; `plans/plan-modulation-standard.md`).
//!
//! Behind the `conformance` feature, which only `[dev-dependencies]` enable — this crate's own
//! tests, and the plugin's, whose route readings are held to [`Declared::deliver`] — so no shipped
//! graph carries it. [`Declared`] answers every question through [`crate::routing`]'s own tables,
//! its own sum and a real [`Graph`], never a copy of them.

use mxm_modulation::conformance::{Declaration, Kind};
use mxm_modulation::standard::{self, Offer, Performance};

use crate::routing::{
    self, Graph, KEY_UNIT_SEMITONES, Routing, SOURCE_NAMES, SOURCES, TARGET_NAMES, TARGETS, target,
};

/// What each target is, for the standard: a pitch, a width (narrowing in this machine's sign, both
/// halves live), a cutoff and the amplitude factor.
const KINDS: [Kind; TARGETS] = [Kind::Pitch, Kind::Width, Kind::Cutoff, Kind::Amplitude];

/// mxm-mono-02's routing declaration.
#[derive(Debug, Clone, Copy, Default)]
pub struct Declared;

/// Exactly one route, at `amount`.
fn one_route(target: usize, source: usize, amount: f32) -> Routing {
    let mut routing = Routing::new();
    routing.present[target][source] = true;
    routing.amounts[target][source] = amount;
    routing.compact();
    routing
}

impl Declaration for Declared {
    fn sources(&self) -> usize {
        SOURCES
    }

    fn targets(&self) -> usize {
        TARGETS
    }

    fn performance(&self, source: usize) -> Option<Performance> {
        routing::PERFORMANCE[source]
    }

    fn kind(&self, target: usize) -> Kind {
        KINDS[target]
    }

    fn machine(&self, target: usize, source: usize) -> bool {
        routing::machine(target, source)
    }

    fn offered(&self, target: usize, source: usize) -> Offer {
        routing::offer(target, source)
    }

    fn key_unit(&self) -> f32 {
        KEY_UNIT_SEMITONES
    }

    /// One route alone through a voice's own [`Graph`], its source publishing `raw`; Amplitude
    /// through the factor the voice applies.
    fn deliver(&self, target: usize, source: usize, amount: f32, raw: f32) -> f32 {
        let routing = one_route(target, source, amount);
        let mut graph = Graph::new();
        graph.set_topology(&routing);
        graph.begin_sample();
        graph.write(source, raw);
        let sum = graph.sum(target, &routing);
        if target == target::AMPLITUDE {
            standard::amplitude_factor(sum) - 1.0
        } else {
            sum
        }
    }

    fn name(&self, target: usize, source: usize) -> String {
        format!("{} from {}", TARGET_NAMES[target], SOURCE_NAMES[source])
    }
}

#[cfg(test)]
mod tests {
    use mxm_modulation::conformance::{self, Case, Input};

    use super::*;
    use crate::keyboard::NoteId;
    use crate::routing::source;
    use crate::voice::{Params, TriggerMode, VcaMode, Voice};

    fn report(result: Result<(), Vec<String>>) {
        if let Err(failures) = result {
            panic!("{} failure(s):\n{}", failures.len(), failures.join("\n"));
        }
    }

    fn id(key: u8) -> NoteId {
        NoteId {
            voice_id: None,
            channel: 0,
            key,
        }
    }

    /// **Every pair means what the standard says**: offered as `standard::offer` says, nothing at
    /// its source's rest, a meaningful move at full, and the standard reach for every pair the SH-2
    /// did not have.
    ///
    /// Falsified before trusted: with the added cutoff reach left at the envelope's ten octaves, it
    /// names every added cutoff pair from a performance source.
    #[test]
    fn every_pair_means_what_the_standard_says() {
        report(conformance::check_declaration(&Declared));
    }

    /// **A voice publishes what the standard says**: Key from middle C over its six-octave unit,
    /// Velocity as `v − 1`, the gestures as they arrive, each exactly zero at rest.
    ///
    /// Falsified before trusted: publishing the raw velocity fails at every input.
    #[test]
    fn a_voice_publishes_what_the_standard_says() {
        report(conformance::check_publishers(&Declared, |from, input| {
            // A route reads the source, so the voice publishes it; at zero depth nothing moves.
            let routing = one_route(target::CUTOFF, from, 0.0);
            let mut voice = Voice::new();
            voice.set_topology(&routing);
            let mut p = Params::default();
            let (key, velocity) = match input {
                Input::Note(n) => (n, 1.0),
                Input::Normalised(value) if from == source::VELOCITY => (60, value),
                _ => (60, 1.0),
            };
            match input {
                Input::Normalised(value) if from == source::WHEEL => p.mod_wheel = value,
                Input::Normalised(value) if from == source::PRESSURE => p.pressure = value,
                Input::Lever(value) => p.bend_position = value,
                _ => {}
            }
            voice.note_on(id(key), p.trigger_mode, velocity);
            voice.process(&p, &routing);
            voice.published_for_test(from)
        }));
    }

    /// **Velocity is the press that last triggered the envelope**: in GATE mode a legato press
    /// sounds its key without retriggering and keeps the phrase's velocity; before any press the
    /// source rests, so a HOLD drone is not touched by a velocity route.
    ///
    /// Falsified before trusted: storing every press's velocity reads the legato press's.
    #[test]
    fn velocity_is_the_press_that_last_triggered_the_envelope() {
        let routing = one_route(target::CUTOFF, source::VELOCITY, 0.0);
        let p = Params {
            trigger_mode: TriggerMode::Gate,
            ..Params::default()
        };
        let mut voice = Voice::new();
        voice.set_topology(&routing);
        voice.process(&p, &routing);
        assert_eq!(
            voice.published_for_test(source::VELOCITY),
            0.0,
            "at rest before any press"
        );
        voice.note_on(id(60), p.trigger_mode, 0.25);
        voice.process(&p, &routing);
        assert_eq!(voice.published_for_test(source::VELOCITY), -0.75);
        voice.note_on(id(64), p.trigger_mode, 0.875);
        voice.process(&p, &routing);
        assert_eq!(
            voice.published_for_test(source::VELOCITY),
            -0.75,
            "a legato press keeps the phrase's"
        );
    }

    /// **After a release, no performance route holds a note open** — every pair, both halves,
    /// the softest and hardest notes and the keyboard's ends, gestures held at full through the
    /// note and let go at the release, through the envelope and through the gate. HOLD is the
    /// machine's own drone and is not a release.
    #[test]
    fn after_a_release_no_performance_route_holds_a_note_open() {
        report(conformance::check_release_silence(
            &Declared,
            &[],
            |case: Case| {
                [VcaMode::Env, VcaMode::Gate].into_iter().all(|vca_mode| {
                    let routing = one_route(case.target, case.source, case.amount);
                    let mut voice = Voice::new();
                    voice.set_topology(&routing);
                    let mut p = Params {
                        mod_wheel: 1.0,
                        pressure: 1.0,
                        bend_position: 1.0,
                        release_s: 0.05,
                        vca_mode,
                        ..Params::default()
                    };
                    voice.note_on(id(case.key), p.trigger_mode, case.velocity);
                    for _ in 0..4_800 {
                        voice.process(&p, &routing);
                    }
                    voice.note_off(None, 0, case.key);
                    p.mod_wheel = 0.0;
                    p.pressure = 0.0;
                    p.bend_position = 0.0;
                    (0..144_000).any(|_| voice.process(&p, &routing) == 0.0 && !voice.is_active())
                })
            },
        ));
    }
}
