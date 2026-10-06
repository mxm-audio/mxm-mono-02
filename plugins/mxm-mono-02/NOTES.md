# NOTES.md — plugins/mxm-mono-02

The detail behind this folder's AGENTS.md: history, measurements, rationale and worked examples. AGENTS.md is the contract; this file is the reference it links to.

## Permanent identifiers

**131 host parameters**: twenty-seven controls and 104 routing parameters, where there were
thirty-five. `lfosync` (2026-09-25) is the newest: the LFO rate's tempo sync, the collection's
quarter note beside Rate on `params::LFO_SYNC`, resolved once a buffer by
`MxmMono02Params::synced_lfo_rate` (`plans/plan-tempo-sync-controls.md`). At 120 bpm this LFO's
0.2 Hz floor holds two bars but not four, so the knob's bottom is two bars there. Treat all of it as
public interface. `parameter_ids_are_unique`,
`the_id_table_is_what_the_derive_actually_produces`, `no_retired_id_reappears_among_the_parameters`
and `every_parameter_is_drawn_at_most_once_and_exactly_once_when_revealed` keep the lists, the derive
and the editor agreeing.

## The machine's modulation is routing, and its wiring is the init patch

`routes.rs` declares a presence and a signed amount for every *(target, source)* pair — four targets
and thirteen sources, `mxm-mono-01`'s shape — and the DSP's `routing` module evaluates them. Nothing
asks whether a route is the machine's own. **The eight paths the SH-2 hard-wired that are still here**
— (Pitch ← LFO), (Pitch ← Auto bend), (Pulse width ← LFO to width), (Pulse width ← Envelope),
(Cutoff ← Envelope), (Cutoff ← LFO), (Cutoff ← Key) and (Cutoff ← Bend), the DSP's `MACHINE` — keep
the reach their controls had. **Six are present in the init patch at zero depth; the pulse width's
two are offered, not shown** (the owner, 2026-09-27: the Pulse width stack starts empty), and a
design that uses one switches it on (*Pulse sweep*, *Pulse lead*, *Choir*). A route at zero renders
as an absent one, so a fresh instance is the machine either way: all 51 bank digests held through
the change. `the_init_patch_wires_six_of_the_machines_routes_at_zero_depth` here, and the bit
identity of Init's six and all eight in the DSP. The ninth, (Cutoff ← Follower), went with the
external input.

**A route's amount reads what its pair delivers**, in the target's own unit — semitones, a percentage
of width, octaves, and per octave of key for a Key route — so the machine's own routes read the
retired controls' numbers at full: +7 st, −12 st, −45 %, +10 and −2.75 oct, +4, +1.20 oct/oct and
+2 oct; a route the SH-2 never had reads the collection's standard reach, +12.00 st or +4.00 oct.
`a_route_reads_what_its_pair_delivers_and_reads_back` holds each, and that a typed reading
lands back on the amount it came from. Every amount is the collection's one route parameter,
`mxm_modulation_params::reading::amount_param` — the envelope's shorter negative half a `Reach` whose
`below` is the inverted reach — and `every_route_parameter_says_what_the_dsp_does` holds every
pair's travel and reading to `mxm_mono_02_dsp::conformance` (`mxm_plugin_test::routing_checks`).
**It never prints a negative zero**: its number goes through `mxm_modulation_params::signed`, and
`every_reading_survives_the_hosts_round_trip_a_rounded_zero_included` sends every amount through
the host's own conversion either side of zero, where a plain signed format printed `-0`, which
`clap-validator`'s `param-conversions` fails whenever its random values land there.

**Nine ids retired for these**, each an owner decision under the governing plan's decision 1.13, and
every state they could reach is still reachable (`crates/mxm-mono-02-dsp/tests/legacy_reachability.rs`)
but the envelope switch's ENV FOL'R, which went with the external input.
What changed on the way, deliberately:

- **The auto bend's depth is continuous.** The retired control was latched at the trigger, so a held
  depth renders as before and a depth automated mid-dip does not.
- **The bender reaches the cutoff from the lever**, independent of `bendrange`, as the hardware's two
  sensitivities are (plan N1). The old `bendfilter` *b* at bend range *r* is a (Cutoff ← Bend) amount
  of *b × r ÷ 24*.
- **The factory designs were translated** by plan §4's table and regenerated; a signed amount is
  stored as `(a + 1) / 2`.

**Decision, X1 — the owner, 2026-09-15: no `filter_state` is built.** A project or host state saved
before the conversion keeps every surviving parameter and **loses what the nine retired ids held**,
and an automation lane on a retired id is not carried — as on `mxm-mono-00` and `mxm-mono-pr1`.

**The Follower's eight route ids retired the same way** (2026-09-26), when the external input it
followed was removed: a state or preset that names them loads everything else, and those are
skipped — the wrapper ignores an id it does not have, and `mxm-preset` reports and skips one.
`a_state_naming_the_retired_follower_routes_still_loads` (the player, through the real CLAP state)
and `a_preset_naming_the_retired_follower_routes_loads_everything_else` hold the two paths.

**Once per buffer, then once per sample**, through `begin_interval` and `render_sample` in `lib.rs`,
which `process()` and the measurement seam `render_block_for_test` both call:

- `Routes::topology_from` builds the topology against last buffer's and **snaps each newly present
  route's smoother** to its stored depth —
  `a_re_added_route_arrives_at_its_stored_depth_rather_than_ramping_from_a_stale_one`.
- `Voice::set_topology` clears a source that has just become read, which is the DSP's contract.
- `Routes::settle` reads **each settled route's amount once** and lists the ramping ones. nice-plug's
  `Smoother::next` moves nothing once a ramp is done, so the read is the value every sample would have
  read, to the bit — `settled_routes_read_once_per_interval_match_advancing_every_sample`. The host's
  changes land between buffers (`SAMPLE_ACCURATE_AUTOMATION` is off); an editor edit the wrapper
  applies to a settled route mid-buffer starts its ramp at the next buffer.
- `Routes::advance`, per sample, advances each ramping route and applies the mod wheel's push.
- `a_route_arriving_after_an_idle_span_is_block_partition_invariant` renders that path at 64, 37 and
  1024 samples a block, with a route arriving mid-ramp after an idle span.

**What the routing costs is not yet measured**: `BASELINE-M0.md` says why, and how to measure it.

## Every parameter's text survives the host's round trip

A host parses a parameter's text and **normalises the number before printing it again**, so the
value it prints lands a hair either side of where it started. A formatter that switches unit or
precision at the raw value is not idempotent there, and `clap-validator`'s `param-conversions` fails
only when its values land in that sliver. So in `src/params.rs` `v2s_time` chooses its unit from what
the millisecond text would round to — anything that would read `1000 ms` prints seconds — and no
reading prints a negative zero: `without_negative_zero` decides it from the text, under VCO-2 tune's
percentage and Master tune's whole cents. **Master tune cannot use nice-plug's `v2s_f32_rounded(0)`**: it tests
for zero with `f32::round`, which takes an exact `-0.5` to `-1`, then prints with `{:.0}`, which
takes it to the even `-0`, and a host reaches `-0.5` cents exactly.
`params::tests::every_parameter_reads_the_same_after_the_hosts_round_trip` holds all 131, converting
as the wrapper does, with the unit: at the validator's grids, either side of each branch point, and
at every representable normalised value near each. Attack, Decay, Release, LFO delay, Portamento,
Tune and VCO-2 tune failed it before the fix.

## No audio input

`AUDIO_IO_LAYOUTS` advertises two layouts, stereo then mono, and **neither has an input of any
kind**. The SH-2's external input — an auxiliary port in two further layouts, its fixed-level path
into the mixer and the envelope follower it fed — was removed by the owner on 2026-09-26, with the
Follower source and its routes (above). `process()` reads no auxiliary buffer.

## Activation refuses a rate the DSP cannot hold

`activate` returns `false`, before anything changes, for a non-finite host rate or one below
`mxm_mono_02_dsp::MIN_SAMPLE_RATE`, 1 kHz: a NaN rate, or one low enough for a corner's floor to
cross 0.45 of it, panicked on the audio thread.
`activation_refuses_a_non_finite_rate_and_any_below_the_floor` holds the refusal, and
`the_rate_floor_activates_and_plays_at_every_parameter_extreme` a held note at the floor with every
parameter at its default and at either end.

## Performance state follows the sounding press

Pitch bend, the mod wheel and **channel pressure** are kept **per channel, sixteen of each**, on the
plugin. Which channel's values reach the voice is decided from `voice.owner().channel` — the press the
keyboard block is sounding, or the last one it sounded — so a release tail or a HOLD drone bends with
the channel that played it, and a press the block ignored moves nothing.
`bend_and_wheel_follow_the_sounding_press_and_an_ignored_press_moves_nothing` pins it. **Velocity**
rides the press itself in the keyboard block, so a fallback to an older press brings back that press's
velocity with its channel — `velocity_and_pressure_follow_the_sounding_press_through_a_fallback`. The
lever, the wheel, pressure and velocity are each a routing source, published by the collection's
standard — and the Velocity source is the press that last **triggered the envelope**, which the voice
holds, not the owner's (`crates/mxm-mono-02-dsp/AGENTS.md`). `bendrange` is **smoothed** (20 ms):
it scales the held bend into pitch, so a range edit under a held bend is a ramp rather than a step —
`docs/code-review-notes.md` §2, and the owner's ruling of 2026-09-15.
`a_range_edit_under_a_held_bend_ramps_rather_than_steps` holds it.

**The keyboard block lives in the DSP.** Every note event passes `voice_id`, `channel`, `note` and its
velocity through, and the block matches releases, chokes and tuning expressions against the presses
it holds. The machine's keyboard had no velocity; here it is a source, and nothing else reads it.
**The mod wheel's legacy push stays** (plan N4): at play time it is added into (Pitch ← LFO)'s amount
**whichever way that amount points** — a negative route is partly cancelled, and the depth stays
continuous through zero (`the_wheel_push_adds_the_same_whichever_way_the_route_points`) — clamped to one, and writes no parameter — what the plugin's wheel always did to the vibrato depth, so
a wheel at zero is exactly the stored depth. The machine had no wheel; this is the interface addition
the README discloses, and the raw wheel is a Wheel source beside it.

## The process status is the voice's activity

`Voice::tail_samples(release, vca_mode)` decides: `None` while the voice is live — a key down in
any mode, or a HOLD drone — and `process()` answers `KeepAlive`; a finite tail answers `Tail(n)`,
and it is the **audible** tail: the envelope's release in ENV mode, the gate's slope in GATE mode
(the envelope runs on behind the closed amplifier and is not the tail), plus the settle the
activity verdict waits for; an idle voice answers `Normal` and renders exact zeros. **HOLD is the second
instrument in the collection that sounds with no key down**, and the player's `KeepAlive` path is
what keeps it sounding; `plugins/mxm-mono-02/host-tests/tests/behaviour.rs` proves it through the player.

## Three terminations, and the HOLD panic latch

Note-off releases the press it names; **All Notes Off** releases every press; **All Sound Off**
(CC 120) cuts the voice and, in HOLD, **latches it silent** until a note-on, a host reset, or
re-entering HOLD — the plan's ruling that a panic over a drone must stick. Choke is a note-off for
the voice it names. All three are the DSP's; the shell only maps the messages.

## Smoothed and unsmoothed

Every route amount, because it multiplies a signal, and the levels, the cutoff, the resonance, the
sustain, the pulse width, the two tunes and the volume are smoothed (10 ms linear); the bend range at
20 ms, because it scales a held bend. Presences, envelope times, the LFO rate and delay, every switch
and the ranges are not — a configuration is not a signal.

## The control map claims the trigger role it added

`control-map.json` fills thirty roles, `amp_env.trigger` among them — the role appended to
the standard's Amp page for this instrument. It is claimed rather than withheld because the role
and the instrument entered the standard in one change, so no player compiles in one without the
other; `docs/MXM_CONTROL_MAP.md` §9 records the rule. **Every depth role names the amount of a route
Init wires**, so its knob is live on a fresh instance — four routing ids for five roles, because the
filter's LFO amount and the LFO's filter depth both name (Cutoff ← LFO);
`a_control_map_role_never_points_at_a_dead_route`. The Osc 2 page's width slot names the same
parameter as the Osc 1 page's, because the width is shared. `osc1.pwm_source` and `osc2.pwm_source`
are unfilled — the switch is gone, its two positions being two routes — and so are `osc1.pwm_depth`
and `osc2.pwm_depth`, since Init routes nothing to the pulse width (2026-09-27), and
`filter_env.polarity`, which is the sign of the envelope route's amount.

## The editor, and its brief

Carries the collection's **developer channel** (`plugins/AGENTS.md`, *A developer channel in every
editor*): with `MXM_DEV_CC` in the process environment, CC 119 selects a category (0–5) or Parameters (127), as defined by the parent, CC 117 opens and closes the preset browser and CC 118 opens
and closes the Bender disclosure, writing the state it reads: `mxm_ui::shell::disclosure_id("Bender")`
(`editor::set_bender`). CC 116 sets the theme by index — 0
light, 1 dark, 2 system — without saving it.

[`docs/briefs/mxm-mono-02.md`](../../docs/briefs/mxm-mono-02.md) gates the editor.
The opening size is the quarter-4K budget hugged (`REFERENCE`). The minimum holds one widest card
plus its gutters (`MINIMUM`, exercised by `every_dynamic_page_fits_and_every_card_is_reachable`), and
the app bar at its last compact step is wider and sets it (`the_app_bar_holds_in_the_minimum_window`).
`page_items` keys 0–6 are the cards' positions in `SECTIONS`: Voice is Performance; LFO and
*Envelope and amplifier* are Modulators; Oscillator 1 and Oscillator 2 are Generators; Mixer/Filter
are Tone and a preferred group (keys 4 and 5). All seven open on one page, one row.

**One card per oscillator** (the owner, 2026-09-27: one Oscillator card for both was *absurdly
tall*), balanced as the owner chose: *Oscillator 1* holds VCO-1's range with its bender switch,
its wave, the **Master tune** (`tune`, which moves both oscillators; its host name is *Master tune*)
and the Pitch stack, which moves both too; *Oscillator 2* holds VCO-2's range and tune range, its
wave, its **Tune** beside the **Pulse width** both pulses share, and the Pulse width stack. The
split took the editor from 1583 × 945 to 1886 × 618.

**The envelope and the amplifier it drives are one card, *Envelope and amplifier*** — the owner's
mxm-mono-01 ruling (2026-09-24), applied here in R2 (`plans/plan-editor-standard.md` A4): hugged, the
Amplifier was one switch and a stack. **No card has a caption** (the owner, 2026-09-27; design
system §7.6): each sentence is its control's tooltip, written for the player — what the control does
to the sound, never the machine's history or its circuit (2026-09-27: *the modulator*, *un-delayed
sine*, *always in circuit* went) — and each button of a row has its own. The opening size is derived:
`the_opening_size_is_the_budget_hugged` prints the number to take when a card changes.

**Every card is a `mxm_ui::tree`** (`crates/ui/AGENTS.md`, *A card body as data*).
`sections::card` describes a card's body once and `sections::paint` draws each leaf through the same
bindings; the paged view is `paging::editor::show`. The gaps are the hand layout's: the body's
`SPACE_3` rhythm, with the `add_space` it put on top as pads.

- **Floors are computed.** `page_items` takes each card's floor from its tree every frame, and each
  card is exactly as wide as that floor: its ceiling is its floor (`plans/plan-editor-standard.md`
  A1), and no card declares a usability minimum (A2). A route
  stack's floor is its widest row at its widest reading, present or not
  (`mxm_modulation_params::ui::stack_size`), so a card carrying one never widens when a route is
  added.
- **A knob row is the collection's `mxm_ui::tree::knob_row`**, at `mxm_ui::control::knob_column`:
  its columns take what they are offered up to the cap and shrink below it only as far as the
  widest knob allows.
- **The displays state their size**: `visuals::SCOPE_HEIGHT` and `visuals::HEIGHT`, each
  filling the card's width with no minimum of its own. Painted names drop the LFO's prefix (*Rate*, *Delay*, *Shape*,
  `sections::panel_label`); on its own card an oscillator's controls drop their *VCO-1 …* and
  *VCO-2 …* prefixes (*Range*, *Wave*, *Bender*, *Tune range*, *Tune*) — the painting only: the
  host, the hover text and the accessibility tree read the parameter's name. *Bender* rather than
  *VCO-1 bender* is what keeps the seven cards inside the quarter-4K width on one page. A switch's cells are its parameter's own option text.
- **VCO-1's bender switch stands on the range switch's cell line** (`Anchor::CellLine`): the label line
  and the gap under it, the drop `the_bender_toggle_sits_on_the_range_switchs_cell_line` measures.
- **The Bender is `tree::disclosure`**, the collection's, as mxm-mono-00's and mxm-mono-01's
  Advanced: the card reserves its body open, so opening it never moves the card's width or height.
- `sections::draw` stays for `apps/mxm-layout-lab`, with its signature: it builds the section's tree
  and shows it.
- `every_card_passes_the_tree_checks_in_every_state` runs `mxm_plugin_test::tree_checks` over every
  card at Init, with the Bender open, with every route revealed at full negative depth (Bender
  closed and open), and with a note sounding — a scope with a wave in it and the modulated cutoff
  away from the set one.

**Volume is in the app bar, not a card** (owner, 2026-09-18: every instrument's master output is in
the app bar; design system §3.1 slot 6). It is an inline slider between the level meter and the zoom
control, drawn through `mxm_ui::navigation::bar_card` under `VOLUME_CARD` (64, outside the paging keys
0–5), and the cursor runs through `navigation::paged_with_bar`. *Envelope and amplifier* holds the
VCA mode switch and the Amplitude stack. Its id, range, default and smoothing are
unchanged. `every_parameter_is_drawn_at_most_once_and_exactly_once_when_revealed` holds that the bar
card draws Volume and nothing else, and that no page card draws it.
`every_dynamic_page_fits_and_every_card_is_reachable` checks all pages in both themes at opening,
quarter-4K content and minimum sizes at 1× and 2× with the simulated physical budget fixed;
tall component canvases retain floor/row checks, not physical
fit claims. The identity accent is `mxm_ui::theme::ORCHID`, measured in the brief’s §7.

**Routes are drawn beneath what they move**, by `mxm_modulation_params::ui::stack`: a Pitch stack under
Oscillator 1's Master tune and a Pulse width stack under Oscillator 2's knob row, a Cutoff stack in the
Filter card and an Amplitude stack on *Envelope and amplifier*. A present route is a row named by its
source under its target's heading; an absent one is offered under `‹ modulate ›`. The retired PWM
source and envelope switches took the two long rows of their own they needed with them.

`editor/visuals.rs` draws the filter's analytic response at the set cutoff and, faded, at the
actual cutoff the DSP publishes, so the envelope pinning the cutoff against the ceiling is visible;
and the **mixer's output as a scope**, from a ring of 2048 samples the audio thread fills with one
relaxed store per sample — the one thing telemetry publishes per sample rather than per block,
because a waveform cannot be summarised — and the one telemetry write gated on the editor:
`Telemetry::editor_open`, set by the editor's lifecycle and read once per block, skips the per-sample
stores while no editor exists (`plugins/AGENTS.md`: visualization work stops when hidden). The scope
triggers on a rising zero crossing and scales to its own peak. **Layout is measured, not judged**: `no_card_overlaps_another_or_leaves_the_panel`,
`every_row_of_cards_shares_one_bottom_edge` (a row is as tall as its tallest card and stretches the
rest to meet it, Bender closed and open — `mxm_ui::flow::cards`, the collection's rule; the first
build filled every last card to the panel's bottom instead, and the build after that levelled three
columns) and
`the_bender_toggle_sits_on_the_range_switchs_cell_line` (through `egui_kittest`'s accessibility
tree) each pin a finding from the owner's first looks.
`editor/binding.rs` re-exports `mxm_preset::binding`, the collection's one binding (2026-09-24); its
`slider_inline` draws the app bar's output, reserving the widest reading so the bar holds still under
a drag.

## The keyboard coverage check opens the Bender and reveals every route

The parent's *The keyboard cursor runs in every editor* owns the contract. What is local: `REVEAL`
opens the Bender (`editor::set_bender`) before painting, because the bend range lives behind the
Voice card's expander and a control that is not painted cannot join the registry; and
`the_keyboard_cursor_reaches_and_operates_every_parameter` runs **twice, both `Exactly`** — the init
patch, then every pair present — because an absent route draws nothing. Volume registers from its
app-bar bar card on every frame, whichever page card is requested.

## `preset.rs` is this instrument's `Instrument` impl and its factory set

The system is `crates/mxm-preset`. This plugin owns the **fifty factory sounds** in `presets/`, each
with its category, generated from `FACTORY_DESIGN` in `preset.rs`’s test
module (`write_the_factory_presets`, `#[ignore]`d; `the_factory_files_match_the_design_they_were_generated_from`
catches a stale file), and `every_factory_preset_can_be_heard`, which asks that a mixer level be
up, the volume be up, and the cutoff or the envelope's route open the filter. **A preset carries the
whole routing state, presences included**, because `Instrument::parameters` chains the routes after the
controls. Init has no file.

## `editor`, `params`, `routes` and `telemetry` are public

They are `pub`, with the `Section` enum, its `SECTIONS`, `title()` and the card grouping the flow
reads, so `apps/mxm-layout-lab` (`apps/mxm-layout-lab/AGENTS.md` in the private archive) can draw **these real cards**
on its bench instead of copying the section code, which would then drift.

The same section data feeds this editor’s paging renderer and the layout bench. The public modules
are an in-repository test seam; they do not change the shipped `cdylib` or CLAP entry point.

## In MXM Player

**In MXM Player:** `plugins/mxm-mono-02/host-tests/tests/behaviour.rs` loads the shipped bundle
through the player's own hosting path and proves a note at its pitch, a host-written cutoff, the
lower key winning and the higher returning on its release, HOLD sounding with no key down and
the envelope mode ending it in exact silence, and **the pulse-width section's LFO route switched on
and raised by the host sweeping the pulse** — the conversion's headline gesture by permanent id. The
golden's score switches that route on too, since Init stopped showing it: the patch it pinned.
`plugins/mxm-mono-02/host-tests/tests/golden_audio.rs` plays a fixed score through the same path, pinned before the routing
conversion and provisionally repinned after it. `t7_editor.rs` opens the editor for real (`--ignored`).
**Not run by a person yet:** the editor judged by eye at each zoom, the brief's trial, a real DAW,
listening to the routing conversion (its plan's M5), and any listening comparison against hardware.
Fidelity is UNVERIFIED.
