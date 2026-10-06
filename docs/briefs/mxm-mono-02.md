# mxm-mono-02 — UI design brief

Required by `MXM_DESIGN_SYSTEM.md` §14. Answers the ten questions in order, then records the
deliberate deviations and the decisions the plan (`plans/plan-mxm-mono-02.md` §9) hands to this
document. Written with the editor, and measured against it, rather than before it — the plan's
phase 0 and phase 2 were delivered in one pass, and every number below is one a test pins.

*Since the split (2026-10-06):* the design system is mxm-kit's
[`docs/MXM_DESIGN_SYSTEM.md`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/MXM_DESIGN_SYSTEM.md),
`crates/ui` is mxm-kit's too, and the plans cited here are in the private archive.

**Instrument:** a monophonic synthesizer with two free-running oscillators, a sub-oscillator locked
to the first, one shared pulse-width-modulation section, a four-OTA cascade filter with a diode
clamp, one envelope shared three ways, a modulator with a delay that fades its sine only, and a
low-note-priority keyboard block whose trigger fires only from below. Architecture inspired by the
Roland SH-2; the interface is not. **Fidelity is UNVERIFIED throughout** — the constants are read
off the service notes and measured against this crate's own harness, never against a unit.

---

## 1. Primary sound-design task

**Stacking the oscillators, then shaping the stack with one envelope.** The SH-2's whole character
is two saws a few cents apart with the sub under them, and the single envelope pulling the filter,
the amplifier and — through the PWM section — the pulse width at once. So the task is not patching
(mono-00) or performing a filter (mono-03) but **choosing two waveforms and a detune, and deciding
what the one envelope does to them**. The editor is organised so the stack is read left to right
and the envelope's three destinations are each visible where they act.

## 2. The three to five parameters users reach for most

1. **Cutoff** and **Resonance** — the cascade is the voice's centre, and the envelope drives it hard.
2. **VCO-2 tune** — the detune that is the beating, and the interval when the range is wide.
3. **Cutoff from Envelope** — how far the one envelope opens the filter, and by its sign which way:
   the route beneath Cutoff that the machine's envelope amount and its polarity switch became.
4. **Sub** — the level that makes it a bass.

Cutoff and Resonance take **Primary** sizing. VCO-2 tune and the three mixer levels are
**Standard**. Everything else is **Compact** (32 px, value on hover), per §7.1: twenty-five
controls in seven cards, and the tier system is the answer rather than a fourth column. The
twenty-sixth, **Volume**, is the instrument's master output and sits in the app bar as an inline
slider beside the level meter (§3.1 slot 6; the owner's ruling of 2026-09-18 for every instrument).
The 112 routing parameters are rows in the stack beneath the target each moves, not knobs. The LFO
rate's tempo sync (`lfosync`, 2026-09-25) is the collection's quarter note beside Rate.

## 3. Signal flow that must be visible

```
 keys ─► keyboard block ─► portamento ─► auto bend ─┬─► VCO-1 ─► sub ─┐
                                                    └─► VCO-2 ────────┴─► MIXER ─► VCF ─► VCA ─► Volume
 one ENVELOPE ─► VCA (HOLD / ENV / GATE) · route → Cutoff (signed) · route → Pulse width
 MODULATOR ─► route → Pitch (the delay on the sine only) · route → Cutoff · its tap → Pulse width
 AUTO BEND · KEY · BENDER ─► routes → Cutoff and Pitch, as the machine wired them
 any of thirteen sources ─► any of Pitch · Pulse width · Cutoff · Amplitude, at a signed amount
```

Four facts about this machine that the panel hides and emulations get wrong, each of which must
read without a manual:

- **One envelope, three destinations.** The envelope is the only ADSR, and the card it shares with
  the amplifier shows the switch that decides whether it drives the amplifier, and the Filter and Oscillator
  cards show its route beneath Cutoff and beneath Pulse width — present in every fresh instance,
  at zero, because the machine wired both.
- **The trigger fires only from below.** A higher key pressed over a held one is ignored and does
  not retrigger; the Envelope card's trigger switch says so in its tooltip.
- **The delay fades the sine only.** The square and the random output arrive at full depth at once;
  the Delay knob's tooltip says so, because a delay that only sometimes delays reads as broken.
- **HOLD is a drone, and the panic is All Sound Off.** The VCA's three-way switch names
  HOLD, and the voice keeps sounding with no key down until the host asks it to stop.

## 4. Which controls belong in Play view

**Not applicable — no `Play` view**, the answer every sibling gave. The reached-for parameters in
§2 are Primary and Standard on the `Synth` view, which is where a performer plays it.

## 5. Advanced controls and their disclosure

**One in the Voice card, behind its Bender disclosure, and one on each oscillator's card:** bend
range; the VCO-2 tune range switch (narrow, ±200 cents, or wide, ±1750 cents — the plan's reading
of the hardware's two-range tune pot); and the **VCO-1 bender** switch that lets the bender reach
VCO-1. Each defaults to a value that changes nothing about the panel's sound — bend range 2
semitones, narrow, bender on — so a fresh patch is unaffected by never touching them. The
bender's reach on the cutoff, once a fourth, is the (Cutoff ← Bend) route now, present at zero.

**Not disclosed:** the trigger mode and the VCA mode. Each is a switch on the machine's panel and
changes what the module *does*; hiding it would make the card lie about the module. The PWM
source and the envelope polarity were two more, and are routes now: the source is which route is
present, the polarity the sign of its amount.

**A route is disclosed by being present.** Each target's stack draws its present routes as rows
and offers every other source under `‹ modulate ›`. The nine the machine wired are present at
Init, at zero, so the machine's own paths are always on the card.

## 6. Views

**Space-derived pages**, following design-system §3.2. Voice is Performance;
LFO and *Envelope and amplifier* — the Envelope and the Amplifier as one card, the owner's
mxm-mono-01 ruling of 2026-09-24 applied here — are Modulators; Oscillator 1 and Oscillator 2 are Generators;
Mixer/Filter are Tone and a preferred group; former cross-category groups split. Bender stays in
Voice and is measured open. Full category names, no fixed tab count, no bar for one page.
Parameters remains separate at developer CC 119 value 127, without a tab; controller pages stay
unchanged. The filter response remains on the Filter card.

## 7. Identity accent

**Orchid.** Dark `#E07BEA`, light `#8A2E9C`.

Measured with the same WCAG formula `mxm_ui::theme::contrast` implements, against the surfaces it is
drawn on:

| | vs `surface-1` | vs `surface-2` |
|---|---:|---:|
| Dark `#E07BEA` | **6.80 : 1** | **6.22 : 1** |
| Light `#8A2E9C` | **7.11 : 1** | **5.96 : 1** |

Both themes clear 4.5 : 1 for text and 3 : 1 for control boundaries.

**The choice was made on hue separation, and the first choice was wrong.** An aqua (`#52D6E4`) was
taken first, on the reasoning that it sat 20° from `mod-lfo` and `mod-performance` — and it sits
**0.7°** from the collection's default accent (`#4CC9D8`), which is the accent `mxm-mono-01` wears.
An identity that is the default is no identity. Every hue on the wheel was then measured against
every taken one — the four instruments' accents, the default, and the modulation and status colours:

| Candidate | Hue | Nearest taken hue | Dark s1 / s2 | Why not |
|---|---:|---:|---:|---|
| Aqua `#52D6E4` | 186° | 0.7° — the default accent | 10.04 / 9.18 | Indistinguishable from `mxm-mono-01` |
| Sky `#4FC3F7` | 199° | 12° — the default accent | 8.70 / 7.96 | Same neighbourhood |
| Gold `#F2D65A` | 49° | 10° — `warning` | 12.07 / 11.04 | A lit control would read as a clip warning |
| Indigo `#7B8CFF` | 232° | 21° — `mod-lfo` and `mod-key-voice` | 5.84 / 5.34 | Squeezed between two modulation blues |
| Magenta `#F06AD8` | 311° | 25° — rose | 6.44 / 5.89 | A neighbour of `mxm-poly-06`'s rose |
| Spring `#6FE39A` | 142° | 20° — `mod-performance` | 10.90 / 9.97 | Reads as the performance green |
| **Orchid** | 295° | **39°** — `mod-key-voice` and rose | 6.80 / 6.22 | The widest gap left on the wheel, and cold where copper and rose are warm |

Orchid is on `mxm-mono-01`'s candidate list, which two earlier briefs left alone. It is taken here
because mono-01 wears the default today and the wheel has nothing wider left; if mono-01 ever moves
off the default, coral remains on its list.

**§5.3's trade-dress rule is satisfied**: the hardware is a black panel with coloured slider caps
and wooden end cheeks, and this is none of those.

**The plan records this as the owner's choice.** Orchid was taken so the editor could ship; the
owner may pick another passing candidate, and the change is one constant in `crates/ui/src/theme.rs`
and this table.

## 8. Live visualizations

Three, and the test each had to pass is whether it answers a question the controls cannot.

1. **The filter's response, set and actual.** The Filter card draws the cascade's analytic
   magnitude at the slider's position and, faded, at wherever the modulation sum has actually put
   the cutoff — because on this machine the envelope dominates the sum so hard that full depth
   pins the cutoff against the ceiling before the attack ends (wart 12 in the instrument page), and
   a person must be able to see the curve pinned rather than infer it. The analytic curve is a
   **declared approximation**: it ignores the input stage and the diode clamp.
2. **The mixer's output, as a scope.** The Mixer card draws the sum of the three sources before
   the filter, triggered on a rising zero crossing so a periodic wave
   stands still, scaled to its own peak so the shape reads at any level, with §7.5's zero line.
   What the mixer *does* is add waveforms, and three level numbers cannot show two saws beating
   or a sub under them; the trace can. Added at the owner's request on the second look.
3. **Output level with clip indication** in the app bar, per §3.1, measured after Volume — which is
   drawn right beside it, the bar's master output control.

Deliberately **not** included: an envelope display, an LFO display, a scope of the output — the
filter's curve already says what the filter did to the mix.

### Ownership

A single `Telemetry` struct, `Arc`-shared, **atomics only**, written once per block: a **peak that
is max-combined and reset on read**, a **clip that latches** until acknowledged, the actual cutoff
and the envelope level, and the sample rate.

| Visualization | Writer | Truth model |
|---|---|---|
| Filter response | Audio thread, once per block | **Exact** for the cutoff the DSP is using; the curve is the analytic prototype |
| Mixer scope | Audio thread, **every sample**, one relaxed word store into a ring of 2048 | **Exact** samples; a snapshot may straddle a write, which is one sample of tear |
| Output level + clip | Audio thread, once per block | **Exact** |

## 9. What is removed from the source hardware layout, and why

**Kept:** the control set and the signal flow it implies. **Removed:** the panel layout, its slider
geometry, its coloured caps, the wooden cheeks, the typography and wordmarks, and the hardware's
own keyboard.

| Removed | Why |
|---|---|
| The panel layout and its sliders | §2 forbids copying the inspiring instrument's panel; the grouping survives because it is the signal flow |
| The keyboard, the bender lever as a lever | §2 forbids a decorative keyboard; the bender is the host's pitch-bend, its range disclosed and its reach on the cutoff a route |
| The tune pot as one knob with two ranges | Two controls — a bipolar tune and a range switch — say what one pot with a detent hid |
| Black, the coloured caps, wood; the wordmarks | §2 and §5.3 |

**Interface improvements, each a limit of the panel and not of the circuit:** a bipolar VCO-2 tune
with a stated range; the pulse width as a control with its routes beneath it, so the section's
two sources can act at once and over any manual width; the envelope's polarity as the sign of its
route's amount.

## 10. Minimum size and 200% scale

**Resizable: the editor's `REFERENCE` and `MINIMUM`, derived and held by its tests.** Category/card
order is §6's. `every_dynamic_page_fits_and_every_card_is_reachable` checks all pages with Bender
reserved open, both themes, at opening size, the quarter-4K content size and the minimum, at 1×/2×
with that simulated physical budget fixed. Every card's floor is computed from its tree with every
route revealed (`plugins/mxm-mono-02/AGENTS.md`). Component floor/row tests remain separate from
physical fit.

Zoom is independently chosen at **75–200%**; only indivisible overflow scrolls. Keep the physical
window fixed for §15's DPI/zoom gate. Native-window, real-DAW and owner inspection remain open.

---

## 10a. Coherence with the sibling editors

| Taken | Why |
|---|---|
| **`SECTIONS` as a `const` array, with the order stated as the contract** | It is the information architecture |
| **`mxm_ui::ModuleCard` per section**, names from the shared vocabulary | §3.3's grouping, already themed |
| **`knob_row`: columns of their own width; a switch beside a knob on the knob's grid** | The siblings' answer to knobs spread across a card |
| **One place brackets gestures** — `binding::Bound::apply` | Load-bearing for the player's step editing. The fifth copy of `binding.rs` |
| **A `Parameters` view, and Init in the app bar's patch actions** | Same placement |
| **`Telemetry` as the only DSP → editor channel** | Same rules |
| **The zoom control, 75–200%** | One size that is always right, absorbed by zoom rather than reflow |
| **A filter response display** | `mxm-mono-01`'s idiom, with the actual cutoff added |

**Where it deliberately differs**: the Envelope and the Amplifier are one card (the owner's
mxm-mono-01 ruling, 2026-09-24), and each oscillator has its own card (the owner, 2026-09-27:
one Oscillator card for both was absurdly tall) — *Oscillator 1* with the Master tune and the Pitch
routes, *Oscillator 2* with its Tune beside the shared Pulse width and the Pulse width routes, which
start empty. Two views, seven cards.

---

## 11. Decisions the plan handed to this brief

| Decision | Standing |
|---|---|
| **The identity accent** | **Orchid, measured above**, after the aqua collision. The owner's to change |
| **The `Synth` view's height** | **896, measured with the expander open**, not the 760 target |
| **The VCO-2 tune range switch** | Disclosed in the Oscillator 2 card rather than hidden: it changes what the tune knob means |
| **The mod wheel** | Pushes the (Pitch ← LFO) route's amount at play time and writes no parameter — the legacy push, kept through the routing conversion — and is a Wheel source beside it. There is no wheel control on the panel, by §4 of the control map |
| **A standalone harness** | Not built; the player's `Show editor` is the harness |

---

## 12. The recognisability trial

§9 makes a recognisable *control set* the requirement — not a recognisable panel, which §2 forbids.

**The trial patch:** the `Beating saws` factory preset — both saws, VCO-2 a few cents sharp, the
sub up, the envelope on the filter — with a key held.

Run with **someone who has used an SH-2 or an SH-101**, without showing them the hardware.

### Stage 1 — before composition, on a wireframe

1. *"How many envelopes does this have, and what does the one drive?"* → **one**, and the answer
   names the filter, the amplifier and the pulse width.
2. *"What happens if I press a higher key while holding a lower one?"* → **nothing**: the lower key
   keeps sounding and nothing retriggers.
3. *"Which LFO output does the delay fade?"* → **the sine only.**

### Stage 2 — on the finished editor

| Task | Control |
|---|---|
| *Make it darker.* | `cutoff` |
| *Make the two oscillators beat faster.* | VCO-2 tune |
| *Make it a bass.* | `sub` |
| *Let it drone without a key.* | the VCA switch, to HOLD |
| *Make the vibrato come in late.* | LFO delay |
| *Turn the whole thing down.* | `volume` |

**Per task:** found within ten seconds, on the `Synth` view. **Gate: five of six.**

*Results: not yet run.* An unrun trial is recorded as unmet, never as passed.

---

## Deliberate deviations from the design system

### ~~A one-line caption under the controls it explains~~ — withdrawn (§7.6)

The captions went when the owner ruled out help text on the panel (2026-09-27). Each fact is its
control's tooltip, written for the player: the trigger firing only from below (Trigger), HOLD
sounding with no key down (the amplifier mode), the delay fading the sine only (Delay).

### No undo/redo (§3.1)

The only undoable events are parameter edits, which hosts already track.

### No audio input

The machine's external input — its jack into the mixer and the envelope follower it fed — is not
built (the owner, 2026-09-26): the plugin advertises stereo and mono with no input of any kind, and
the Follower source and its routes went with the jack.

---

## The fidelity gate

**UNVERIFIED.** No unit was measured. The filter's onset of self-oscillation, the saw's discharge
corner, the sine's rounding and the delay's fade are read off the service notes and pinned by this
crate's own harness, and `research:filters/machines/ba662-sh-2.md` names the measurements worth making
if a unit appears.

---

## Sign-off checklist

- [x] Signal flow readable without documentation: **one envelope, three destinations**, **the
      trigger from below**, **the delay on the sine only**, **HOLD as a drone** — by the cards'
      switches and tooltips; not yet judged by a person
- [x] Cutoff and Resonance at Primary sizing, adjacent
- [x] Every disclosed control changing nothing at Init
- [x] Identity accent applied, with the measured ratios above holding against `surface-2` as well
- [x] Every parameter present, on exactly one card, Volume on the app bar's —
      `every_parameter_is_drawn_at_most_once_and_exactly_once_when_revealed`,
      and painted where the keyboard reaches it with every route revealed —
      `the_keyboard_cursor_reaches_and_operates_every_parameter`
- [x] Card names match the collection's vocabulary
- [x] Both views reachable, `Synth` active on open
- [x] The `Synth` view's height measured and pinned by a test rather than assumed, and every card measured inside its column
- [ ] Verified by eye at 75%, 100%, 150% and 200%
- [ ] Dark and light both complete, with all control states
- [ ] §12's trial run and recorded
- [ ] §15 QA gate passed in full
