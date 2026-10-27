//! Presets: this instrument's factory set, and what the collection's preset crate needs of it.
//!
//! The format, the library on disk, favourites, the loaded identity and the app-bar controls are
//! `mxm-preset`'s — one crate for every instrument and effect, extracted from the five verbatim
//! copies this file used to be one of (`plugins/AGENTS.md`, *A preset is parameter values*). What
//! is left here is what only this instrument knows: its id, its parameters, and its sounds.

use std::sync::RwLock;

pub use mxm_preset::{
    Category, Entry, INIT_NAME, Library, Loaded, Origin, Preset, PresetIdentity, Refused, Value,
    factory, loaded, mark_loaded, mark_none, read_favourites, snapshot, write_favourites,
};

use crate::params::MxmMono02Params;

/// **The tempo syncs this plugin gained on 2026-09-25** (`plans/plan-tempo-sync-controls.md`). A
/// preset file written before them was written unsynced, so each loads off rather than keeping the
/// instance's sync, and without reporting a missing control.
pub(crate) const TEMPO_SYNC_IDS: &[&str] = &["lfosync"];

impl mxm_preset::Instrument for MxmMono02Params {
    fn clap_id(&self) -> &'static str {
        crate::CLAP_ID
    }

    /// In declaration order, from the one list the editor draws from, then every route. **The
    /// routes are parameters like any other**: a preset that did not name them would leave the
    /// previous patch's modulation in place, and the machine's own modulation *is* routes now.
    fn parameters(&self) -> Vec<(&'static str, &dyn mxm_params::ErasedParam)> {
        crate::editor::sections::all_parameters(self)
            .into_iter()
            .map(|bound| (bound.id, bound.param))
            .chain(self.routes.parameters())
            .collect()
    }

    fn identity(&self) -> &RwLock<PresetIdentity> {
        &self.preset
    }

    /// The installed set, but in a unit test, which is loaded from no bundle: the repository's.
    #[cfg(test)]
    fn factory_files(&self) -> Vec<(String, String)> {
        FACTORY_FILES
            .iter()
            .map(|(name, json)| ((*name).to_owned(), (*json).to_owned()))
            .collect()
    }

    fn default_missing_legacy_parameter(&self, id: &str) -> bool {
        TEMPO_SYNC_IDS.contains(&id)
    }
}

/// The factory set as it ships, `(name, json)` in its order: the repository's `presets/`, which
/// is installed beside the plugin and read there (`mxm_preset::installed_factory_dir`). For the
/// tests, the integration tests and the audits, which are loaded from no bundle; the plugin itself
/// never reads it.
pub static FACTORY_FILES: std::sync::LazyLock<Vec<(&'static str, &'static str)>> =
    std::sync::LazyLock::new(|| {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("presets");
        mxm_preset::read_factory_dir(&folder)
            .into_iter()
            .map(|(file, json)| {
                let name = mxm_preset::Preset::parse(&json, crate::CLAP_ID)
                    .map_or(file, |preset| preset.name);
                (&*String::leak(name), &*String::leak(json))
            })
            .collect()
    });

#[cfg(test)]
mod tests {
    use super::*;

    /// **A project saved before the tempo syncs restores them Off** (`mxm_preset::add_switches_off`),
    /// whatever this instance had.
    /// **Every stored `text` is this plugin's own formatting of the `v` beside it**
    /// (`mxm_preset::stale_factory_texts`).
    #[test]
    fn every_factory_text_is_this_plugins_formatting_of_its_value() {
        let stale = mxm_preset::stale_factory_texts(&MxmMono02Params::default());
        assert!(
            stale.is_empty(),
            "{} stale texts — regenerate the files:\n{}",
            stale.len(),
            stale.join("\n")
        );
    }

    #[test]
    fn an_older_state_restores_the_tempo_syncs_off() {
        use nice_plug::prelude::Plugin as _;
        let mut state = nice_plug::prelude::PluginState {
            version: String::new(),
            params: Default::default(),
            fields: Default::default(),
        };
        crate::MxmMono02::filter_state(&mut state);
        for id in TEMPO_SYNC_IDS {
            assert!(
                matches!(
                    state.params.get(*id),
                    Some(nice_plug::plugin::ParamValue::Bool(false))
                ),
                "{{id}} was not restored off"
            );
        }
    }

    /// **A preset saved before the tempo syncs loads them off, and cleanly** ([`TEMPO_SYNC_IDS`]).
    #[test]
    fn a_preset_from_before_the_tempo_syncs_loads_them_off() {
        let params = crate::params::MxmMono02Params::default();
        let mut old = mxm_preset::Preset::init(&params);
        for id in TEMPO_SYNC_IDS {
            old.params.remove(*id);
        }
        let (writes, problems) = old.resolve(&params);
        assert!(problems.is_empty(), "{{problems:?}}");
        for id in TEMPO_SYNC_IDS {
            assert!(
                writes.iter().any(|(w, _, v)| w == id && *v == 0.0),
                "{{id}} was not written off"
            );
        }
    }

    /// **A preset naming the retired Follower routes loads everything else** (the owner,
    /// 2026-09-26): a user preset saved while the external input was here names its eight route
    /// ids, which are reported and skipped, and every real parameter is still written.
    #[test]
    fn a_preset_naming_the_retired_follower_routes_loads_everything_else() {
        let params = crate::params::MxmMono02Params::default();
        let mut old = mxm_preset::Preset::init(&params);
        let retired = &crate::routes::RETIRED_IDS[9..];
        assert!(retired.iter().all(|id| id.contains("follower")));
        for id in retired {
            old.params.insert(
                (*id).to_owned(),
                mxm_preset::Value {
                    v: 0.8,
                    text: String::new(),
                },
            );
        }
        let (writes, problems) = old.resolve(&params);
        assert_eq!(problems.len(), retired.len(), "{problems:?}");
        for id in retired {
            assert!(
                problems.iter().any(|p| p.contains(&format!("`{id}`"))),
                "`{id}` was not the one skipped: {problems:?}"
            );
        }
        assert_eq!(
            writes.len(),
            mxm_preset::Instrument::parameters(&params).len(),
            "every real parameter still applies"
        );
    }

    use mxm_preset::user_root;
    use nice_plug::params::Param;

    fn params() -> MxmMono02Params {
        MxmMono02Params::default()
    }

    /// Prints every parameter as eleven `normalised=formatted` steps.
    ///
    /// A facility, not a test: designing a factory preset means choosing normalised values, and
    /// choosing them blind is how a preset ends up with a filter at 0.5 that nobody meant.
    ///
    /// ```text
    /// cargo test -p mxm-mono-02 --lib the_mapping_table -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "prints what each normalised value means, for preset design"]
    fn the_mapping_table() {
        let params = params();
        for bound in crate::editor::sections::all_parameters(&params) {
            let steps: Vec<String> = (0..=10)
                .map(|i| {
                    let v = i as f32 / 10.0;
                    format!("{v:.1}={}", bound.param.format(v))
                })
                .collect();
            eprintln!("{:<12} {}", bound.id, steps.join("  "));
        }
    }

    /// A signed route amount, as the normalised value a preset stores: zero is 0.5.
    const fn signed(amount: f32) -> f32 {
        (amount + 1.0) / 2.0
    }

    /// The factory sounds, as **overrides on the defaults**.
    ///
    /// Written as the handful of values that make each sound rather than as thirty-one numbers
    /// apiece: a file full of defaults hides the three that matter. `write_the_factory_presets`
    /// turns each into a complete file, because the format takes no sparse overlays — an overlay's
    /// meaning would change the day the defaults were retuned.
    ///
    /// **Twenty, and Init is not one of them.** Init is generated from the parameter defaults and
    /// has no file at all; see [`Preset::init`].
    ///
    /// Most designs are an envelope shape, a filter position and a chorus button — which is
    /// faithful, because that is how the machine's own patches were made.
    const FACTORY_DESIGN: &[Design] = &[
        ("Init saw", Category::Template, &[("cutoff", 0.85)]),
        (
            "Beating saws",
            Category::Pad,
            &[
                ("vco2", 1.0),
                ("vco2tune", 0.53),
                ("cutoff", 0.7),
                ("release", 0.35),
            ],
        ),
        (
            "Sub bass",
            Category::Bass,
            &[
                ("sub", 0.9),
                ("vco1range", 0.25),
                ("cutoff", 0.45),
                ("mod_cutoff_env", signed(0.3)),
                ("decay", 0.45),
                ("sustain", 0.4),
            ],
        ),
        (
            "Square lead",
            Category::Lead,
            &[
                ("vco1wave", 0.6667),
                ("vco2wave", 0.6667),
                ("vco2", 0.8),
                ("vco2tune", 0.47),
                ("cutoff", 0.6),
                ("resonance", 0.3),
                ("mod_pitch_lfo", signed(0.08)),
                ("lfodelay", 0.4),
            ],
        ),
        (
            "Pulse sweep",
            Category::Pad,
            &[
                ("vco1wave", 1.0),
                ("vco2wave", 1.0),
                ("vco2", 1.0),
                ("vco2tune", 0.52),
                ("mod_width_lfowidthon", 1.0),
                ("mod_width_lfowidth", signed(0.9)),
                ("lforate", 0.15),
                ("cutoff", 0.65),
            ],
        ),
        (
            "Sine flute",
            Category::Lead,
            &[
                ("vco1wave", 0.0),
                ("cutoff", 0.7),
                ("attack", 0.35),
                ("release", 0.45),
                ("mod_pitch_lfo", signed(0.06)),
                ("lfodelay", 0.5),
            ],
        ),
        (
            "Noise wash",
            Category::Fx,
            &[
                ("vco1", 0.0),
                ("vco2wave", 0.0),
                ("vco2", 1.0),
                ("cutoff", 0.35),
                ("resonance", 0.55),
                ("mod_cutoff_env", signed(0.4)),
                ("attack", 0.55),
                ("release", 0.6),
            ],
        ),
        (
            "Bent brass",
            Category::Brass,
            &[
                ("vco2", 0.9),
                ("vco2tune", 0.48),
                ("mod_pitch_autobend", signed(0.35)),
                ("cutoff", 0.4),
                ("mod_cutoff_env", signed(0.45)),
                ("attack", 0.2),
                ("decay", 0.4),
                ("sustain", 0.6),
            ],
        ),
        (
            "Delayed vibrato",
            Category::Lead,
            &[
                ("vco2", 0.7),
                ("vco2tune", 0.53),
                ("mod_pitch_lfo", signed(0.12)),
                ("lfodelay", 0.6),
                ("lforate", 0.5),
                ("cutoff", 0.55),
                ("release", 0.4),
            ],
        ),
        (
            "Shuffle bass",
            Category::Bass,
            &[
                ("trigger", 0.5),
                ("vco1range", 0.25),
                ("cutoff", 0.35),
                ("mod_cutoff_env", signed(0.5)),
                ("decay", 0.3),
                ("sustain", 0.2),
                ("resonance", 0.25),
            ],
        ),
        (
            "Organ hold",
            Category::Keys,
            &[
                ("vcamode", 0.0),
                ("vco1wave", 0.6667),
                ("sub", 0.6),
                ("vco2", 0.5),
                ("vco2range", 0.75),
                ("cutoff", 0.6),
            ],
        ),
        (
            "Random filter",
            Category::Sequence,
            &[
                ("lfomode", 1.0),
                ("lforate", 0.55),
                ("mod_cutoff_lfo", signed(0.6)),
                ("cutoff", 0.4),
                ("resonance", 0.6),
                ("vco2", 0.8),
                ("vco2tune", 0.52),
            ],
        ),
        (
            "Gate stab",
            Category::Pluck,
            &[
                ("vcamode", 1.0),
                ("cutoff", 0.3),
                ("mod_cutoff_env", signed(0.7)),
                ("decay", 0.25),
                ("sustain", 0.0),
                ("resonance", 0.4),
                ("vco2", 0.9),
                ("vco2tune", 0.51),
            ],
        ),
        (
            "Auto dip",
            Category::Lead,
            &[
                ("mod_pitch_autobend", signed(0.7)),
                ("cutoff", 0.5),
                ("resonance", 0.2),
                ("decay", 0.35),
                ("sustain", 0.5),
            ],
        ),
        (
            "Hollow fifths",
            Category::Pad,
            &[
                ("vco1wave", 0.6667),
                ("vco2wave", 0.6667),
                ("vco2", 1.0),
                ("vco2tunerange", 1.0),
                ("vco2tune", 0.7),
                ("cutoff", 0.55),
                ("release", 0.3),
            ],
        ),
        (
            "Wide detune",
            Category::Pad,
            &[
                ("vco2", 1.0),
                ("vco2tunerange", 1.0),
                ("vco2tune", 0.57),
                ("vco2range", 0.25),
                ("cutoff", 0.5),
                ("resonance", 0.15),
            ],
        ),
        (
            "Inverted pluck",
            Category::Pluck,
            &[
                ("cutoff", 0.75),
                ("mod_cutoff_env", signed(-0.6)),
                ("decay", 0.3),
                ("sustain", 0.0),
                ("resonance", 0.3),
            ],
        ),
        (
            "Tracking sweep",
            Category::Lead,
            &[
                ("mod_cutoff_key", signed(0.8)),
                ("cutoff", 0.3),
                ("resonance", 0.7),
                ("mod_cutoff_env", signed(0.35)),
                ("decay", 0.5),
            ],
        ),
        (
            "Slow glide",
            Category::Lead,
            &[
                ("portamento", 0.45),
                ("vco2", 0.8),
                ("vco2tune", 0.53),
                ("cutoff", 0.5),
                ("attack", 0.3),
                ("release", 0.45),
            ],
        ),
        (
            "LFO trill",
            Category::Lead,
            &[
                ("trigger", 1.0),
                ("lforate", 0.6),
                ("cutoff", 0.45),
                ("mod_cutoff_env", signed(0.5)),
                ("decay", 0.2),
                ("sustain", 0.0),
            ],
        ),
        (
            "Octave bass",
            Category::Bass,
            &[
                ("vco1range", 0.25),
                ("vco2", 0.8),
                ("vco2range", 0.0),
                ("cutoff", 0.4),
                ("mod_cutoff_env", signed(0.35)),
                ("decay", 0.4),
                ("sustain", 0.3),
                ("resonance", 0.15),
            ],
        ),
        (
            "Rubber sub",
            Category::Bass,
            &[
                ("vco1wave", 0.6667),
                ("sub", 0.7),
                ("vco1range", 0.25),
                ("cutoff", 0.3),
                ("resonance", 0.35),
                ("mod_cutoff_env", signed(0.45)),
                ("decay", 0.35),
                ("sustain", 0.1),
                ("release", 0.25),
            ],
        ),
        (
            "Reese",
            Category::Bass,
            &[
                ("vco2", 1.0),
                ("vco2tune", 0.6),
                ("vco1range", 0.25),
                ("vco2range", 0.25),
                ("cutoff", 0.35),
                ("resonance", 0.2),
                ("release", 0.4),
            ],
        ),
        (
            "Punch bass",
            Category::Bass,
            &[
                ("vco1range", 0.25),
                ("sub", 0.5),
                ("cutoff", 0.25),
                ("resonance", 0.4),
                ("mod_cutoff_env", signed(0.6)),
                ("attack", 0.0),
                ("decay", 0.28),
                ("sustain", 0.0),
                ("release", 0.2),
            ],
        ),
        (
            "Hollow bass",
            Category::Bass,
            &[
                ("vco1wave", 1.0),
                ("pulsewidth", 0.3),
                ("vco1range", 0.25),
                ("sub", 0.4),
                ("cutoff", 0.45),
                ("mod_cutoff_env", signed(0.3)),
                ("decay", 0.4),
                ("sustain", 0.4),
            ],
        ),
        (
            "Fat saw lead",
            Category::Lead,
            &[
                ("vco2", 0.9),
                ("vco2tune", 0.55),
                ("cutoff", 0.65),
                ("resonance", 0.15),
                ("mod_cutoff_env", signed(0.2)),
                ("decay", 0.5),
                ("sustain", 0.8),
                ("mod_pitch_lfo", signed(0.05)),
                ("lfodelay", 0.5),
                ("portamento", 0.15),
            ],
        ),
        (
            "Screamer",
            Category::Lead,
            &[
                ("cutoff", 0.5),
                ("resonance", 0.8),
                ("mod_cutoff_env", signed(0.5)),
                ("decay", 0.45),
                ("sustain", 0.6),
                ("mod_cutoff_key", signed(0.5)),
                ("vco2", 0.7),
                ("vco2wave", 0.6667),
                ("vco2range", 0.75),
            ],
        ),
        (
            "Whistle",
            Category::Lead,
            &[
                ("vco1wave", 0.0),
                ("vco1range", 0.75),
                ("cutoff", 0.8),
                ("attack", 0.25),
                ("release", 0.4),
                ("mod_pitch_lfo", signed(0.1)),
                ("lfodelay", 0.55),
                ("lforate", 0.55),
            ],
        ),
        (
            "Pulse lead",
            Category::Lead,
            &[
                ("vco1wave", 1.0),
                ("mod_width_lfowidthon", 1.0),
                ("mod_width_lfowidth", signed(0.5)),
                ("lforate", 0.3),
                ("cutoff", 0.7),
                ("resonance", 0.2),
                ("sustain", 0.9),
                ("release", 0.35),
            ],
        ),
        (
            "Octave lead",
            Category::Lead,
            &[
                ("vco2", 1.0),
                ("vco2range", 0.75),
                ("cutoff", 0.7),
                ("resonance", 0.25),
                ("mod_cutoff_env", signed(0.25)),
                ("decay", 0.45),
                ("sustain", 0.7),
                ("portamento", 0.2),
            ],
        ),
        (
            "Glide brass",
            Category::Brass,
            &[
                ("vco2", 0.8),
                ("vco2tune", 0.52),
                ("cutoff", 0.45),
                ("mod_cutoff_env", signed(0.5)),
                ("attack", 0.3),
                ("decay", 0.45),
                ("sustain", 0.55),
                ("release", 0.35),
                ("portamento", 0.3),
                ("resonance", 0.1),
            ],
        ),
        (
            "Soft horn",
            Category::Brass,
            &[
                ("vco1wave", 0.6667),
                ("sub", 0.3),
                ("cutoff", 0.4),
                ("mod_cutoff_env", signed(0.35)),
                ("attack", 0.35),
                ("decay", 0.5),
                ("sustain", 0.7),
                ("release", 0.4),
                ("lfodelay", 0.5),
                ("mod_pitch_lfo", signed(0.05)),
            ],
        ),
        (
            "Warm pad",
            Category::Pad,
            &[
                ("vco2", 1.0),
                ("vco2tune", 0.54),
                ("cutoff", 0.5),
                ("attack", 0.55),
                ("decay", 0.6),
                ("sustain", 0.9),
                ("release", 0.6),
                ("resonance", 0.1),
                ("mod_pitch_lfo", signed(0.04)),
                ("lforate", 0.3),
            ],
        ),
        (
            "Square pad",
            Category::Pad,
            &[
                ("vco1wave", 0.6667),
                ("vco2wave", 0.6667),
                ("vco2", 0.9),
                ("vco2tune", 0.46),
                ("cutoff", 0.45),
                ("attack", 0.5),
                ("release", 0.6),
                ("sustain", 0.85),
                ("lfodelay", 0.4),
                ("mod_pitch_lfo", signed(0.06)),
            ],
        ),
        (
            "Sweep pad",
            Category::Pad,
            &[
                ("vco2", 1.0),
                ("vco2tune", 0.53),
                ("cutoff", 0.25),
                ("mod_cutoff_env", signed(0.6)),
                ("attack", 0.7),
                ("decay", 0.7),
                ("sustain", 0.7),
                ("release", 0.65),
                ("resonance", 0.35),
            ],
        ),
        (
            "Breathing pad",
            Category::Pad,
            &[
                ("vco2", 0.9),
                ("vco2tune", 0.52),
                ("cutoff", 0.4),
                ("mod_cutoff_lfo", signed(0.3)),
                ("lforate", 0.2),
                ("attack", 0.6),
                ("sustain", 1.0),
                ("release", 0.6),
            ],
        ),
        (
            "Choir",
            Category::Pad,
            &[
                ("vco1wave", 1.0),
                ("mod_width_lfowidthon", 1.0),
                ("mod_width_lfowidth", signed(0.4)),
                ("lforate", 0.25),
                ("vco2", 0.8),
                ("vco2wave", 1.0),
                ("vco2tune", 0.55),
                ("cutoff", 0.5),
                ("attack", 0.55),
                ("sustain", 0.9),
                ("release", 0.6),
            ],
        ),
        (
            "Electric piano",
            Category::Keys,
            &[
                ("vco1wave", 0.0),
                ("sub", 0.4),
                ("cutoff", 0.6),
                ("mod_cutoff_env", signed(0.3)),
                ("attack", 0.0),
                ("decay", 0.55),
                ("sustain", 0.3),
                ("release", 0.35),
            ],
        ),
        (
            "Clav",
            Category::Keys,
            &[
                ("vco1wave", 1.0),
                ("pulsewidth", 0.3),
                ("cutoff", 0.55),
                ("resonance", 0.45),
                ("mod_cutoff_env", signed(0.5)),
                ("decay", 0.35),
                ("sustain", 0.15),
                ("release", 0.25),
                ("mod_cutoff_key", signed(0.6)),
            ],
        ),
        (
            "Bell",
            Category::Keys,
            &[
                ("vco1wave", 0.0),
                ("vco2", 0.6),
                ("vco2wave", 0.6667),
                ("vco2range", 1.0),
                ("vco2tunerange", 1.0),
                ("vco2tune", 0.62),
                ("cutoff", 0.75),
                ("attack", 0.0),
                ("decay", 0.65),
                ("sustain", 0.0),
                ("release", 0.6),
            ],
        ),
        (
            "Marimba",
            Category::Percussion,
            &[
                ("vco1wave", 0.0),
                ("vco1range", 0.75),
                ("cutoff", 0.7),
                ("mod_cutoff_env", signed(0.4)),
                ("attack", 0.0),
                ("decay", 0.3),
                ("sustain", 0.0),
                ("release", 0.25),
            ],
        ),
        (
            "Kick",
            Category::Percussion,
            &[
                ("vco1wave", 0.0),
                ("vco1range", 0.0),
                ("mod_pitch_autobend", signed(0.6)),
                ("cutoff", 0.35),
                ("mod_cutoff_env", signed(0.5)),
                ("decay", 0.3),
                ("sustain", 0.0),
                ("release", 0.2),
            ],
        ),
        (
            "Noise hat",
            Category::Percussion,
            &[
                ("vco1", 0.0),
                ("vco2", 1.0),
                ("vco2wave", 0.0),
                ("cutoff", 0.85),
                ("resonance", 0.3),
                ("attack", 0.0),
                ("decay", 0.15),
                ("sustain", 0.0),
                ("release", 0.1),
            ],
        ),
        (
            "Snare",
            Category::Percussion,
            &[
                ("vco1", 0.4),
                ("vco1wave", 0.0),
                ("vco1range", 0.25),
                ("vco2", 1.0),
                ("vco2wave", 0.0),
                ("cutoff", 0.6),
                ("mod_cutoff_env", signed(0.4)),
                ("decay", 0.25),
                ("sustain", 0.0),
                ("release", 0.15),
                ("mod_pitch_autobend", signed(0.3)),
            ],
        ),
        (
            "Zap",
            Category::Fx,
            &[
                ("mod_pitch_autobend", signed(1.0)),
                ("vco1range", 0.75),
                ("cutoff", 0.6),
                ("mod_cutoff_env", signed(0.6)),
                ("decay", 0.2),
                ("sustain", 0.0),
                ("resonance", 0.6),
            ],
        ),
        (
            "Siren",
            Category::Fx,
            &[
                ("mod_pitch_lfo", signed(0.6)),
                ("lforate", 0.25),
                ("lfomode", 0.0),
                ("cutoff", 0.7),
                ("vco1wave", 0.6667),
            ],
        ),
        (
            "Random arp",
            Category::Sequence,
            &[
                ("lfomode", 1.0),
                ("lforate", 0.5),
                ("mod_pitch_lfo", signed(0.4)),
                ("trigger", 1.0),
                ("cutoff", 0.5),
                ("mod_cutoff_env", signed(0.5)),
                ("decay", 0.3),
                ("sustain", 0.0),
            ],
        ),
        (
            "Pulse gate",
            Category::Sequence,
            &[
                ("trigger", 1.0),
                ("lfomode", 0.5),
                ("lforate", 0.55),
                ("vcamode", 1.0),
                ("cutoff", 0.55),
                ("mod_cutoff_env", signed(0.4)),
                ("decay", 0.3),
                ("sustain", 0.2),
                ("vco2", 0.8),
                ("vco2tune", 0.53),
            ],
        ),
        (
            "Beating drone",
            Category::Drone,
            &[
                ("vcamode", 0.0),
                ("cutoff", 0.35),
                ("resonance", 0.4),
                ("vco2", 0.7),
                ("vco2tune", 0.51),
                ("sub", 0.5),
            ],
        ),
        (
            "Low drone",
            Category::Drone,
            &[
                ("vcamode", 0.0),
                ("vco1range", 0.0),
                ("sub", 0.8),
                ("vco2", 0.6),
                ("vco2range", 0.0),
                ("vco2tune", 0.53),
                ("cutoff", 0.3),
                ("mod_cutoff_lfo", signed(0.2)),
                ("lforate", 0.1),
            ],
        ),
    ];

    /// One designed sound: its name, its category, and the values that make it.
    type Design = (&'static str, Category, &'static [(&'static str, f32)]);

    /// Writes the twenty factory presets to `plugins/mxm-mono-02/presets/`.
    ///
    /// A facility, not a test — and the *only* thing that writes those files, so the numbers in
    /// `FACTORY_DESIGN` stay the readable statement of each sound and the JSON stays generated
    /// output. `every_factory_preset_covers_every_parameter` is what catches a file that has fallen
    /// behind a new parameter.
    ///
    /// ```text
    /// cargo test -p mxm-mono-02 --lib write_the_factory_presets -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "writes the factory preset files"]
    fn write_the_factory_presets() {
        let params = params();

        for (name, category, overrides) in FACTORY_DESIGN {
            let preset = generated(&params, name, *category, overrides);

            // From the manifest directory, not the working one: a test's cwd is the crate root
            // and not the workspace root, which is the sort of thing that only says so once.
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("presets")
                .join(format!("{}.json", name.to_lowercase().replace(' ', "-")));
            std::fs::write(&file, preset.to_json()).expect("write the preset");
            eprintln!("wrote {}", file.display());
        }
    }

    #[test]
    fn every_designed_preset_names_real_parameters() {
        // Runs by default, unlike the generator: a typo in `FACTORY_DESIGN` would otherwise only
        // surface the next time somebody regenerated the files, and silently leave that value at
        // its default in the meantime.
        let params = params();
        let bindings = mxm_preset::Instrument::parameters(&params);
        for (name, _category, overrides) in FACTORY_DESIGN {
            for (id, v) in *overrides {
                assert!(
                    bindings.iter().any(|(bound, _)| *bound == *id),
                    "{name:?} names `{id}`, which is not a parameter of this instrument"
                );
                assert!(
                    (0.0..=1.0).contains(v),
                    "{name:?} sets `{id}` to {v}, which is not a normalised value"
                );
            }
        }
    }

    /// What the generator would write for one design, in memory.
    fn generated(
        params: &MxmMono02Params,
        name: &str,
        category: Category,
        overrides: &[(&str, f32)],
    ) -> Preset {
        let bindings = mxm_preset::Instrument::parameters(params);
        let mut preset = Preset::init(params);
        preset.name = name.to_owned();
        preset.category = category;
        for (id, v) in overrides {
            let (_, param) = bindings
                .iter()
                .find(|(bound, _)| *bound == *id)
                .unwrap_or_else(|| panic!("{name:?} names `{id}`, which is not a parameter"));
            preset.params.insert(
                (*id).to_owned(),
                Value {
                    v: *v,
                    text: param.format(*v),
                },
            );
        }
        preset
    }

    #[test]
    fn the_factory_files_match_the_design_they_were_generated_from() {
        // The generator is `#[ignore]`d, so nothing forces it to have been run. This is what says
        // the shipped files are the current design rather than a stale one — the same class of
        // mistake as a stale `.clap` bundle, and just as quiet.
        //
        // **The whole preset, `text` included**, not only the overridden values. The first version
        // compared the overrides alone and let files generated before a formatter changed ship
        // with `"5.4 kHz Hz"` in them: never read, but exactly the drift this test exists to see.
        let params = params();
        for (name, category, overrides) in FACTORY_DESIGN {
            let (_, text) = FACTORY_FILES
                .iter()
                .find(|(file_name, _)| file_name == name)
                .unwrap_or_else(|| panic!("no factory file for {name:?}"));
            let shipped = Preset::parse(text, crate::CLAP_ID).expect("parses");
            let expected = generated(&params, name, *category, overrides);
            assert_eq!(
                shipped, expected,
                "{name:?} on disk is not what the design generates — regenerate the files"
            );
        }
    }

    #[test]
    fn the_user_root_is_under_this_instruments_own_id() {
        // Namespaced by CLAP id so another instrument's presets cannot appear in this one's list.
        let Some(root) = user_root(crate::CLAP_ID) else {
            return;
        };
        assert!(root.ends_with("presets"));
        assert!(root.to_string_lossy().contains(crate::CLAP_ID));
    }

    #[test]
    fn every_factory_preset_can_be_heard() {
        // Audibility here is a source, the volume and the filter. A preset with every source
        // off, or the output at nothing, or the filter shut with no envelope to open it, loads
        // without complaint and reads as the instrument being broken.
        for (name, text) in FACTORY_FILES.iter() {
            let preset = Preset::parse(text, crate::CLAP_ID).expect("parses");
            let v = |id: &str| preset.params.get(id).map_or(0.0, |value| value.v);
            assert!(
                v("vco1") > 0.05 || v("vco2") > 0.05 || v("sub") > 0.05,
                "{name:?} has every source off"
            );
            assert!(v("volume") > 0.05, "{name:?} is turned down to nothing");
            assert!(
                v("cutoff") > 0.05 || v("mod_cutoff_env") > signed(0.2),
                "{name:?} has the filter shut and nothing to open it"
            );
        }
    }

    #[test]
    fn no_two_factory_presets_are_the_same_sound() {
        // Twenty is enough that a copied-and-edited design could lose its edit unnoticed.
        for (index, (name, text)) in FACTORY_FILES.iter().enumerate() {
            let a = Preset::parse(text, crate::CLAP_ID).expect("parses");
            for (other, text) in &FACTORY_FILES[index + 1..] {
                let b = Preset::parse(text, crate::CLAP_ID).expect("parses");
                assert_ne!(a.params, b.params, "{name:?} and {other:?} are identical");
            }
        }
    }

    #[test]
    fn every_factory_preset_has_a_category() {
        // A sound is saved with its category (the owner's rule, 2026-09-04), and the factory set
        // is where a person first sees what the categories mean. *Uncategorised* is for files
        // written before the field existed, not for sounds this instrument ships.
        for (name, text) in FACTORY_FILES.iter() {
            let preset = Preset::parse(text, crate::CLAP_ID).expect("parses");
            assert_ne!(
                preset.category,
                Category::Uncategorised,
                "factory preset {name:?} has no category"
            );
        }
    }

    #[test]
    fn every_factory_preset_parses_and_is_for_this_instrument() {
        // A malformed factory preset is a build mistake, not a user's, so it is caught here rather
        // than skipped quietly in the browser.
        for (name, text) in FACTORY_FILES.iter() {
            let preset = Preset::parse(text, crate::CLAP_ID)
                .unwrap_or_else(|e| panic!("factory preset {name:?} does not parse: {e}"));
            assert_eq!(preset.name, *name, "the file's name must match its listing");
        }
    }

    #[test]
    fn every_factory_preset_covers_every_parameter() {
        // The one that catches a factory preset written before a parameter existed: it would load
        // and quietly leave that parameter wherever the last patch left it.
        let params = params();
        for (name, text) in FACTORY_FILES.iter() {
            let preset = Preset::parse(text, crate::CLAP_ID).expect("parses");
            let (_, problems) = preset.resolve(&params);
            assert!(
                problems.is_empty(),
                "factory preset {name:?} is incomplete: {problems:?}"
            );
        }
    }

    #[test]
    fn the_factory_list_begins_with_init() {
        let params = params();
        let all = factory(&params);
        assert_eq!(all[0].name, INIT_NAME);
        assert_eq!(all.len(), FACTORY_FILES.len() + 1);
    }

    #[test]
    fn no_factory_preset_sets_the_master_volume() {
        // Volume is an ordinary parameter under the shared preset contract — saved, restored,
        // compared — and what keeps it out of the *sound* of a preset is content, not mechanism:
        // no factory sound moves it. The patch's Level is part of the sound; the master is the
        // person's.
        let params = params();
        let default = params.volume.default_normalized_value();
        for (name, text) in FACTORY_FILES.iter() {
            let preset = Preset::parse(text, crate::CLAP_ID).expect("parses");
            let v = preset.params.get("volume").map_or(default, |value| value.v);
            assert!(
                (v - default).abs() < 1e-6,
                "{name:?} sets the master volume, which no factory preset does"
            );
        }
    }
}
