//! One tree per card.
//!
//! The brief owns what goes where; this file implements it. §10's section order is the contract, and
//! [`super::SECTIONS`] is written in that order so a reordering is a visible diff rather than a
//! drift.
//!
//! A card's body is described once, as a `mxm_ui::tree` ([`card`]), and that one description is
//! both measured — the card's floor and its height, by the paging renderer — and drawn, leaf by
//! leaf, through the bindings ([`paint`]). Nothing is typed and nothing is drawn to learn a size
//! (`plans/plan-layout-tree.md`).

use std::collections::HashMap;

use egui::{Rect, Ui};
use mxm_mono_02_dsp::routing::{TARGET_NAMES, target};
use mxm_ui::control::{Size, Wave};
use mxm_ui::space::SPACE_3;
use mxm_ui::theme::Tokens;
use mxm_ui::tree::{self, Anchor, Height, Kind, Node, leaf, pad, pad_all, row_gap, stack};
use nice_plug::prelude::ParamSetter;

use super::binding::{Bound, segmented_named, segmented_waves_named, toggle_labelled};
use super::{Section, visuals};
use crate::params::MxmMono02Params;
use crate::telemetry::Telemetry;

/// §7.1's sizing, as used here. Brief §2 puts Cutoff and Resonance at **Primary**; everything else
/// is Standard, and the disclosed bend range is Compact.
const PRIMARY: Size = Size::Primary;
const STANDARD: Size = Size::Standard;
const COMPACT: Size = Size::Compact;

/// The Voice card's expander: its title, which is also where its open state is kept
/// ([`mxm_ui::shell::disclosure_id`], in the editor's egui memory — the developer channel's CC 118
/// writes it there too, through `editor::set_bender`).
pub const BENDER: &str = "Bender";

/// What the Bender's switch says it holds (§7.1's sentence).
const BENDER_DESCRIPTION: &str = "The bend range.";

/// A switch's cells, **labelled by the parameter itself**: each is its option's own formatted value,
/// so a cell reads what the host's automation list reads.
fn switch_options(params: &MxmMono02Params, id: &'static str) -> Vec<String> {
    let param = binding_for(id, params).param;
    let last = param
        .steps()
        .unwrap_or_else(|| panic!("`{id}` is not drawn as a switch"));
    (0..=last)
        .map(|option| param.format(option as f32 / last as f32))
        .collect()
}

/// What a control **paints**, where its card already says the rest (design system §7.1): *Rate*,
/// *Delay* and *Shape* on the LFO. `None` paints the parameter's own name, which is what a
/// host, a tooltip and a screen reader always read. Each oscillator has its own card, so its
/// controls drop the *VCO-1 …* and *VCO-2 …* prefixes the host needs, and VCO-2's tune reads *Tune*
/// beside VCO-1's *Master tune* (the owner, 2026-09-27). VCO-1's bender switch paints *Bender*:
/// with its prefix the row of seven cards passes the quarter-4K budget and the editor opens on two
/// pages. What a painted name replaces is only the painting: the host, the hover text and the
/// accessibility tree still read *VCO-1 bender*, *VCO-1 range* and the rest.
fn panel_label(id: &str) -> Option<&'static str> {
    match id {
        "vco1range" | "vco2range" => Some("Range"),
        "vco1wave" | "vco2wave" => Some("Wave"),
        "vco1bender" => Some("Bender"),
        "vco2tune" => Some("Tune"),
        "vco2tunerange" => Some("Tune range"),
        "lforate" => Some("Rate"),
        "lfodelay" => Some("Delay"),
        "lfomode" => Some("Shape"),
        // The envelope's faders, by the convention (the owner, 2026-09-25).
        "attack" => Some("A"),
        "decay" => Some("D"),
        "sustain" => Some("S"),
        "release" => Some("R"),
        _ => None,
    }
}

/// A waveform selector's pictures, drawn rather than spelled: the collection's shared glyphs. The
/// names are the hover text and the accessible name.
fn waves_of(id: &str) -> &'static [(Wave, &'static str)] {
    match id {
        "lfomode" => &[
            (Wave::Sine, "Sine"),
            (Wave::Square, "Square"),
            (Wave::Random, "Random"),
        ],
        "vco1wave" => &[
            (Wave::Sine, "Sine"),
            (Wave::RampUp, "Sawtooth"),
            (Wave::Square, "Square"),
            (Wave::Pulse, "Pulse"),
        ],
        "vco2wave" => &[
            (Wave::Noise, "Noise"),
            (Wave::RampUp, "Sawtooth"),
            (Wave::Square, "Square"),
            (Wave::Pulse, "Pulse"),
        ],
        other => panic!("`{other}` has no pictures"),
    }
}

// --------------------------------------------------------------------------------------------
// The cards, as trees (plans/plan-layout-tree.md). Each card is described once — `card` — and that
// one description is both measured (its floor and its height) and drawn, leaf by leaf, through the
// bindings (`paint`). The gaps are the hand layout's own: a card body's `SPACE_3` rhythm between
// siblings, and whatever `add_space` it put on top of that, as a pad.
// --------------------------------------------------------------------------------------------

/// What a leaf of this editor's cards draws. Hashed by what it names, which is also what keeps its
/// widget ids stable when a route appears above it.
#[derive(Clone, Debug, Hash)]
pub enum Leaf {
    Knob(&'static str, Size),
    /// A fader in the collection's fader row.
    Fader(&'static str),
    /// A stepped parameter as a segmented control.
    Switch(&'static str),
    /// A waveform selector, on the grid of the knob it shares a row with, if any.
    Waves(&'static str, Option<Size>),
    /// A boolean as one button with two states.
    Toggle(&'static str),
    /// A control's tempo sync, the quarter note beside it.
    Picture(&'static str),
    /// A target's route stack, by target.
    Routes(usize),
    MixerScope,
    FilterResponse,
}

/// Knobs in **columns of their own width**, left to right — `mxm-mono-03`'s solution to two knobs
/// sitting far apart in a card that was mostly gap: `ui.columns` inside a width capped at
/// `Σ max(diameter + SPACE_5, 78)`, as data. The columns take what they are offered up to that cap,
/// and shrink below it only as far as the widest knob allows.
/// Levels as the collection's fader row (`tree::fader_row`): the mixer's three sources, the
/// envelope's A, D, S and R.
fn faders(ui: &Ui, params: &MxmMono02Params, ids: &[&'static str]) -> Node<Leaf> {
    mxm_ui::tree::fader_row(
        ui,
        ids.iter()
            .map(|&id| {
                let bound = binding_for(id, params);
                mxm_ui::tree::fader(
                    Leaf::Fader(id),
                    bound.painted(),
                    mxm_ui::control::widest_value(|n| bound.param.format(n as f32)),
                )
            })
            .collect(),
    )
}

fn knobs(ui: &Ui, params: &MxmMono02Params, knobs: &[(&'static str, Size)]) -> Node<Leaf> {
    mxm_ui::tree::knob_row(
        ui,
        knobs
            .iter()
            .map(|(id, size)| {
                (*size, {
                    let bound = binding_for(id, params);
                    let param = bound.param;
                    // A syncable control's column holds its free readings and its divisions.
                    let widest = if *id == "lforate" {
                        super::binding::synced_widest(param, crate::params::LFO_SYNC.span)
                    } else {
                        mxm_ui::control::widest_value(|n| param.format(n as f32))
                    };
                    leaf(
                        Leaf::Knob(id, *size),
                        Kind::Knob {
                            name: bound.painted().to_owned(),
                            widest,
                            size: *size,
                            column: 0.0,
                        },
                    )
                })
            })
            .collect(),
    )
}

/// A stepped parameter as a segmented control, each cell as wide as the longest option and no
/// wider (design system §7.3).
fn switch(params: &MxmMono02Params, id: &'static str) -> Node<Leaf> {
    leaf(
        Leaf::Switch(id),
        Kind::Segmented {
            label: binding_for(id, params).painted().to_owned(),
            options: switch_options(params, id),
            beside: None,
        },
    )
}

/// A waveform selector. `beside` is the knob it shares a top-aligned row with, if any; the control
/// then sits on that knob's grid.
fn waves(params: &MxmMono02Params, id: &'static str, beside: Option<Size>) -> Node<Leaf> {
    leaf(
        Leaf::Waves(id, beside),
        Kind::Waves {
            label: Some(binding_for(id, params).painted().to_owned()),
            count: waves_of(id).len(),
            marks: Vec::new(),
            beside,
        },
    )
}

/// One of §8's displays: it fills the card's width at a fixed height, and has no minimum width of
/// its own.
fn display(key: Leaf, height: f32) -> Node<Leaf> {
    leaf(
        key,
        Kind::Custom {
            min_width: 0.0,
            height: Height::Fixed(height),
            fills: true,
        },
    )
}

/// A target's routes, `SPACE_3` below what precedes it. A route stack is a composite with a rule of
/// its own — its floor is every route revealed — so it states its size (`stack_size`).
fn routes_leaf(ui: &Ui, params: &MxmMono02Params, t: usize) -> Node<Leaf> {
    let size = mxm_modulation_params::ui::stack_size(
        ui,
        TARGET_NAMES[t],
        &params.routes.each()[t].routes(t),
    );
    pad(
        SPACE_3,
        leaf(
            Leaf::Routes(t),
            Kind::Custom {
                min_width: size.x,
                height: Height::Fixed(size.y),
                fills: true,
            },
        ),
    )
}

/// One section's body, as a tree, from the parameters and the one editor fact that changes what a
/// card shows: whether the Voice card's Bender is open, read from egui's memory. The card
/// reserves it open, so opening it never grows the card; what is under it moves down.
pub fn card(ui: &Ui, section: Section, params: &MxmMono02Params) -> Node<Leaf> {
    let gap = ui.spacing().item_spacing.x;
    match section {
        // One LFO, three shapes, a delay that fades the sine only. The rate's tempo sync is the
        // quarter note beside it (`plans/plan-tempo-sync-controls.md`); the shapes stand on the
        // knobs' grid beside them.
        Section::Lfo => stack(vec![row_gap(
            gap,
            vec![
                knobs(ui, params, &[("lforate", STANDARD)]),
                tree::switch_beside_knob(
                    STANDARD,
                    leaf(Leaf::Picture("lfosync"), Kind::SyncToggle),
                ),
                knobs(ui, params, &[("lfodelay", STANDARD)]),
                pad_all(0.0, SPACE_3, 0.0, waves(params, "lfomode", Some(STANDARD))),
            ],
        )]),
        // Two VCOs, a card each (the owner, 2026-09-27: one card for both was absurdly tall). A
        // five-way range switch needs 164 px at the minimum target and a four-wave picker about 200
        // px, so neither shares a row with the other; each range switch shares its row with a small
        // switch instead.
        //
        // VCO-1: range with the bender switch beside it, dropped onto the range's cell line, the
        // waveform, then the master tune and the Pitch routes beneath it — both move both
        // oscillators; the machine's vibrato and auto bend are among the routes, at zero.
        Section::Oscillator1 => stack(vec![
            row_gap(
                gap,
                vec![
                    switch(params, "vco1range"),
                    pad_all(
                        0.0,
                        SPACE_3,
                        0.0,
                        Node::Beside(Anchor::CellLine, Box::new(toggle(params, "vco1bender"))),
                    ),
                ],
            ),
            pad(SPACE_3, waves(params, "vco1wave", None)),
            pad(SPACE_3, knobs(ui, params, &[("tune", STANDARD)])),
            routes_leaf(ui, params, target::PITCH),
        ]),
        // VCO-2: range with its tune range beside it, the waveform, then its tune beside the pulse
        // width both pulses share, and the Pulse width routes beneath — the owner's balanced split
        // (2026-09-27). The section's LFO and envelope positions, which were a source switch and a
        // depth, are two routes offered under ‹ modulate ›, not rows at Init.
        Section::Oscillator2 => stack(vec![
            row_gap(
                gap,
                vec![
                    switch(params, "vco2range"),
                    pad_all(0.0, SPACE_3, 0.0, switch(params, "vco2tunerange")),
                ],
            ),
            pad(SPACE_3, waves(params, "vco2wave", None)),
            pad(
                SPACE_3,
                knobs(
                    ui,
                    params,
                    &[("vco2tune", STANDARD), ("pulsewidth", STANDARD)],
                ),
            ),
            routes_leaf(ui, params, target::PULSE_WIDTH),
        ]),
        // Sub, VCO-1, VCO-2. The mix, as a trace: the owner asked to see what the mixer puts out.
        Section::Mixer => stack(vec![
            display(Leaf::MixerScope, visuals::SCOPE_HEIGHT),
            pad(SPACE_3, faders(ui, params, &["sub", "vco1", "vco2"])),
        ]),
        // Where the playing happens. The two Primary knobs, and the Cutoff routes beneath them: the
        // envelope in either polarity, the modulator, keyboard tracking and the bender, all at zero
        // at Init.
        Section::Filter => stack(vec![
            display(Leaf::FilterResponse, visuals::HEIGHT),
            pad(
                SPACE_3,
                knobs(ui, params, &[("cutoff", PRIMARY), ("resonance", PRIMARY)]),
            ),
            routes_leaf(ui, params, target::CUTOFF),
        ]),
        // The one envelope and its trigger switch, then the amplifier it drives: HOLD, ENV or GATE,
        // and the Amplitude routes. One card since the owner merged mxm-mono-01's (2026-09-24):
        // hugged, the Amplifier was one switch and a stack. The volume is in the app bar (§3.1).
        Section::Envelope => stack(vec![
            faders(ui, params, &["attack", "decay", "sustain", "release"]),
            pad(SPACE_3, switch(params, "trigger")),
            pad(SPACE_3, switch(params, "vcamode")),
            routes_leaf(ui, params, target::AMPLITUDE),
        ]),
        // Portamento, and the bend range, disclosed. No card carries a caption (the owner,
        // 2026-09-27: no help text on the panel); a control's sentence is its tooltip.
        Section::Voice => stack(vec![
            knobs(ui, params, &[("portamento", STANDARD)]),
            pad(
                SPACE_3,
                // The collection's disclosure, as mxm-mono-00's and mxm-mono-01's: the card
                // reserves its body open, so opening it never moves the card's width or height.
                tree::disclosure(
                    ui.ctx(),
                    BENDER,
                    BENDER_DESCRIPTION,
                    knobs(ui, params, &[("bendrange", COMPACT)]),
                ),
            ),
        ]),
    }
}

/// A boolean as one button with two states, carrying the modulation mark.
fn toggle(params: &MxmMono02Params, id: &'static str) -> Node<Leaf> {
    leaf(
        Leaf::Toggle(id),
        Kind::Toggle {
            label: binding_for(id, params).painted().to_owned(),
        },
    )
}

/// Everything a leaf draws with: the parameters and their host, the telemetry the displays read
/// (none of it destructive), and the text-entry buffers.
pub struct Live<'a, 'b> {
    pub params: &'a MxmMono02Params,
    pub telemetry: &'a Telemetry,
    pub setter: &'a ParamSetter<'b>,
    pub entries: &'a mut HashMap<&'static str, Option<String>>,
}

/// Draws one leaf, in the `Ui` the tree bounded to `rect`, through the bindings — so the controls,
/// their gestures and their names are exactly what they were.
pub fn paint(ui: &mut Ui, tokens: &Tokens, leaf: &Leaf, rect: Rect, live: &mut Live<'_, '_>) {
    let params = live.params;
    let setter = live.setter;
    match *leaf {
        // Inside its column this **is** the column's width.
        // Synced to a tempo, the LFO rate reads its division; the host still reads its hertz.
        Leaf::Knob(id, size) => {
            let bound = binding_for(id, params);
            let division = (id == "lforate" && params.lfo_sync.value())
                .then(|| {
                    use nice_plug::prelude::Param as _;
                    let rate = &params.lfo_rate;
                    crate::params::LFO_SYNC.shown(
                        rate.unmodulated_normalized_value(),
                        live.telemetry.tempo.get(),
                        f64::from(rate.preview_plain(0.0)),
                        f64::from(rate.preview_plain(1.0)),
                    )
                })
                .flatten();
            match division {
                Some(division) => bound.knob_with_reading(
                    ui,
                    tokens,
                    setter,
                    size,
                    rect.width(),
                    live.entries,
                    division.label(),
                ),
                None => bound.knob(ui, tokens, setter, size, rect.width(), live.entries),
            }
        }
        Leaf::Picture(id) => {
            super::binding::sync_picture(ui, tokens, id, binding_for(id, params).param, setter);
        }
        Leaf::Fader(id) => {
            let bound = binding_for(id, params);
            bound.slider_vertical(
                ui,
                tokens,
                setter,
                live.entries,
                bound.painted(),
                rect.width(),
                mxm_ui::control::FADER_HEIGHT,
            );
        }
        Leaf::Switch(id) => {
            let bound = binding_for(id, params);
            let labels = switch_options(params, id);
            let options: Vec<&str> = labels.iter().map(String::as_str).collect();
            segmented_named(
                ui,
                tokens,
                bound.id,
                bound.param,
                bound.panel.as_deref(),
                &options,
                bound.details,
                setter,
                0.0,
            );
        }
        Leaf::Waves(id, beside) => {
            let bound = binding_for(id, params);
            segmented_waves_named(
                ui,
                tokens,
                bound.id,
                bound.param,
                bound.panel.as_deref(),
                waves_of(id),
                beside,
                bound.details,
                setter,
            );
        }
        Leaf::Toggle(id) => {
            let bound = binding_for(id, params);
            toggle_labelled(
                ui,
                tokens,
                id,
                bound.param,
                bound.painted(),
                bound.description,
                setter,
                0.0,
            );
        }
        Leaf::Routes(t) => routes(ui, tokens, t, params, setter, live.entries),
        Leaf::MixerScope => {
            let mut samples = Vec::with_capacity(crate::telemetry::SCOPE_LEN);
            live.telemetry.scope_snapshot(&mut samples);
            visuals::mixer_scope(ui, tokens, &samples, visuals::SCOPE_HEIGHT);
        }
        Leaf::FilterResponse => visuals::filter_response(
            ui,
            tokens,
            set_cutoff_hz(params),
            live.telemetry.cutoff_hz().max(1.0),
            params.resonance.value(),
            visuals::HEIGHT,
        ),
    }
}

/// Draws one section's body: its tree, shown in `ui`. The layout lab (`apps/mxm-layout-lab`, in the
/// private archive since the split) draws these real cards through this.
pub fn draw(
    ui: &mut Ui,
    tokens: &Tokens,
    section: Section,
    params: &MxmMono02Params,
    telemetry: &Telemetry,
    setter: &ParamSetter<'_>,
    text_entry: &mut HashMap<&'static str, Option<String>>,
) {
    let tree = card(ui, section, params);
    let mut live = Live {
        params,
        telemetry,
        setter,
        entries: text_entry,
    };
    tree::show(ui, tokens, &tree, |ui, leaf, rect| {
        paint(ui, tokens, leaf, rect, &mut live);
    });
}

/// A target's routes, drawn beneath the control they move: the live ones stacked, `‹ modulate ›`
/// under them (`mxm_modulation_params::ui::stack`). **Routing belongs under the thing it affects**,
/// never in a detached footer. No target's name carries its card's title, so the painted and the
/// canonical name are one string.
fn routes(
    ui: &mut Ui,
    tokens: &Tokens,
    t: usize,
    params: &MxmMono02Params,
    setter: &ParamSetter<'_>,
    text_entry: &mut HashMap<&'static str, Option<String>>,
) {
    let group = params.routes.each()[t];
    let entry = text_entry.entry("routes").or_default();
    mxm_modulation_params::ui::stack(
        ui,
        tokens,
        TARGET_NAMES[t],
        TARGET_NAMES[t],
        &group.routes(t),
        entry,
        setter,
    );
}

// --------------------------------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------------------------------

/// Where the slider alone puts the cutoff, in hertz — the display's declared approximation.
fn set_cutoff_hz(params: &MxmMono02Params) -> f32 {
    use mxm_mono_02_dsp::voice::{CUTOFF_FLOOR_HZ, CUTOFF_RANGE_OCTAVES};
    CUTOFF_FLOOR_HZ * (params.cutoff.value() * CUTOFF_RANGE_OCTAVES).exp2()
}

/// One parameter's binding, with the sentence §7.1 requires in its tooltip.
/// How the keyboard steps a parameter: a semitone and an octave for the bend reach,
/// a cent and ten for the tune,
/// the owner's ruling of 2026-09-23. Everything not named keeps its own step. See
/// [`crate::editor::binding::StepLaw`].
fn step_law(id: &str) -> super::binding::StepLaw {
    use super::binding::StepLaw;
    match id {
        "bendrange" => StepLaw::Semitones,
        "tune" => StepLaw::Cents,
        _ => StepLaw::Own,
    }
}

pub fn binding_for<'a>(id: &'static str, p: &'a MxmMono02Params) -> Bound<'a> {
    let (param, description, bipolar): (&'a dyn super::binding::ErasedParam, &'static str, bool) =
        match id {
            "lforate" => (&p.lfo_rate, "How fast the LFO runs.", false),
            "lfosync" => (&p.lfo_sync, super::binding::SYNC_DESCRIPTION, false),
            "lfodelay" => (
                &p.lfo_delay,
                "How long the LFO takes to fade in after a note starts; only the sine fades.",
                false,
            ),
            "lfomode" => (&p.lfo_mode, "The LFO's shape.", false),
            "pulsewidth" => (
                &p.pulse_width,
                "How narrow both pulse waves are: square at the top, thin at the bottom.",
                false,
            ),
            "tune" => (&p.tune, "Tunes both oscillators together, in cents.", true),
            "vco1range" => (&p.vco1_range, "VCO-1's octave.", false),
            "vco1wave" => (&p.vco1_wave, "VCO-1's waveform.", false),
            "vco1bender" => (
                &p.vco1_bender,
                "Whether pitch bend moves VCO-1; off, only VCO-2 bends.",
                false,
            ),
            "vco2range" => (&p.vco2_range, "VCO-2's octave.", false),
            "vco2wave" => (&p.vco2_wave, "VCO-2's waveform, or noise.", false),
            "vco2tune" => (
                &p.vco2_tune,
                "Tunes VCO-2 against VCO-1; the centre is in tune.",
                true,
            ),
            "vco2tunerange" => (
                &p.vco2_tune_range,
                "How far the tune knob reaches: a major second, or an octave and a third.",
                false,
            ),
            "sub" => (
                &p.sub,
                "The sub-oscillator's level: a square one octave under VCO-1.",
                false,
            ),
            "vco1" => (&p.vco1, "VCO-1's level.", false),
            "vco2" => (&p.vco2, "VCO-2's level.", false),
            "cutoff" => (&p.cutoff, "The filter's cutoff: lower is darker.", false),
            "resonance" => (
                &p.resonance,
                "Emphasis at the cutoff; near the top the filter whistles on its own.",
                false,
            ),
            "vcamode" => (&p.vca_mode, "How the volume follows the keys.", false),
            "attack" => (&p.attack, "How long the envelope takes to rise.", false),
            "decay" => (
                &p.decay,
                "How long it takes to fall to the sustain level.",
                false,
            ),
            "sustain" => (&p.sustain, "The level held while a key is down.", false),
            "release" => (
                &p.release,
                "How long the sound takes to fade after the key is released.",
                false,
            ),
            "trigger" => (&p.trigger, "What restarts the envelope.", false),
            "portamento" => (
                &p.portamento,
                "Glide time between notes; zero turns glide off.",
                false,
            ),
            "volume" => (&p.volume, "Output level.", false),
            "bendrange" => (
                &p.bend_range,
                "The bender's reach on pitch, in semitones.",
                false,
            ),
            other => unreachable!("no binding for {other}"),
        };
    Bound {
        id,
        param,
        description,
        panel: panel_label(id).map(std::borrow::Cow::Borrowed),
        bipolar,
        law: step_law(id),
        stepped: None,
        details: details_of(id),
    }
}

/// What each option of a stepped control does, one sentence per cell in the parameter's own order
/// (design system §7.3; the owner, 2026-09-27: the cells of a row do not share one sentence).
/// Empty for everything drawn as a knob, slider or toggle.
fn details_of(id: &str) -> &'static [&'static str] {
    match id {
        "vco1range" | "vco2range" => &[
            "Two octaves below the note played.",
            "One octave below the note played.",
            "The note as played.",
            "One octave above the note played.",
            "Two octaves above the note played.",
        ],
        "vco1wave" => &[
            "A pure, soft tone with no harmonics.",
            "Bright and buzzy: every harmonic.",
            "Hollow: odd harmonics only.",
            "Thin and nasal; Pulse width sets how thin.",
        ],
        "vco2wave" => &[
            "Hiss instead of a pitch: VCO-2 becomes a noise source.",
            "Bright and buzzy: every harmonic.",
            "Hollow: odd harmonics only.",
            "Thin and nasal; Pulse width sets how thin.",
        ],
        "vco2tunerange" => &[
            "Tune reaches a whole tone either way, for detuning.",
            "Tune reaches over an octave either way, for intervals.",
        ],
        "lfomode" => &[
            "A smooth wobble, and the only shape Delay fades in.",
            "Jumps between two values, like a trill.",
            "A new random value on every cycle.",
        ],
        "vcamode" => &[
            "Always open: the sound drones with no key held.",
            "The envelope shapes the volume of each note.",
            "Full volume while a key is held, silent once it is released.",
        ],
        "trigger" => &[
            "Every new note restarts the envelope, even over held keys.",
            "Only the first key starts the envelope; notes played over it glide in.",
            "While a key is held, the LFO restarts the envelope each cycle.",
        ],
        _ => &[],
    }
}

/// Every parameter, bound, in the instrument's own order.
pub fn all_parameters(params: &MxmMono02Params) -> Vec<Bound<'_>> {
    ALL_IDS.iter().map(|id| binding_for(id, params)).collect()
}

/// Every id this editor draws — on the cards, and `volume` in the app bar. One list, so the
/// coverage test and the lookup cannot disagree.
pub const ALL_IDS: &[&str] = &[
    "lforate",
    "lfosync",
    "lfodelay",
    "lfomode",
    "vco1range",
    "vco1wave",
    "vco1bender",
    "vco2range",
    "vco2wave",
    "vco2tune",
    "vco2tunerange",
    "tune",
    "pulsewidth",
    "sub",
    "vco1",
    "vco2",
    "cutoff",
    "resonance",
    "attack",
    "decay",
    "sustain",
    "release",
    "trigger",
    "vcamode",
    "volume",
    "portamento",
    "bendrange",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_drawn_parameter_has_a_binding_with_a_sentence() {
        let params = MxmMono02Params::default();
        for id in ALL_IDS {
            let bound = binding_for(id, &params);
            assert!(
                bound.description.ends_with('.'),
                "{id}: {:?}",
                bound.description
            );
        }
    }
}
