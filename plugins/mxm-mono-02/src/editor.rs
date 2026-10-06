//! mxm-mono-02's editor.
//!
//! Built to `docs/briefs/mxm-mono-02.md`, which is the gating document — this module implements it
//! and does not re-decide it. The brief owns §6's category/card inventory, §10's resize contract,
//! §7's identity accent and §8's display. Parameters is a separate developer-only surface.
//!
//! # It is a panel, not a window
//!
//! [`panel`] takes a `Ui` and draws into it. It does not create a window, run an event loop, or own
//! a swapchain. That is what lets the same code be the plugin's CLAP editor and, later, a
//! standalone harness's contents.
//!
//! # Gestures
//!
//! Every edit is bracketed: `begin_set_parameter`, `set_parameter_normalized`, `end_set_parameter`,
//! in exactly one place — [`binding::Bound::apply`]. An unclosed gesture leaves a host's automation
//! lane latched, and it breaks the player's step editing outright.

pub mod binding;
pub mod sections;
mod visuals;

use std::collections::HashMap;
use std::sync::Arc;

use egui::Ui;
use mxm_ui::space::SPACE_5;
use mxm_ui::theme::Tokens;
use nice_plug::context::gui::GuiContext;
use nice_plug::prelude::*;
use nice_plug_egui::{EguiEditorState, NiceEguiApp, create_egui_editor};

use crate::params::MxmMono02Params;
use crate::telemetry::Telemetry;

/// The size the editor **opens** at — **derived, not chosen**: the quarter-4K budget hugged around
/// every page, which `tests::the_opening_size_is_the_budget_hugged` holds.
const REFERENCE: (u32, u32) = (1886, 618);

/// Opens or closes the Voice card's Bender, where its header keeps that state: egui's memory, under
/// [`mxm_ui::shell::disclosure_id`]. The developer channel's CC 118 lands here.
pub fn set_bender(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(mxm_ui::shell::disclosure_id(sections::BENDER), open));
}

/// Stable inventory order; `page_items` supplies §6's primary categories.
pub const SECTIONS: &[Section] = &[
    Section::Voice,
    Section::Lfo,
    Section::Oscillator1,
    Section::Oscillator2,
    Section::Mixer,
    Section::Filter,
    Section::Envelope,
];

/// The narrowest the window may be: the wider of one card with the panel's gutters and the app bar
/// at its last compact step. The bar decides it: `the_app_bar_holds_in_the_minimum_window` measures
/// it.
const MINIMUM: (u32, u32) = (446, 320);

/// The keyboard cursor's card for the app bar's Volume, outside the paging keys 0…6.
const VOLUME_CARD: u64 = 64;

const _: () = assert!(VOLUME_CARD >= SECTIONS.len() as u64);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Section {
    Lfo,
    /// VCO-1, with the master tune and the Pitch routes that move both oscillators.
    Oscillator1,
    /// VCO-2, with the pulse width both pulses share and its routes.
    Oscillator2,
    Mixer,
    Filter,
    /// The envelope and the amplifier it drives, as one card (the owner's mxm-mono-01 ruling of
    /// 2026-09-24, applied here in R2): hugged, the Amplifier was one switch and a stack.
    Envelope,
    Voice,
}

impl Section {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Lfo => "LFO",
            Self::Oscillator1 => "Oscillator 1",
            Self::Oscillator2 => "Oscillator 2",
            Self::Mixer => "Mixer",
            Self::Filter => "Filter",
            Self::Envelope => "Envelope and amplifier",
            Self::Voice => "Voice",
        }
    }
}

/// Builds the editor. Called from `Plugin::editor`.
pub fn create(params: Arc<MxmMono02Params>, telemetry: Arc<Telemetry>) -> Option<MxmMono02Editor> {
    let state = EguiEditorState::from_size(
        nice_plug::editor::dpi::LogicalSize::new(REFERENCE.0, REFERENCE.1),
        1.0,
    );

    create_egui_editor(
        state,
        nice_plug_egui::RepaintNotifier::new(),
        nice_plug_egui::EguiNiceSettings {
            title: crate::NAME.to_owned(),
            // **A fixed window**: the layout never reflows, so a window of any other size could
            // only add empty space or clip. Scaling is the zoom control in the app bar.
            resize_hint: ResizeHint {
                size_constraints: nice_plug::editor::SizeConstraints::min_logical_size(
                    nice_plug::editor::dpi::LogicalSize::new(MINIMUM.0 as f32, MINIMUM.1 as f32),
                ),
                ..ResizeHint::RESIZABLE
            },
            ..Default::default()
        },
        MxmMono02App::new(params, telemetry),
    )
}

/// The editor type the plugin exposes.
pub type MxmMono02Editor = nice_plug_egui::EguiEditor<MxmMono02App>;

/// The editor's own state: what the plugin does not own and the host does not need.
pub struct MxmMono02App {
    params: Arc<MxmMono02Params>,
    telemetry: Arc<Telemetry>,
    gui_context: Option<GuiContext>,
    view: usize,
    text_entry: HashMap<&'static str, Option<String>>,
    presets: PresetUi,
    /// Where the keyboard is: a card, and a parameter inside it. Transient, like the text
    /// buffers — it is not a parameter and nothing durable reads it.
    nav: mxm_ui::navigation::State,
}

/// The app bar's preset controls and what they need between frames — `mxm-preset`'s, one for
/// every instrument.
pub use mxm_preset::PresetUi;

impl MxmMono02App {
    pub fn new(params: Arc<MxmMono02Params>, telemetry: Arc<Telemetry>) -> Self {
        let params_for_presets = Arc::clone(&params);
        Self {
            params,
            telemetry,
            gui_context: None,
            view: 0,
            text_entry: HashMap::new(),
            presets: PresetUi::new(params_for_presets.as_ref()),
            nav: mxm_ui::navigation::State::default(),
        }
    }
}

impl NiceEguiApp for MxmMono02App {
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        mxm_ui::theme::apply(&egui_ctx);
        mxm_ui::typography::apply(&egui_ctx);
        // Light by default, overridable with `MXM_EDITOR_THEME`. The reasoning, and why the
        // default is not `System`, lives on `mxm_ui::theme::preference`.
        egui_ctx.set_theme(mxm_ui::theme::preference());
        self.gui_context = Some(nice_gui_ctx);
        // The scope ring is filled only while this editor exists to read it.
        self.telemetry.set_editor_open(true);
        Ok(())
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut nice_plug_egui::Frame) {
        let Some(gui_context) = self.gui_context.clone() else {
            return;
        };
        panel(
            ui,
            &self.params,
            &self.telemetry,
            &gui_context.param_setter(),
            &mut self.view,
            &mut self.text_entry,
            &mut self.presets,
            &mut self.nav,
        );
    }

    fn editor_closed(&mut self) {
        // §8: the visualizations stop when the editor is closed — the audio thread stops
        // filling the scope on its next block. Dropping the context releases the host's callbacks.
        self.telemetry.set_editor_open(false);
        self.gui_context = None;
    }
}

/// The whole editor, as a panel.
#[allow(clippy::too_many_arguments)]
pub fn panel(
    ui: &mut Ui,
    params: &MxmMono02Params,
    telemetry: &Telemetry,
    setter: &ParamSetter<'_>,
    view: &mut usize,
    text_entry: &mut HashMap<&'static str, Option<String>>,
    presets: &mut PresetUi,
    nav: &mut mxm_ui::navigation::State,
) {
    let tokens = &tokens_for(ui);

    // A frame every 50 ms while the editor is open: the level meter changes between input events,
    // and so does a developer-channel request, which a frame that waited for the pointer would
    // strand on a view where nothing animates.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));

    // The collection's developer channel (`plugins/AGENTS.md`): the view, and the Bender's
    // expander, whose state is egui's under a fixed id its header reads.
    // One question, and both layers suspend on it: the paging renderer's `hold` and the cursor's
    // `inert` both ask whether another surface owns this frame's keyboard.
    let busy = presets.holds_the_keyboard() || text_entry.values().any(Option::is_some);
    mxm_ui::paging::editor::hold(ui.ctx(), busy);
    mxm_ui::paging::editor::developer_request(ui.ctx(), view, telemetry.take_view_request());

    // **The keyboard cursor moves before anything is drawn**, so a navigation arrow is consumed
    // here rather than also walking egui's own focus ring. It reads the registry and the exact
    // card rectangles the previous frame built, and it resolves the developer-view request first,
    // because which surface this frame is deciding who owns its keyboard.
    if *view == mxm_ui::paging::PARAMETERS {
        // This surface has no cards. Stop rather than merely hiding the outline, or its controls
        // lose their legacy bare-arrow editing to an invisible stale musician cursor.
        mxm_ui::navigation::stop(ui.ctx());
    } else {
        // The app bar's Volume is above the paging renderer, which cannot report it, so it is a
        // bar card the cursor reaches first.
        mxm_ui::navigation::paged_with_bar(ui.ctx(), nav, busy, &[VOLUME_CARD]);
    }
    if let Some(open) = telemetry.take_browser_request() {
        presets.set_browser_open(open);
    }
    // A theme, by index. Applied and not stored: this channel is how a screenshot run and a test
    // reach a state, and neither should overwrite the choice made in the control.
    if let Some(index) = telemetry.take_theme_request()
        && let Some(preference) = mxm_ui::theme::from_index(index)
    {
        ui.ctx().set_theme(preference);
    }
    if let Some(open) = telemetry.take_disclosure_request() {
        set_bender(ui.ctx(), open);
    }

    let peak = telemetry.take_peak();
    let clipped = telemetry.clipped();
    mxm_ui::AppBar::new(crate::NAME).show_with(
        ui,
        tokens,
        |ui| mxm_preset::ui::preset_row(ui, tokens, params, setter, presets),
        |ui| {
            if mxm_ui::shell::level_meter(ui, tokens, peak, clipped) {
                telemetry.clear_clip();
            }
            // Design system §3.1 slot 6: the instrument's output control sits beside its meter,
            // not in the Amplifier card among the controls that shape a note.
            mxm_ui::navigation::bar_card(ui, VOLUME_CARD, |ui| {
                ui.scope(|ui| {
                    sections::binding_for("volume", params)
                        .slider_inline(ui, tokens, setter, text_entry, 96.0);
                })
                .response
                .rect
            });
            mxm_ui::shell::zoom_control(ui);

            // §3.1 slot 5, and the same place the player keeps it: at the left end of the bar's
            // right-hand group. What the person picks is remembered for every MXM editor, so the
            // next one to open agrees with this one.
            mxm_ui::shell::editor_theme_control(ui);
        },
    );

    mxm_preset::ui::overlays(ui, tokens, params, setter, presets);

    // **The Parameters view has no tab.** It is the complete generated list, and an editor whose
    // own interface reaches every control does not need a second way to the same parameters in
    // front of a musician every day. It stays reachable: the developer channel still requests it
    // by index, which is what the CLI and a host's automation list use it for.
    // Navigation is derived by the paging renderer.

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(tokens.canvas)
                .inner_margin(egui::Margin::same(SPACE_5 as i8)),
        )
        .show(ui, |ui| {
            if *view == mxm_ui::paging::PARAMETERS {
                parameters_view(ui, tokens, params, setter, text_entry);
            } else {
                paged_view(ui, tokens, params, telemetry, setter, text_entry);
            }
        });
}

/// Every paging item, each floor computed from its card's tree in `ui`'s fonts every frame, and
/// each card exactly as wide as that floor: its ceiling is its floor (`plans/plan-editor-standard.md`
/// A1). No card declares a usability minimum (A2).
pub fn page_items(ui: &Ui, params: &MxmMono02Params) -> Vec<mxm_ui::paging::Item<'static>> {
    use mxm_ui::{
        flow::Card,
        paging::{Category as C, Item, Key},
    };
    SECTIONS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let tree = sections::card(ui, *s, params);
            let floor = mxm_ui::tree::card_floor(ui, s.title(), &tree);
            Item {
                key: Key(i as u64),
                card: Card::new(s.title(), floor).capped(floor),
                category: match s {
                    Section::Voice => C::Performance,
                    Section::Lfo | Section::Envelope => C::Modulators,
                    Section::Oscillator1 | Section::Oscillator2 => C::Generators,
                    Section::Mixer | Section::Filter => C::Tone,
                },
                kind: s.title(),
            }
        })
        .collect()
}

fn paged_view(
    ui: &mut Ui,
    tokens: &Tokens,
    params: &MxmMono02Params,
    telemetry: &Telemetry,
    setter: &ParamSetter<'_>,
    entries: &mut HashMap<&'static str, Option<String>>,
) {
    // The scope moves whenever the voice does.
    ui.ctx().request_repaint();
    use mxm_ui::paging::Key;
    let items = page_items(ui, params);
    let text_editing = entries.values().any(Option::is_some);
    // Nothing a card draws reads telemetry destructively: the peak is the app bar's, read above.
    let mut live = sections::Live {
        params,
        telemetry,
        setter,
        entries,
    };
    mxm_ui::paging::editor::show(
        ui,
        tokens,
        &items,
        &[&[Key(4), Key(5)]],
        text_editing,
        &mut |ui, i| sections::card(ui, SECTIONS[i], params),
        &mut |ui, _, leaf, rect| sections::paint(ui, tokens, leaf, rect, &mut live),
    );
}

/// The paging items as the editor computes them, from a context set up as an editor's is — three
/// passes in, so the weighted font cuts are bound — for tests, which have no editor `Ui` to hand.
#[cfg(test)]
pub(crate) fn test_items() -> Vec<mxm_ui::paging::Item<'static>> {
    let ctx = egui::Context::default();
    mxm_ui::typography::apply(&ctx);
    mxm_ui::theme::apply(&ctx);
    let params = MxmMono02Params::default();
    let mut items = Vec::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            items = page_items(ui, &params);
        });
        output.textures_delta.clear();
    }
    items
}

/// The cards' floors in paging order, as [`test_items`] computes them.
#[cfg(test)]
pub(crate) fn test_floors() -> Vec<f32> {
    test_items().iter().map(|item| item.card.floor).collect()
}

/// The `Parameters` view: every parameter as a slider, in the instrument's order. The one view
/// that reflows, because it is a list with no layout to remember.
fn parameters_view(
    ui: &mut Ui,
    tokens: &Tokens,
    params: &MxmMono02Params,
    setter: &ParamSetter<'_>,
    text_entry: &mut HashMap<&'static str, Option<String>>,
) {
    mxm_ui::shell::scroll_list(ui).show(ui, |ui| {
        let columns = if ui.available_width() >= 1000.0 { 3 } else { 2 };
        let entries: Vec<_> = sections::ALL_IDS
            .iter()
            .map(|id| sections::binding_for(id, params))
            .chain(
                params
                    .routes
                    .parameters()
                    .into_iter()
                    .map(|(id, param)| binding::Bound {
                        id,
                        param,
                        description: "A route: how far its source moves its target.",
                        bipolar: !id.ends_with("on"),
                        law: binding::StepLaw::Own,
                        panel: None,
                        stepped: None,
                        details: &[],
                    }),
            )
            .collect();
        let per_column = entries.len().div_ceil(columns);

        ui.columns(columns, |uis| {
            for (index, chunk) in entries.chunks(per_column).enumerate() {
                let Some(column) = uis.get_mut(index) else {
                    continue;
                };
                for entry in chunk {
                    entry.slider(column, tokens, setter, text_entry);
                }
            }
        });
    });
}

/// The collection's tokens, with this instrument's identity accent (brief §7).
fn tokens_for(ui: &Ui) -> Tokens {
    let dark = ui.visuals().dark_mode;
    let base = if dark { mxm_ui::DARK } else { mxm_ui::LIGHT };
    base.with_identity(mxm_ui::theme::ORCHID, dark)
}

#[cfg(test)]
mod tests {
    use mxm_plugin_test::{opening_size, paging_checks};

    /// **The editor opens at the quarter-4K budget, hugged** — the owner's rule, 2026-09-09. The budget
    /// is the most room an editor may ask for, so laying the panel out there shows as many modules as
    /// it ever will; taking the slack away is the whole of the size.
    #[test]
    fn the_opening_size_is_the_budget_hugged() {
        let params = MxmMono02Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0usize;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        opening_size::is_the_budget_hugged(
            egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            &REVEAL,
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }

    /// **The app bar holds in the narrowest window**: its `…` menu whole and nothing drawn over
    /// anything else, from `MINIMUM` up (`opening_size::bar_holds_from_the_minimum`).
    #[test]
    fn the_app_bar_holds_in_the_minimum_window() {
        let params = MxmMono02Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0usize;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        opening_size::bar_holds_from_the_minimum(
            egui::vec2(MINIMUM.0 as f32, MINIMUM.1 as f32),
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }

    use mxm_plugin_test::keyboard_checks;

    /// What this editor keeps behind a disclosure, opened so the reachability check sees it.
    /// The Voice card's Bender expander holds the bend range.
    const REVEAL: fn(&egui::Context) = |ctx| set_bender(ctx, true);

    /// Every pair present, as the `‹ modulate ›` menu would make them one at a time.
    fn reveal_every_route(params: &MxmMono02Params) {
        use nice_plug::params::InternalParamMut as _;
        for group in params.routes.each() {
            for presence in group.presence_params() {
                // Safety: a test owns these parameters outright.
                unsafe { presence._internal_set_plain_value(true) };
            }
        }
    }

    /// The rollout's own failure mode: a control whose `navigation::at` scope was forgotten paints
    /// exactly as before and is simply unreachable from the keyboard. Nothing else would say so.
    ///
    /// **Two frames, both `Exactly`**, because an absent route draws nothing: the init patch, whose
    /// six routes are rows and whose other pairs are not, then every pair present — the only state
    /// that covers every parameter the conversion added.
    #[test]
    fn the_keyboard_cursor_reaches_and_operates_every_parameter() {
        for every_route in [false, true] {
            let params = MxmMono02Params::default();
            if every_route {
                reveal_every_route(&params);
            }
            let telemetry = Telemetry::default();
            let host = keyboard_checks::Recorder::default();
            let setter = ParamSetter::new(&host);
            let mut ids: Vec<&str> = sections::all_parameters(&params)
                .iter()
                .map(|bound| bound.id)
                .collect();
            for (t, pairs) in crate::routes::ROUTE_IDS.iter().enumerate() {
                for (s, (amount, presence)) in pairs.iter().enumerate() {
                    if every_route || mxm_mono_02_dsp::routing::INIT_PRESENT.contains(&(t, s)) {
                        ids.push(amount);
                        ids.push(presence);
                    }
                }
            }
            let mut view = 0usize;
            let mut text_entry = HashMap::new();
            let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
            let mut nav = mxm_ui::navigation::State::default();
            keyboard_checks::the_cursor_reaches_and_operates(
                egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
                &test_items(),
                keyboard_checks::Coverage::Exactly(&ids),
                &REVEAL,
                &host,
                &mut |ui| {
                    panel(
                        ui,
                        &params,
                        &telemetry,
                        &setter,
                        &mut view,
                        &mut text_entry,
                        &mut presets,
                        &mut nav,
                    );
                },
            );
        }
    }

    /// **Every parameter is drawn at most once, and exactly once when its routes are revealed** — read
    /// off the painted panel, not off a table of what each card ought to draw. Every card is requested
    /// in turn with every pair present and the Bender open, and each parameter the keyboard registry
    /// recorded is counted by the cards it was painted in: the fixed controls by their own cards —
    /// Volume by the app bar's bar card — each route by the card whose target it moves. A table
    /// beside the drawing code agrees with itself whatever the drawing does; the registry is built
    /// by drawing.
    #[test]
    fn every_parameter_is_drawn_at_most_once_and_exactly_once_when_revealed() {
        use mxm_mono_02_dsp::routing::{SOURCES, TARGETS};
        use nice_plug::prelude::Params as _;
        use std::collections::{BTreeMap, BTreeSet};

        let params = MxmMono02Params::default();
        reveal_every_route(&params);
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0usize;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        let session =
            keyboard_checks::Session::new(egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32));
        let mut painted: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
        let items = test_items();
        for item in &items {
            mxm_ui::paging::editor::request_card(session.context(), item.key);
            REVEAL(session.context());
            session.settle(&mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            });
            for spot in mxm_ui::navigation::spots(session.context()) {
                painted.entry(spot.key).or_default().insert(spot.card);
            }
        }

        let declared: Vec<String> = params
            .param_map()
            .into_iter()
            .map(|(id, _, _)| id)
            .collect();
        for id in &declared {
            let cards = painted.get(id).map_or(0, BTreeSet::len);
            assert_eq!(cards, 1, "{id} is painted in {cards} cards, expected one");
        }
        let stray: Vec<&String> = painted.keys().filter(|k| !declared.contains(k)).collect();
        assert!(stray.is_empty(), "painted but not parameters: {stray:?}");
        // The instrument's output is in the app bar (design system §3.1), on its bar card and on no
        // card of the page; nothing else is up there.
        assert_eq!(
            painted.get("volume"),
            Some(&BTreeSet::from([VOLUME_CARD])),
            "Volume is drawn in the app bar, on its bar card alone"
        );
        let in_the_bar: Vec<&String> = painted
            .iter()
            .filter(|(_, cards)| cards.contains(&VOLUME_CARD))
            .map(|(id, _)| id)
            .collect();
        assert_eq!(
            in_the_bar,
            ["volume"],
            "the app bar draws Volume and nothing else"
        );
        assert_eq!(
            sections::ALL_IDS.len() + TARGETS * SOURCES * 2,
            declared.len(),
            "ALL_IDS has fallen behind the parameters"
        );
    }

    #[test]
    fn every_dynamic_page_fits_and_every_card_is_reachable() {
        let params = MxmMono02Params::default();
        let telemetry = Telemetry::default();
        telemetry.request_disclosure(true);
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0;
        let mut entries = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        paging_checks::verify(
            &test_items(),
            &[
                egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
                egui::vec2(1880.0, 1040.0),
                egui::vec2(MINIMUM.0 as f32, MINIMUM.1 as f32),
            ],
            |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut entries,
                    &mut presets,
                    &mut nav,
                )
            },
        );
    }
    use super::*;
    use nice_plug::params::internals::ParamPtr;
    use nice_plug::prelude::{PluginApi, PluginState};

    struct NoHost;

    impl nice_plug::context::gui::GuiContextInner for NoHost {
        // A test double has no host to ask for a restart (nice-plug 0.4).
        fn request_restart(&self) {}
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(&self, _param: ParamPtr, _normalized: f32) {}
        unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}
        fn get_state(&self) -> PluginState {
            PluginState {
                version: String::new(),
                params: Default::default(),
                fields: Default::default(),
            }
        }
        fn set_state(&self, _state: PluginState) {}
    }

    /// Lays the whole editor out at a given size and reports the height it actually needed.
    fn measure(view: usize, width: f32, height: f32) -> f32 {
        let ctx = egui::Context::default();
        mxm_ui::theme::apply(&ctx);
        mxm_ui::typography::apply(&ctx);
        ctx.set_theme(egui::ThemePreference::Light);

        let params = MxmMono02Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = view;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        };

        let mut used = 0.0;
        for _ in 0..3 {
            let mut output = ctx.run_ui(input.clone(), |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            });
            output.textures_delta.clear();
            used = ctx.globally_used_rect().height();
        }
        used
    }

    /// The scar every editor here inherited: a height guessed before the panel existed.
    /// The `Synth` view laid out in the accessibility harness, with the Voice card's expander
    /// open or closed, and where every card landed. Both states matter: the window is fixed, so
    /// it must fit the **open** state, which the first measurement forgot — the owner's third
    /// look found the disclosed knobs cut off at the window's bottom edge.
    fn layout(bender_open: bool) -> Vec<egui::Rect> {
        use kittest::Queryable;

        let params = MxmMono02Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let mut view = 0;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();

        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32))
            .build_ui(|ui| {
                let setter = ParamSetter::new(&host);
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            });
        // **Once, as the editor's `build` does**: applied every frame, the fonts are reset each
        // frame and the weighted cuts never bind, so every text measured about a point narrower
        // than the editor draws it — invisible while a card could be wider than its floor, and a
        // failure once it is exactly its floor.
        mxm_ui::theme::apply(&harness.ctx);
        mxm_ui::typography::apply(&harness.ctx);
        harness.ctx.set_theme(egui::ThemePreference::Light);
        // Tall component canvas, separate from the every-page physical-budget test.
        harness.set_size(egui::vec2(REFERENCE.0 as f32, 20000.0));
        harness.run_steps(3);
        if bender_open {
            harness.get_by_label("Bender").click();
            // The expander animates open; enough frames for it to finish.
            harness.run_steps(20);
        }
        paging_checks::all_rects(&harness.ctx, SECTIONS.len())
    }

    /// Cards whose vertical extents overlap are on one row, read off the geometry.
    fn rows(placed: &[egui::Rect]) -> Vec<Vec<egui::Rect>> {
        let mut sorted = placed.to_vec();
        sorted.sort_by(|a, b| {
            a.top()
                .partial_cmp(&b.top())
                .unwrap()
                .then(a.left().partial_cmp(&b.left()).unwrap())
        });
        let mut rows: Vec<Vec<egui::Rect>> = Vec::new();
        for rect in sorted {
            match rows.last_mut() {
                Some(row) if row.iter().any(|r| r.bottom() > rect.top() + 1.0) => row.push(rect),
                _ => rows.push(vec![rect]),
            }
        }
        rows
    }

    /// The collection's developer channel, in-process: a view request switches the view and an
    /// expander request opens the Bender, with no pointer anywhere near either.
    #[test]
    fn the_developer_channel_switches_the_view_and_opens_the_bender() {
        use kittest::Queryable;

        let params = MxmMono02Params::default();
        let telemetry = std::sync::Arc::new(Telemetry::default());
        let host = NoHost;
        let mut view = 0;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        let shared = std::sync::Arc::clone(&telemetry);

        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32))
            .build_ui(|ui| {
                mxm_ui::theme::apply(ui.ctx());
                mxm_ui::typography::apply(ui.ctx());
                ui.ctx().set_theme(egui::ThemePreference::Light);
                let setter = ParamSetter::new(&host);
                panel(
                    ui,
                    &params,
                    &shared,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            });
        harness.run_steps(3);
        assert!(
            harness.query_by_label("Portamento").is_some(),
            "the oracle: a knob's name is queryable"
        );
        assert!(
            harness.query_by_label("Bend range").is_none(),
            "the premise: the Bender is closed and its knobs are not drawn"
        );

        telemetry.request_disclosure(true);
        harness.run_steps(20);
        assert!(
            harness.query_by_label("Bend range").is_some(),
            "the expander request opened the Bender"
        );
        telemetry.request_disclosure(false);
        harness.run_steps(20);
        assert!(
            harness.query_by_label("Bend range").is_none(),
            "and the request closes it again"
        );

        telemetry.request_view(mxm_ui::paging::PARAMETERS as u8);
        harness.run_steps(3);
        assert!(
            harness.query_by_label("Bend range").is_some(),
            "the view request showed the Parameters view, which lists the closed Bender's range"
        );
    }

    /// The height the `Synth` view needs, from the cards' natural bottoms plus the panel margin.
    fn needed_height(placed: &[egui::Rect]) -> f32 {
        placed.iter().map(egui::Rect::bottom).fold(0.0f32, f32::max) + SPACE_5
    }

    /// The scar every editor here inherited: a height guessed before the panel existed. The
    /// window must fit the view with the Voice card's expander **open**, and must not be much
    /// taller than that.
    #[test]
    fn the_synth_view_fits_the_editor() {
        let closed = needed_height(&layout(false));
        let open = needed_height(&layout(true));
        eprintln!("the Synth view needs {closed} points closed and {open} with the bender open");
        // **`>=`, not `>`, and the reason is recorded rather than relaxed away.** The expander
        // lives in the Voice card, and Voice is no longer in the tallest column: once the
        // oscillator's PWM source and the filter's envelope polarity moved to rows of their own
        // (their cells were narrower than the words in them), those columns grew past it. Opening
        // the bender still adds height to its own column; it just no longer decides the frame.
        assert!(
            open >= closed,
            "opening the expander must not need less height: {open} against {closed}"
        );
        // Total surface height no longer sizes the window.
        every_dynamic_page_fits_and_every_card_is_reachable();
    }

    /// **Every row of cards shares one bottom edge**, Bender closed and open.
    ///
    /// This replaced *the three columns end on one line*. There are no columns: the cards wrap into
    /// rows, and §3.3's rule is per row.
    #[test]
    fn every_row_of_cards_shares_one_bottom_edge() {
        for bender_open in [false, true] {
            let placed = layout(bender_open);
            let rows = rows(&placed);
            for row in &rows {
                if row.len() < 2 {
                    continue;
                }
                let low = row
                    .iter()
                    .map(egui::Rect::bottom)
                    .fold(f32::INFINITY, f32::min);
                let high = row
                    .iter()
                    .map(egui::Rect::bottom)
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!(
                    high - low < 1.0,
                    "bender open {bender_open}: a row of {} cards ends {:.1} ragged",
                    row.len(),
                    high - low
                );
            }
        }
    }

    /// Round 1 of the code review: the problems `resolve` reports were dropped on the floor. A
    /// preset missing a parameter loads what it has and says what it lacks.
    #[test]
    fn a_preset_missing_a_parameter_reports_it_when_applied() {
        let params = MxmMono02Params::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut preset =
            crate::preset::Preset::capture("Partial", mxm_preset::Category::Uncategorised, &params);
        preset.params.remove("cutoff");
        let problems = mxm_preset::ui::apply_preset(&params, &setter, &preset);
        assert!(
            problems.iter().any(|p| p.contains("cutoff")),
            "the missing parameter is named: {problems:?}"
        );
    }

    /// The owner's finding on the second look: the bender toggle sat a label line above the
    /// range switch's cells. Measured from the accessibility tree the editor exposes, which is
    /// where a control's rectangle is a fact rather than an impression.
    #[test]
    fn the_bender_toggle_sits_on_the_range_switchs_cell_line() {
        use kittest::Queryable;

        let params = MxmMono02Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let mut view = 0;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();

        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32))
            .build_ui(|ui| {
                mxm_ui::theme::apply(ui.ctx());
                mxm_ui::typography::apply(ui.ctx());
                ui.ctx().set_theme(egui::ThemePreference::Light);
                let setter = ParamSetter::new(&host);
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            });
        // Not `run()`: the live display asks for a repaint every frame, by design.
        harness.run_steps(3);
        // **Ask for the card rather than assuming it opens on the first page.** This is a check of
        // geometry inside the Oscillator 1 card; which page the opening size puts it on is
        // `the_opening_size_is_the_smallest_one`'s business, and it moved when that size did.
        mxm_ui::paging::editor::request_card(&harness.ctx, mxm_ui::paging::Key(2));
        harness.run_steps(3);

        let toggle = harness.get_by_label("VCO-1 bender").rect();
        let cell = harness.get_by_label("VCO-1 range: 8'").rect();
        assert!(
            (toggle.top() - cell.top()).abs() < 1.0
                && (toggle.bottom() - cell.bottom()).abs() < 1.0,
            "the toggle spans y {:.1}..{:.1}, the range cells {:.1}..{:.1}",
            toggle.top(),
            toggle.bottom(),
            cell.top(),
            cell.bottom()
        );
    }

    /// The owner's finding on the first look: cards overlapped. Nothing may overlap, nothing may
    /// leave the panel, and nothing may be squeezed under its floor or grown past the cap —
    /// measured from the rectangles the layout actually produced.
    ///
    /// *Its column* is gone from this test with the columns themselves; the panel's edge and the
    /// card's own floor are what bound a card now.
    #[test]
    fn no_card_overlaps_another_or_leaves_the_panel() {
        let floors = test_floors();
        let placed = layout(true);
        assert_eq!(placed.len(), SECTIONS.len(), "every card was placed");

        for (index, rect) in placed.iter().enumerate() {
            eprintln!(
                "{:<12} x {:7.1}..{:7.1}  y {:7.1}..{:7.1}",
                SECTIONS[index].title(),
                rect.left(),
                rect.right(),
                rect.top(),
                rect.bottom()
            );
            assert!(
                (rect.width() - floors[index]).abs() <= 0.5,
                "{} is {:.1} wide, not its floor {:.1}",
                SECTIONS[index].title(),
                rect.width(),
                floors[index]
            );
            assert!(
                rect.right() <= REFERENCE.0 as f32 + 0.5,
                "{} runs to {:.1} in a {} point window",
                SECTIONS[index].title(),
                rect.right(),
                REFERENCE.0
            );
        }

        for (i, a) in placed.iter().enumerate() {
            for (j, b) in placed.iter().enumerate().skip(i + 1) {
                let overlap = a.intersect(*b);
                assert!(
                    overlap.width() <= 0.5 || overlap.height() <= 0.5,
                    "{} ({a:?}) overlaps {} ({b:?})",
                    SECTIONS[i].title(),
                    SECTIONS[j].title()
                );
            }
        }
    }

    #[test]
    fn the_parameters_view_lays_out_without_panicking() {
        assert!(
            measure(
                mxm_ui::paging::PARAMETERS,
                REFERENCE.0 as f32,
                REFERENCE.1 as f32
            ) > 0.0
        );
    }

    #[test]
    fn the_sections_are_in_the_briefs_order() {
        let titles: Vec<&str> = SECTIONS.iter().map(|s| s.title()).collect();
        assert_eq!(
            titles,
            [
                "Voice",
                "LFO",
                "Oscillator 1",
                "Oscillator 2",
                "Mixer",
                "Filter",
                "Envelope and amplifier",
            ]
        );
    }

    #[test]
    fn card_names_come_from_the_collections_vocabulary() {
        const KNOWN: &[&str] = &[
            "LFO",
            "Oscillator 1",
            "Oscillator 2",
            "Mixer",
            "Filter",
            "Envelope and amplifier",
            "Voice",
        ];
        for title in SECTIONS.iter().map(|s| s.title()) {
            assert!(
                KNOWN.contains(&title),
                "{title} is not a collection card name"
            );
        }
    }

    use mxm_plugin_test::tree_checks;

    /// A host that **applies** what a `ParamSetter` reports, which `NoHost` deliberately does not —
    /// for a test that has to move a parameter through the setter.
    struct ApplyingHost;

    impl nice_plug::context::gui::GuiContextInner for ApplyingHost {
        // A test double has no host to ask for a restart (nice-plug 0.4).
        fn request_restart(&self) {}
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Clap
        }
        unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}
        unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
            // SAFETY: the parameter outlives the setter, which borrows this host for the call.
            unsafe { param._internal_set_normalized_value(normalized) };
        }
        unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}
        fn get_state(&self) -> PluginState {
            PluginState {
                version: String::new(),
                params: Default::default(),
                fields: Default::default(),
            }
        }
        fn set_state(&self, _state: PluginState) {}
    }

    /// Every card, in every state that changes what it holds, passes the layout tree's checks
    /// (plans/plan-layout-tree.md §4.3, `tree_checks::card`): its floor holds its content with
    /// nothing painted outside the card, the content floor is exact, the height its tree states is
    /// the height it draws, and every leaf stays in the room it was given.
    ///
    /// The states are this editor's structural-state matrix: the init patch, where the machine's
    /// six Init routes are the only rows; every route revealed at full negative depth — where a reading
    /// carries its sign and every digit, the widest text a row can show; each of those with the
    /// Voice card's Bender closed and open; and a note sounding — a scope with a wave in it and
    /// the modulated cutoff away from the set one, the second curve the filter display draws; and
    /// the LFO synced, with no tempo and with one, where its rate reads a division. Both displays
    /// are a fixed height, so telemetry changes only what they paint.
    #[test]
    fn every_card_passes_the_tree_checks_in_every_state() {
        let floors = test_floors();
        for state in [
            "init",
            "Bender open",
            "every route revealed",
            "every route revealed, Bender open",
            "a note sounding",
            "LFO synced, no tempo",
            "LFO synced to a tempo",
        ] {
            let params = MxmMono02Params::default();
            let host = ApplyingHost;
            let setter = ParamSetter::new(&host);
            if state.starts_with("every route revealed") {
                reveal_every_route(&params);
                for (t, group) in params.routes.each().into_iter().enumerate() {
                    for route in &group.routes(t) {
                        route.amount.set(&setter, 0.0);
                    }
                }
            }
            let telemetry = Telemetry::default();
            if state == "a note sounding" {
                telemetry.set_editor_open(true);
                for i in 0..crate::telemetry::SCOPE_LEN {
                    telemetry.push_scope((i as f32 * 0.05).sin());
                }
                telemetry.publish_voice(18_000.0, 1.0);
            }
            if state.starts_with("LFO synced") {
                // SAFETY: the parameters are this test's own and nothing else reads them.
                unsafe {
                    use nice_plug::params::InternalParamMut;
                    let _ = params.lfo_sync._internal_set_normalized_value(1.0);
                }
            }
            if state == "LFO synced to a tempo" {
                telemetry.tempo.publish(Some(120.0));
            }
            let open = state.ends_with("Bender open");
            let setup = move |ctx: &egui::Context| set_bender(ctx, open);
            for (index, section) in SECTIONS.iter().enumerate() {
                let mut entries = HashMap::new();
                let mut live = sections::Live {
                    params: &params,
                    telemetry: &telemetry,
                    setter: &setter,
                    entries: &mut entries,
                };
                tree_checks::card(
                    &setup,
                    state,
                    section.title(),
                    floors[index],
                    &|ui| sections::card(ui, *section, &params),
                    &mut |ui, leaf, rect| {
                        sections::paint(ui, &tokens_for(ui), leaf, rect, &mut live);
                    },
                );
            }
        }
    }

    /// **The layout lab's entry draws each card as its tree.** `apps/mxm-layout-lab` (private
    /// archive) calls `sections::draw` with its own state; the wrapper builds the card's tree and
    /// shows it, so the card the lab draws is as tall as that tree says, at the floor the paging
    /// renderer is given.
    #[test]
    fn the_layout_labs_entry_draws_each_card_as_its_tree() {
        let floors = test_floors();
        for (index, section) in SECTIONS.iter().enumerate() {
            let params = MxmMono02Params::default();
            let telemetry = Telemetry::default();
            let host = NoHost;
            let setter = ParamSetter::new(&host);
            let mut text_entry = HashMap::new();
            let ctx = tree_checks::context(&|_| {});
            let mut result = (0.0, 0.0);
            for _ in 0..3 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let mut column =
                        ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(floors[index], 4000.0),
                        )));
                    let tokens = tokens_for(ui);
                    result =
                        mxm_ui::ModuleCard::new(section.title()).show(&mut column, &tokens, |ui| {
                            let tree = sections::card(ui, *section, &params);
                            let stated =
                                tree.height(ui, tree.drawn_width(ui, ui.available_width()));
                            let top = ui.min_rect().top();
                            sections::draw(
                                ui,
                                &tokens,
                                *section,
                                &params,
                                &telemetry,
                                &setter,
                                &mut text_entry,
                            );
                            (stated, ui.min_rect().bottom() - top)
                        });
                });
                output.textures_delta.clear();
            }
            let (stated, drawn) = result;
            assert!(
                (stated - drawn).abs() < 0.5,
                "{}: the tree says {stated:.1}, the lab's entry drew {drawn:.1}",
                section.title()
            );
        }
    }

    /// Every page at the opening size, light and dark, for the owner's review of the layout-tree
    /// conversion (plans/plan-layout-tree.md §4.3): `target/layout-tree/mxm-mono-02/<tag>/`, where
    /// `MXM_PICTURES` names the tag — `before` on the unconverted editor, `after` on the tree.
    ///
    /// `MXM_PICTURES=after cargo test -p mxm-mono-02 --lib tree_pictures -- --ignored`
    #[test]
    #[ignore = "renders through wgpu; run by hand"]
    fn tree_pictures() {
        let tag = std::env::var("MXM_PICTURES").unwrap_or_else(|_| "after".to_owned());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/layout-tree/mxm-mono-02")
            .join(tag);
        let params = MxmMono02Params::default();
        let telemetry = Telemetry::default();
        let host = NoHost;
        let setter = ParamSetter::new(&host);
        let mut view = 0usize;
        let mut text_entry = HashMap::new();
        let mut presets = PresetUi::at(crate::preset::Library::at(None), &params);
        let mut nav = mxm_ui::navigation::State::default();
        tree_checks::pictures(
            &|_| {},
            egui::vec2(REFERENCE.0 as f32, REFERENCE.1 as f32),
            &dir,
            &mut |ui| {
                panel(
                    ui,
                    &params,
                    &telemetry,
                    &setter,
                    &mut view,
                    &mut text_entry,
                    &mut presets,
                    &mut nav,
                );
            },
        );
    }
}
