# AGENTS.md — plugins/mxm-mono-02

Parent: [`../AGENTS.md`](../AGENTS.md)

# Purpose

The nice-plug shell for **mxm-mono-02**, a monophonic synthesizer inspired by the Roland SH-2.
Identity, parameters and their routing, MIDI, presets, telemetry and the editor. The whole voice is
[`crates/mxm-mono-02-dsp`](../../crates/mxm-mono-02-dsp/AGENTS.md); the plans that shaped both are
`plans/plan-mxm-mono-02.md` (`plans/plan-mxm-mono-02.md` in the private archive) and, for the routing,
`plans/plan-mxm-mono-02-modulation.md` (`plans/plan-mxm-mono-02-modulation.md` in the private archive).

Shared conventions — nice-plug's API, the init-patch contract, preset rules, `process()` realtime
rules, the editor contract — live in the parent and are not restated here. This doc holds what is
**local to this plugin**; the history, measurements and worked detail behind it are in
[NOTES.md](NOTES.md).

# Ownership

`BASELINE-M0.md`, `Cargo.toml`, `LICENSE`, `README.md`, `control-map.json`, `presets/`, and `src/` —
`lib.rs`, `params.rs`, `routes.rs`, `preset.rs`, `telemetry.rs`, and `editor.rs` with its
`editor/{binding, sections, visuals}.rs`.

# Local Contracts

## Permanent identifiers

| What | Value |
|---|---|
| `CLAP_ID` | `dk.mxm.mxm-mono-02` — assembled from `plugin_name!` in `src/lib.rs`, **not** from `CARGO_PKG_NAME` |
| Parameter `#[id]`s | **LFO:** `lforate` `lfosync` `lfodelay` `lfomode`<br>**Oscillator:** `pulsewidth` `tune` `vco1range` `vco1wave` `vco1bender` `vco2range` `vco2wave` `vco2tune` `vco2tunerange`<br>**Mixer:** `sub` `vco1` `vco2`<br>**Filter:** `cutoff` `resonance`<br>**Envelope:** `attack` `decay` `sustain` `release` `trigger`<br>**Amplifier:** `vcamode`<br>**Voice:** `portamento` `bendrange`<br>**Output, in the app bar:** `volume` |
| Routing `#[id]`s | `mod_<target>_<source>`, the amount, and `mod_<target>_<source>on`, the presence — targets `pitch` `width` `cutoff` `amp`, sources `lfo` `lfowidth` `env` `autobend` `key` `vel` `wheel` `press` `bend` `vco1` `vco2` `sub` `noise`. Written out in `routes::ROUTE_IDS` |
| Retired, never re-used | `vcolfo` `autobend` `pwmdepth` `pwmmode` `envamount` `envpolarity` `vcflfo` `keytrack` `bendfilter` (the routing conversion); `mod_pitch_follower` `mod_width_follower` `mod_cutoff_follower` `mod_amp_follower` and each one's `…on` presence (the external input's removal, 2026-09-26) — `routes::RETIRED_IDS` |

**Treat all of it as public interface**: 131 host parameters, 27 controls and 104 routing
([NOTES.md § Permanent identifiers](NOTES.md#permanent-identifiers), with `lfosync`'s tempo sync).
`parameter_ids_are_unique`, `the_id_table_is_what_the_derive_actually_produces`,
`no_retired_id_reappears_among_the_parameters` and
`every_parameter_is_drawn_at_most_once_and_exactly_once_when_revealed` keep them agreeing.

## Routing: the machine's modulation, and its wiring is the init patch
[NOTES.md § The machine's modulation is routing](NOTES.md#the-machines-modulation-is-routing-and-its-wiring-is-the-init-patch)

- `routes.rs` declares a presence and a signed amount per *(target, source)* pair; the DSP's
  `routing` evaluates them, and nothing asks whether a route is the machine's own. The eight SH-2
  paths (`MACHINE`) keep their controls' reach; **six are present in Init at zero depth, the pulse
  width's two offered, not shown** (the owner, 2026-09-27;
  `the_init_patch_wires_six_of_the_machines_routes_at_zero_depth`).
- **An amount reads what its pair delivers**, in the target's unit; an added route reads the
  collection's standard reach (`a_route_reads_what_its_pair_delivers_and_reads_back`). Every amount
  is `mxm_modulation_params::reading::amount_param`, held to `mxm_mono_02_dsp::conformance` by
  `every_route_parameter_says_what_the_dsp_does`.
- **No reading prints a negative zero** (`mxm_modulation_params::signed`;
  `every_reading_survives_the_hosts_round_trip_a_rounded_zero_included`).
- **Decision X1 (the owner, 2026-09-15): no `filter_state` is built.** State saved before the
  conversion loses what the nine retired ids held; automation on a retired id is not carried.
- The Follower's eight route ids retired (2026-09-26): a state or preset naming them loads everything
  else (`a_state_naming_the_retired_follower_routes_still_loads`,
  `a_preset_naming_the_retired_follower_routes_loads_everything_else`).
- **Once per buffer, then once per sample**, through `begin_interval` and `render_sample`, which
  `process()` and `render_block_for_test` both call: `Routes::topology_from` snaps each newly present
  route's smoother to its stored depth; `Voice::set_topology` clears a newly read source;
  `Routes::settle` reads each settled amount once; `Routes::advance` ramps the rest and applies the
  wheel's push. Tests: `a_re_added_route_arrives_at_its_stored_depth_rather_than_ramping_from_a_stale_one`,
  `settled_routes_read_once_per_interval_match_advancing_every_sample`,
  `a_route_arriving_after_an_idle_span_is_block_partition_invariant`.
- What the routing costs is not yet measured: `BASELINE-M0.md` says why, and how.

## Parameter text survives the host's round trip
[NOTES.md § Every parameter's text survives the host's round trip](NOTES.md#every-parameters-text-survives-the-hosts-round-trip)

- A formatter must not switch unit or precision at the raw value: `v2s_time` chooses its unit from
  the rounded millisecond text, and `without_negative_zero` decides from the text.
- **Master tune cannot use nice-plug's `v2s_f32_rounded(0)`** (it prints `-0` at exactly −0.5 cents).
- `params::tests::every_parameter_reads_the_same_after_the_hosts_round_trip` holds all 131.

## Audio I/O and activation
[NOTES.md § No audio input](NOTES.md#no-audio-input) ·
[NOTES.md § Activation](NOTES.md#activation-refuses-a-rate-the-dsp-cannot-hold)

- `AUDIO_IO_LAYOUTS`: stereo then mono, **neither has an input of any kind**; the external input was
  removed (the owner, 2026-09-26). `process()` reads no auxiliary buffer.
- `activate` returns `false`, before anything changes, for a non-finite rate or one below
  `mxm_mono_02_dsp::MIN_SAMPLE_RATE` (`activation_refuses_a_non_finite_rate_and_any_below_the_floor`,
  `the_rate_floor_activates_and_plays_at_every_parameter_extreme`).

## Performance state follows the sounding press
[NOTES.md § Performance state](NOTES.md#performance-state-follows-the-sounding-press)

- Bend, wheel and channel pressure are kept per channel (sixteen each); the voice gets the channel
  of `voice.owner().channel`, so a press the block ignored moves nothing
  (`bend_and_wheel_follow_the_sounding_press_and_an_ignored_press_moves_nothing`,
  `velocity_and_pressure_follow_the_sounding_press_through_a_fallback`).
- The Velocity source is the press that last triggered the envelope, held by the voice. **The
  keyboard block lives in the DSP**: every note event passes `voice_id`, `channel`, `note` and velocity.
- **The mod wheel's legacy push stays** (plan N4): added into (Pitch ← LFO)'s amount whichever way it
  points, clamped to one, writing no parameter
  (`the_wheel_push_adds_the_same_whichever_way_the_route_points`). The README discloses it.

## The process status is the voice's activity
[NOTES.md § The process status](NOTES.md#the-process-status-is-the-voices-activity)

- `Voice::tail_samples(release, vca_mode)` decides: `None` (key down, or a HOLD drone) →
  `KeepAlive`; a finite **audible** tail → `Tail(n)`; idle → `Normal`, with exact zeros.
- HOLD sounds with no key down; the player's `KeepAlive` path keeps it sounding
  (`plugins/mxm-mono-02/host-tests/tests/behaviour.rs`).

## Terminations and smoothing
[NOTES.md § Three terminations](NOTES.md#three-terminations-and-the-hold-panic-latch) ·
[NOTES.md § Smoothed and unsmoothed](NOTES.md#smoothed-and-unsmoothed)

- Note-off releases its press; **All Notes Off** every press; **All Sound Off** (CC 120) cuts and, in
  HOLD, **latches silent** until a note-on, a host reset, or re-entering HOLD; choke is a note-off
  for its voice. All three are the DSP's; the shell only maps the messages.
- Smoothed, 10 ms linear: every route amount, the levels, cutoff, resonance, sustain, pulse width,
  both tunes and volume; `bendrange` at 20 ms, so a range edit under a held bend ramps
  (`a_range_edit_under_a_held_bend_ramps_rather_than_steps`; the owner, 2026-09-15). Not smoothed:
  presences, envelope times, the LFO rate and delay, switches and ranges — a configuration is not a signal.

## The control map
[NOTES.md § The control map claims the trigger role it added](NOTES.md#the-control-map-claims-the-trigger-role-it-added)

- `control-map.json` claims `amp_env.trigger`, the role added for this instrument.
- **Every depth role names the amount of a route Init wires**
  (`a_control_map_role_never_points_at_a_dead_route`). `osc*.pwm_source`, `osc*.pwm_depth` and
  `filter_env.polarity` stay unfilled.

## The editor
[NOTES.md § The editor, and its brief](NOTES.md#the-editor-and-its-brief)

- Carries the developer channel (`MXM_DEV_CC`): CC 119 category, CC 117 preset browser, CC 118 the
  Bender disclosure (`editor::set_bender`), CC 116 theme, unsaved.
- [`docs/briefs/mxm-mono-02.md`](../../docs/briefs/mxm-mono-02.md) gates the editor. Sizes are
  derived, never copied: `REFERENCE` (`the_opening_size_is_the_budget_hugged` prints the number to
  take when a card changes), `MINIMUM` (`every_dynamic_page_fits_and_every_card_is_reachable`,
  `the_app_bar_holds_in_the_minimum_window`). All seven cards open on one page, one row.
- **One card per oscillator**; *Envelope and amplifier* is one card. **No card has a caption**:
  each sentence is its control's tooltip, for the player, never the machine's history or circuit.
- **Every card is a `mxm_ui::tree`**, exactly as wide as its computed floor; a route stack's floor is
  its widest row, so adding a route never widens it; the Bender is `tree::disclosure`, reserved
  open. Painted names may drop prefixes; host, hover and accessibility text keep the parameter's
  name. `sections::draw` keeps its signature for `apps/mxm-layout-lab`.
- **Volume is in the app bar, not a card**; **routes are drawn beneath what they move**. The scope's
  per-sample stores are skipped while no editor exists (`Telemetry::editor_open`).
- **Layout is measured, not judged**: `every_card_passes_the_tree_checks_in_every_state`,
  `no_card_overlaps_another_or_leaves_the_panel`, `every_row_of_cards_shares_one_bottom_edge`,
  `the_bender_toggle_sits_on_the_range_switchs_cell_line`.
- Keyboard coverage: `REVEAL` opens the Bender before painting, and
  `the_keyboard_cursor_reaches_and_operates_every_parameter` runs **twice, both `Exactly`** — Init,
  then every pair present ([NOTES.md § The keyboard coverage check](NOTES.md#the-keyboard-coverage-check-opens-the-bender-and-reveals-every-route)).

## Presets and public modules
[NOTES.md § `preset.rs`](NOTES.md#presetrs-is-this-instruments-instrument-impl-and-its-factory-set) ·
[NOTES.md § Public modules](NOTES.md#editor-params-routes-and-telemetry-are-public)

- This plugin owns the **fifty factory sounds** in `presets/`, generated from `FACTORY_DESIGN`
  (`the_factory_files_match_the_design_they_were_generated_from`, `every_factory_preset_can_be_heard`).
  **A preset carries the whole routing state, presences included.** Init has no file.
- **Regenerating after a parameter change:** `cargo test -p mxm-mono-02 --lib write_the_factory_presets -- --ignored`;
  `every_factory_preset_covers_every_parameter` fails until it is run.
- `editor`, `params`, `routes` and `telemetry` are `pub` (with `Section`, `SECTIONS`, `title()`) so
  the layout bench draws these real cards; they do not change the shipped `cdylib` or CLAP entry point.

# Work Guidance

# Verification

```bash
cargo test -p mxm-mono-02
cargo test -p mxm-mono-02 --lib every_card_passes_the_tree_checks_in_every_state
# Every page, light and dark, for review -> target/layout-tree/mxm-mono-02/<MXM_PICTURES tag>/
MXM_PICTURES=after cargo test -p mxm-mono-02 --lib tree_pictures -- --ignored
cargo clippy -p mxm-mono-02 --all-targets
cargo xtask bundle mxm-mono-02 --release
cargo xtask bundle mxm-mono-02              # debug, for the allocation assertions
clap-validator validate "target/bundled/mxm-mono-02.clap"
cargo test -p mxm-mono-02-host-tests       # behaviour and golden_audio, through MXM Player
cargo test -p mxm-player --test t5_control_map --test t7_editor
cargo test -p mxm-mono-02 --release --lib baseline -- --ignored --nocapture --test-threads=1   # BASELINE-M0.md; timings count only on a quiet machine
```

The validator’s sample-rate sweep relies on every DSP corner being clamped below Nyquist.

- **In MXM Player**, `host-tests/tests/behaviour.rs` proves the shipped bundle by permanent id (pitch,
  host-written cutoff, low-note priority, HOLD, the pulse-width LFO route) and `golden_audio.rs`
  plays a fixed score, provisionally repinned after the routing conversion; `t7_editor.rs` opens the
  editor for real (`--ignored`). Detail: [NOTES.md § In MXM Player](NOTES.md#in-mxm-player).
- **Not run by a person yet:** the editor by eye at each zoom, the brief's trial, a real DAW,
  listening to the routing conversion, and any comparison against hardware. Fidelity is UNVERIFIED.

# Child DOX Index

No child AGENTS.md files.
