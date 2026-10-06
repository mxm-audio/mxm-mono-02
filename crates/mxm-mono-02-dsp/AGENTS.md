# AGENTS.md — crates/mxm-mono-02-dsp

Parent: [`../../AGENTS.md`](../../AGENTS.md)

# Purpose

The mxm-mono-02 voice as plain Rust: keyboard block, calibrated filter, oscillators, envelope,
modulator, auto bend, portamento, and the modulation routing that wires them. Framework-free, so the
whole signal path is testable with `cargo test` and no host involved.

**The instrument is a monophonic two-oscillator synthesizer inspired by the Roland SH-2**;
`research:instruments/sh-2.md` is the hardware it copies and
`research:filters/machines/ba662-sh-2.md` its filter.
This document owns the current DSP contract. Detailed design/review evidence remains in
`plans/plan-mxm-mono-02.md` (`plans/plan-mxm-mono-02.md` in the private archive),
`plans/plan-mxm-mono-02-modulation.md` (`plans/plan-mxm-mono-02-modulation.md` in the private archive) and
`plans/reviews-mxm-mono-02/` (`plans/reviews-mxm-mono-02/log.md` in the private archive). The hosting plugin is
[`plugins/mxm-mono-02`](../../plugins/mxm-mono-02/AGENTS.md).

# Ownership

Owns `src/` (`lib.rs`, `keyboard.rs`, `filter.rs`, `oscillator.rs`, `envelope.rs`, `lfo.rs`,
`voice.rs`, `routing.rs`), `tests/` (`routing.rs`, `legacy_reachability.rs`), `examples/`
(`mono_02_filter_spike.rs`, `sh2_demo.rs`; the hand-written `common/wav.rs` that `sh2_demo` used
retired to `mxm-measure`'s encoder, and that encoder to `mxm-audio-file`) and `Cargo.toml`.

Does **not** own parameter definitions, ranges, smoothing or the editor — those belong to
[`plugins/AGENTS.md`](../../plugins/AGENTS.md). This crate takes plain values, a routing and a sample
rate.

# Local Contracts

## The keyboard block is the instrument's feel, and it keeps presses, not pitches

`keyboard.rs` is the SH-2's bus-bar keyboard and trigger logic (`sh-2.md` §5): **the lowest held
key sounds; the trigger fires on the gate's rising edge and on every new *lower* key, and on
nothing else.** A higher key over a held lower one produces no event at all — not a pitch change,
not a trigger, not a change to the held key's expression. The voice wires the one trigger to its
three consumers (the envelope in GATE+TRIG mode, the LFO delay's restart, the auto bend's
restart), so the asymmetry reaches all three.

The rules the review made explicit, each pinned by a test named beside it:

- **Presses, not pitches.** The player emits a tie's new note before the old one's release, so two
  presses of one key are held for a moment. Among equal keys the *older* sounds; the old press's
  release hands over with no trigger and no gate change; a per-note expression addressed to the
  new press lands on it and takes effect at the handoff.
  `a_same_pitch_tie_hands_off_between_two_presses_without_a_gap`;
  `a_stack_keyed_by_pitch_would_fail_the_tie` is the sabotage as a property.
- **An id is authoritative when both sides have one; otherwise the key decides, and an id-less
  release retires the *oldest* press of that key** — `mxm-poly-06-dsp`'s rule, for the same
  reason: the tie's old press is the one being released.
- **A non-finite expression is refused**, and the press keeps the offset it had: a NaN in the pitch
  sum would stay in both VCOs' phases. `a_non_finite_expression_is_refused_and_the_pitch_stays_finite`.
- **Bounded, with exhaustion as a policy.** `CAPACITY` presses; when full, a new press evicts the
  oldest press that is not sounding, and the sounding press is never evicted by a press, so its
  release always finds it. A release for an evicted press is dropped.
  `the_block_survives_exhaustion_without_a_stuck_note`,
  `exhaustion_never_evicts_the_sounding_press_even_when_it_is_the_newest`.
- **The owner outlives the press.** The key CV holds the last note after every release, and so
  the `Owner` — key, channel, velocity, per-note expression — stays the last sounding press's until a
  new press sounds, and is a voice-supplied default before the first press since `reset`. A release
  tail or a HOLD drone bends with the channel that played it and keeps its offset.
  `the_owner_outlives_its_press_and_starts_as_the_default`. **Velocity rides the press**, so a
  fallback to an older press brings back that press's velocity with its key and channel; the
  machine's keyboard had none. **The Velocity source is not the owner's, though**: by the
  collection's modulation standard it is the press that last triggered the envelope, held by the
  voice — a legato press in GATE mode keeps the phrase's, and before any press it rests at full so a
  HOLD drone is untouched (`velocity_is_the_press_that_last_triggered_the_envelope`).
- **Three ways of ending a note are three outcomes.** A note-off falls back without a trigger and
  closes the gate when nothing remains; **All Notes Off** releases everything at once and is never
  a panic (the voice's All Sound Off is not this block's business); a **choke** is a release that
  also carries `cut` when nothing remains, so the voice stops without its tail.
  `choke_is_a_release_with_others_held_and_a_cut_alone`.

**Sabotaged before trusted:** with the priority flipped to highest-key, three tests fail
(`a_higher_key_over_a_held_lower_key_does_nothing`, `a_lower_key_takes_over_with_a_trigger`,
`releasing_a_higher_held_key_changes_nothing_audible`). Run 2026-09-03 and reverted.

## The filter is mono-01's diode-clamped ladder with this machine's constants, and its calibration is the machine's

`filter.rs` is `crates/mxm-mono-01-dsp/src/filter.rs` copied whole — the TPT one-poles, the
per-sample Newton solve, `diode_clamp` as the loop's only nonlinearity — with the text kept
verbatim so the extraction plan can diff it. `research:filters/machines/ba662-sh-2.md`
is why that is the right model. Four constants differ, and one thing is added, each with its
reason beside it:

| Constant | mono-01 | Here | Why |
|---|---|---|---|
| `K_MAX` | 4.5 | **5.0** | **Calibrated to the machine**: the service notes put self-oscillation's onset between 7 and 9 on the RESONANCE scale; with the threshold at `k` = 4.00 the onset lands at `4 / K_MAX` = 0.80, the middle of the band. `calibration_puts_self_oscillation_between_7_and_9` measures it on the running filter at 40 Hz, 1 kHz and the top of the range, at four rates: **0.79–0.82** |
| `CUTOFF_MIN_HZ` | 20 | **10** | The hardware's published floor; the procedure has the filter oscillate below 50 Hz |
| `INPUT_KNEE` (new) | unity `tanh` drive | **8.0** | The OTAs sit behind 40 dB attenuators on bipolar rails and run linear; the stage bounds late for the numeric contract, not as the machine's drive. A full-scale signal is compressed 0.05 dB (`the_input_stage_bounds_late_and_the_stages_do_not_saturate`). Chosen |
| `EXCITATION_THRESHOLD` | 0.9 | **0.75** | Must sit below the calibrated onset with a margin, or a filter that would sing waits for a signal |
| `STAGE_SPREAD` (new) | none — one `G` for four stages | **±2 %**, a fixed seeded trim per stage | **Wart 16**: the four BA662s were matched to a paint-dot band, not made identical, and the integrating capacitors were not matched at all. `mxm-poly-06-dsp`'s form — the cascade's input gain and constant term become products over four `G`s. **Chosen**: the one spread whose effect the research has measured (`ir3109-roland.md` §10). `Ladder::new()` is this unit; `Ladder::matched()` the four-identical-stages reference the spread is measured against |

`CLAMP_KNEE` stays 1.0, chosen as mono-01's is; the SH-2's own knee is unmeasured and the two
bench measurements that would fit it are in the deep-dive's §9.

**What the spread does, measured** (`this_units_stage_spread_moves_the_peak_and_not_the_onset`,
48 kHz, cutoff 1 kHz): this unit's trims are 1.0079 / 1.0082 / 0.9856 / 0.9817; the resonant
peak at resonance 0.7 sits **0.62 dB below** the matched reference's (5.40 against 6.02 dB) and
1.5 dB below it at `k` = 3.83; the onset on the control is **0.7995 against 0.7994** — the
deep-dive's §10 finding, that a stage spread moves how much the filter resonates and not where it
sings, holds here. The test bounds the move: more than 0.02 dB, under 3 dB. **Sabotaged**: with
`STAGE_SPREAD` at zero the test fails (2026-09-03). **The clamp's shape test runs on
`Ladder::matched()`**, deliberately: it reads the saturator through the peak gain
`1/(4 − k)` at a fixed `k`, so a spread that moves the peak moves its operating point and its
numbers stop being comparable with mono-01's; the spread is tested on its own.

**The top of the range at 44.1 kHz is the DSP's ceiling, not the hardware's 20 kHz** — a
labelled deviation (plan §5.3): `NYQUIST_FRACTION` caps the cutoff just under 20 kHz there, the
calibration test measures at whichever is lower and reports which. **That clamp is also what holds
the filter's coefficients under audio-rate routes into the cutoff**: with the voice's octave clamp
removed alone every routing test stays green, and with both removed the resonance loop runs away
(the plan's mutation D16).

**Measured by `mono_02_filter_spike` (release, 2026-09-03, on this unit with its spread in).**
Threshold
`k` = 3.97–4.01 at 110 Hz to 4 kHz across four rates; self-oscillation within 2 % of the cutoff;
self-oscillation peaks **0.87 / 0.93 / 1.02 / 1.15** at resonance 0.90 / 0.92 / 0.95 / 1.00 (`k`
4.5–5.0); the loop compresses **0.22 dB** at an input of 0.03 where `tanh` compressed 1.10 dB,
growing as the cube (1.19 dB at 0.06) where `tanh` grew as the square (2.72 dB) — on the matched
reference, and the sabotage was run in this crate too: `tanh` fails
`the_resonance_loop_is_clean_below_the_knee_and_limits_above_it`; DC gain tracks `1/(1+k)` to
0.01 dB; worst-case output 2.52 against a bound of 15.5. The spread against the matched reference
(the spike's section 9): the peak at cutoff **−0.12 / −0.20 / −0.62 / −2.86 dB** at resonance
0.30 / 0.50 / 0.70 / 0.78 — small far from the onset, and growing as `1/(4 − k)` does towards it —
and the onset within 0.0002 of the control at 40 Hz, 1 kHz and 19 kHz.

**One divergence from mono-01's harness, deliberate:** `measure_oscillation_threshold`'s sustain
floor is `1e-3`, not `1e-9`. With the excitation starting *below* the onset here, the old floor
read the excitation's −120 dB as oscillation at high cutoffs, where the impulse response decays
inside the settle window — section 7 of the spike read exactly 0.750, the excitation threshold,
at 10 and 19 kHz. The extraction plan should carry the fix back.

## The VCO is one waveform at a time, the sub is a divider on its reset, and three constants are chosen

`oscillator.rs` is the SH-2's sawtooth core with a four-position selector per VCO — VCO-1 sine,
saw, square, pulse; VCO-2 noise, saw, square, pulse — never a mix (`sh-2.md` §2, wart 1). PolyBLEP
saw and pulse are `mxm-mono-01-dsp`'s; the triangle from the phase is `mxm-mono-00-dsp`'s, copied
because that crate landed the same circuit first (plan §5.2); the sub's band-limited toggle is
`mxm-poly-06-dsp`'s divider. What is this machine's own, each measured:

- **The saw's corner is a discharge** (wart 18's waveform half). The reset is two half-steps
  `RESET_S` apart, and `RESET_S` is **chosen** — the schematic does not give it. Its consequence
  is a `cos(π f RESET_S)` roll-off on the harmonics, and
  `the_saws_corner_is_a_discharge_and_the_test_says_where_it_lands` reports where it starts:
  **the first harmonic more than 0.1 dB down is at 24.6 kHz** at the chosen 2 µs (derived 24 kHz),
  so nothing inside the audio band moves by more than 0.1 dB. Whether that is audible is not
  claimed either way. **Sabotaged**: with `RESET_S` at zero the test fails (2026-09-03).
- **The sine is a diode-rounded triangle** (wart 15): `SINE_KNEE` chosen, **3.6 % third
  harmonic, 1.0 % fifth, no even harmonics**, measured by
  `the_sine_is_a_rounded_triangle_with_a_few_percent_of_third_harmonic`, which also holds that the
  rounding reduces the bare triangle's third (11.1 %).
- **The sub is locked to VCO-1 and blind to the pulse width** (wart 6): its samples are
  bit-identical at any width, it toggles exactly at the reset and only there, and its toggle is
  band-limited (`the_sub_is_one_octave_below_locked_to_vco1_and_blind_to_the_pulse_width`,
  `the_subs_toggle_is_band_limited`).
- **One noise source**, white and seeded — the shaping network's effect is unmeasured and not
  invented — read by VCO-2's NOISE position, by the modulator's sample-and-hold, and by the Noise
  routing source, none of which draws a sample of its own.

**The waveforms' relative polarity and phase are inherited and chosen** (plan §5.2, §9): the ramp
rises, the pulse starts high, the sub's edge sits on the reset, the sine's seam where the reset
was. Recorded before presets and the golden score make them durable.

The one-sidedness of the width — narrowing from 50 % and never past it — is not the oscillator's
rule: it renders any width it is given, and the voice's clamp never gives it one above a half,
whatever routes reach it.

## One envelope, and the trigger modes are the voice's

`envelope.rs` is `mxm-poly-06-dsp`'s ADSR copied whole — mono-01's with the decay stall fixed, as
the plan's §7.2 asks. One object feeds the VCA and, through its routes, the VCF and the pulse width
(wart 19). **Which events call `trigger` is not its business**: GATE+TRIG, GATE and LFO are the
voice's three rules, fed by the keyboard block's trigger and the modulator's `square_rose`. Its
attack floor is the machine's 1 ms and nothing de-clicks below it — the manual documents the click
at Attack 0. The segment curves are unverified (`sh-2.md` §8, §11) and exponential is kept as the
honest guess.

## The modulator fades the sine and nothing else, and its random is the noise held

`lfo.rs` is the SH-2's one LFO (`sh-2.md` §6): a free-running triangle core, the panel's ∿ being
the triangle through **the same diode rounding as VCO-1's sine**, a square, and RANDOM as the
instrument's one noise source sampled on the square's rising edge — stepped, never slewed, at
the RATE slider's rate (wart 9's neighbours). The rules, each tested:

- **The delay is an RC fade on the sine path only.** The square and the random are at full level
  from the first sample after a trigger; the sine's gain follows `1 − e^(−t/τ)`; the **width tap —
  the LFO to width source — is the sine before the attenuator**, whatever the MODE switch says
  (`the_delay_fades_in_the_sine_only_on_an_rc_curve`). A zero delay is transparent.
- **A trigger dumps the capacitor and never resets the phase**
  (`a_zero_delay_is_transparent_and_a_trigger_does_not_reset_the_phase`).
- **Idle settles the fade and freezes the phase and the hold** (`Modulator::settle`, plan §5.6).
- **Chosen:** `DELAY_TAUS` — the control's seconds are the time at which the fade has all but
  arrived, so τ is a third of them. The specification gives the control's span, not what its
  number means.

## The voice wires the trigger to three consumers, defines idle, and lets a panic beat HOLD

`voice.rs` is `sh-2.md` §2's diagram: keyboard block → portamento → two VCOs and the sub → mixer
→ the four-OTA cascade → the VCA's input coupling → HOLD / ENV / GATE → volume, with every
modulation path a route (below). **The machine's external input is not built** (the owner,
2026-09-26): its jack into the mixer, the envelope follower it fed and the Follower source were
removed with the plugin's auxiliary input, so `Voice::process` takes the patch and the routes and
nothing from outside. The rules, each a test named in the module:

- **One trigger, three consumers.** The keyboard block's trigger restarts the envelope
  (GATE+TRIG) and the modulator's delay, and recharges the Auto bend source, so a higher key over a
  held lower one moves none of them and a lower key moves all three. GATE triggers on the gate's
  rising edge only; LFO on every rising edge of the modulator's square while the gate is high — the
  first note in LFO mode waits for the next edge, as the circuit's gated trigger does (**chosen**
  reading of `sh-2.md` §5.3). `ties_are_the_machines_legato_asymmetric_by_trigger_mode` is the plan's
  tie claim, all three directions in both keyed modes. **The auto bend's depth is its route's
  amount**, read continuously; before the routing conversion it was latched at the trigger.
- **The shared width is the slider plus its routes, clamped `0.05…0.5`** — one width both pulses
  read, never past square whatever sign reaches it (wart 4). The machine's two PWM positions are the
  (Pulse width ← LFO to width) and (Pulse width ← Envelope) routes, which narrow on a rising source
  (**chosen** polarity, `sh-2.md` §11).
- **Portamento never snaps.** The lag starts from the pre-note key at `reset`, glides from the
  previous note at every phrase start, and feeds the Key source. Its state is `f64`,
  which is load-bearing: in `f32` a glide towards a key near 72 **stalled 0.037 semitones
  short for ever**, the per-sample step falling under half an ulp of the value — the envelope's
  stall class again, found by `portamento_is_fixed_time_never_snaps_and_reaches_the_filter`.
- **The cutoff is a sum in octaves clamped at the machine's ceiling before the exponential**,
  and the envelope's route dominates it: at full depth it clips the sweep at the top before the
  attack ends (wart 12), and its negative half reaches `INVERTED_RATIO` of its positive (wart 13).
- **HOLD sounds with no gate; GATE is a step while the envelope keeps sweeping the filter.**
- **All Sound Off silences every mode and latches HOLD** until a note-on, `reset`, or HOLD left
  and re-entered; a mixer move does not resume it. A choke with nothing left cuts without a
  tail; All Notes Off releases and a HOLD drone plays on through it.
- **What a panic clears, and what it keeps.** It releases every press, silences the envelope,
  closes GATE's slope, zeroes the auto bend's dip, and resets the recursive state that carries
  what was heard: the ladder and the coupling stage. It keeps the owner — key, channel, velocity, expression — as any release
  does; the free-running state — both VCOs, the sub's divider, the modulator's phase, hold and
  fade, the noise — by the modulator contract above; the routing frame's late VCO sources, which
  are that free-running state a sample on; and, **undecided** (`plans/plan-sibling-audit.md`
  O4), the portamento lag where it stood and the activity settle, so the voice runs on through
  `POST_TAIL_S` before idling. The voice keeps processing through that settle, which is why the
  ladder and the coupling stage owe a reset: a note inside it would ring with what came before.
  `panic_clears_the_filter_and_the_coupling_but_retains_the_owner` panics two voices whose
  free-running state is identical — the same key for the same number of samples — and whose
  filters sat far apart, and requires the same note to render bit-identically; without either
  reset it differs.
- **Activity follows the output, with two exceptions.** The voice is active while its output has
  been above `SILENCE_FLOOR` within `POST_TAIL_S`, never because the switch says HOLD — so a silent
  or panic-latched HOLD is idle. **A held key holds idle off whatever the output**: idling silences
  the shared envelope, which a held GATE note with its levels down still needs on the filter and the
  pulse width the moment a level comes up, and the process-status contract promises the host
  `KeepAlive` for a key down (`a_held_gate_note_with_nothing_fed_keeps_its_envelope`; round 3 of the
  code review found idle destroying that envelope). **And a HOLD drone an Amplitude route is holding
  silent does not idle** while those routes can move it — idling there would reset the filter and
  freeze the modulator that is about to open it again — and `Routing::moves` asks for a depth, not
  a presence, so a route at zero holds nothing awake. **Idle is defined**: the portamento settles to
  the last key, the delay's fade to full, the auto bend to nothing; the phases freeze: an idle voice
  advances nothing and renders exact zeros until a wake, so a host that keeps calling changes
  nothing, and it publishes the cutoff the slider and the routes from still sources — Key,
  Velocity, Wheel, Pressure, Bend — put it at. It wakes when an open amplifier — HOLD unlatched,
  since a key down was never idle — has something to pass: a level up, or the filter singing past
  its excitation threshold. Idle is VCA-aware: the envelope holds it off only in
  ENV mode, where it is the gain; behind a closed GATE it runs on and is not the tail.
  `idle_freezes_free_running_state_and_settles_targeted_state_host_independently` renders two
  phrases, one with three seconds of calls through the silence, and requires them identical, and
  a glide cut short by idle to start the next phrase from the previous note.

**Every constant the research does not have is a `pub const` labelled chosen** — the envelope's
octaves, the inverted ratio, the LFO's reach on pitch and cutoff, the
auto bend's semitones and time constant (from the plan's C76 reading), the key tracking's
over-tracking top, the bender's reach on the cutoff, the gate's slope, the settle, the pre-note key.
The reaches are the routes' full scales now (`routing::FULL_SCALE`). Their values are in `voice.rs`
beside their reasons; the listening gate is where they are judged.

**The AC coupling is the VCA's input capacitor, at its corner and in its place.** `sh-2.md`
§7.4: C14 (0.1 µF) into R69 (270 kΩ), a first-order high-pass at **5.9 Hz, derived** from the
schematic (`VCA_COUPLING_HZ`); the output network's 10 µF (§7.6) is a decade lower, load-dependent
and not modelled. It sits **before** the amplifier as on the machine, so what the amplifier gates
carries no offset and a closed amplifier hides the stage's tail
(`the_coupling_sits_before_the_amplifier_so_a_closed_amplifier_leaves_no_tail`: a narrow pulse in
ENV mode is exact zero the moment the envelope ends). At the lowest F of the 32' range — 10.9 Hz,
if the keyboard's 8' bottom is F1, which `sh-2.md` does not record — it costs **1.11 dB**, the
machine's own thinning; mono-01's 15 Hz, which this crate carried until round 3 of the code review
found it, cost 4.61 dB there
(`the_coupling_is_the_vcas_and_costs_the_bottom_of_32_feet_a_decibel`, which also holds the
measured loss to the analytic first-order corner within 0.2 dB). **Both sabotaged 2026-09-03**:
15 Hz fails the corner test, and the stage moved back after the amplifier fails the position
test. The stage is reset on a panic and
at idle — a panic clears the recursive state (the list above), and idle's exact zero must not wait
on a 6 Hz tail under HOLD's open amplifier — and **no longer on a cut**: the cut closes the amplifier, which is enough,
and resetting a stage under HOLD's open amplifier put a step into the drone.

## Modulation is routing: thirteen sources, four targets, and the order they run in

`routing.rs` is this instrument's declaration on the shared
[`mxm-modulation`](https://github.com/mxm-audio/mxm-kit/blob/main/crates/mxm-modulation/AGENTS.md): its sources, its targets, each pair's full scale, the
one law that is this machine's rather than the collection's, and which routes the init patch holds
(`plans/plan-mxm-mono-02-modulation.md` §2 and §3). **Nothing in the voice asks whether a route is
the machine's own**; `MACHINE` is the whole of that, and `INIT_PRESENT` which of those a fresh
instance shows. The routing travels **beside** `Params`,
never inside it: `Params` is `Copy` and rebuilt every sample.

| # | Source | Value |
|---|---|---|
| 0 | LFO | The modulator as MODE selects it — the delayed sine, the square or the random — ±1 |
| 1 | LFO to width | The un-delayed sine as `(1 + sine) / 2`, 0…1: the PWM section's tap whatever MODE says (plan G2) |
| 2 | Envelope | 0…1 |
| 3 | Auto bend | The dip's shape: one at the keyboard's trigger, decaying over `AUTO_BEND_TAU_S` (plan G4) |
| 4 | Key | The glided key from middle C over `KEY_UNIT_SEMITONES`, 72: every MIDI key inside unit magnitude, chosen for that alone |
| 5–8 | Velocity, Wheel, Pressure, Bend | `v − 1` of the press that last triggered the envelope; the owner channel's wheel, pressure and lever position, the last ±1 — all through `mxm_modulation::standard`, each zero at rest |
| 9–11 | VCO-1, VCO-2, Sub | Audio, as each selector holds it |
| 12 | Noise | The voice's one noise sample: publishing it draws nothing more |

The Follower was source 3 until the external input it followed was removed (2026-09-26); the sources
after it moved down one and kept their order, so no route sum's order changed.

| Target | Law | Full scale at amount one — the machine's paths, then every added one |
|---|---|---|
| Pitch | Sum in semitones, onto both VCOs and the sub | LFO 7 (`VCO_LFO_SEMITONES`), Auto bend −12 (`AUTO_BEND_SEMITONES`); added 12, Key 12 per octave |
| Pulse width | Sum onto the slider, clamped `0.05…0.5` | −0.45 (`PWM_SWING`), narrowing from square, which the standard also takes; Key 9 % per octave |
| Cutoff | Sum in octaves, clamped at the ceiling before the exponential | Envelope 10 (`FILTER_ENV_OCTAVES`), LFO 4, Key 7.2 over the key's unit — 1.2 octaves per octave of key — and Bend 2 (`BEND_FILTER_OCTAVES`); added 4 |
| Amplitude | `gain × standard::amplitude_factor(Σ)` on whatever the VCA switch chose | 1, Key 20 % per octave: a route can close the amplifier or double it, and never opens a closed one |

**Every path the SH-2 did not have takes the collection's standard reach**
(`plans/plan-modulation-standard.md`, 2026-09-26). An added pitch route used to take the LFO's seven
semitones — Pitch ← Key at full was 1.17 semitones per octave — and an added cutoff route the
envelope's ten octaves. No factory design used an added route, and the bank capture is unchanged.
`conformance.rs`'s `Declared` runs the standard's checks over the real tables and sum, each falsified
once, and the plugin's tests reuse it through the `conformance` feature.

**Wart 13 is a law, not a switch.** The (Cutoff ← Envelope) route's negative half scales by
`INVERTED_ENVELOPE_OCTAVES`, `INVERTED_RATIO` of the positive, and the law is continuous through zero
because both halves are zero there (plan G3). Each route is `(amount × source) × scale`, the shared
crate's order, and a route at zero depth is skipped in the sum, which changes no bit. The sums are
bounded by `BOUND` — generous on the three targets that clamp their own result, and **one on
Amplitude**, which is what keeps the output inside `OUTPUT_BOUND` whatever a player routes there.

**The evaluation order is a contract**, and `a_route_from_vco1_is_a_sample_late_into_pitch_and_on_time_into_cutoff`
reads it off real samples:

| Published | Read by Pitch and Pulse width | Read by Cutoff and Amplitude |
|---|---|---|
| LFO, LFO to width, Envelope, Auto bend, Key, Velocity, Wheel, Pressure, Bend, Noise — before the pitch sum | this sample | this sample |
| VCO-1, VCO-2, Sub — after the oscillators, before the cutoff sum | last sample | this sample |

The unit delay is what makes a player-made cycle well defined, and the frame's bound at publication
is what keeps it bounded.

**The init patch is the machine.** The eight routes the SH-2 hard-wired that are still here —
(Pitch ← LFO), (Pitch ← Auto bend), (Pulse width ← LFO to width), (Pulse width ← Envelope),
(Cutoff ← Envelope), (Cutoff ← LFO), (Cutoff ← Key), (Cutoff ← Bend) — are `MACHINE`, each at its
control's reach (`machine`). Init shows six of them at zero depth, `INIT_PRESENT`: **the pulse
width's two are offered, not present** (the owner, 2026-09-27), and `Routing::machine` is all eight,
which `tests/legacy_reachability.rs` translates the old panel into.
`the_machines_own_routes_at_zero_render_bit_identically_to_nothing_routed` holds that Init's six and
all eight render exactly what nothing routed renders while every source they read moves.

**What the routing owes, each a test in `tests/routing.rs` and each run against the defect it names**
(the plan's §13 records every mutation):

- **A source that becomes read starts from silence.** `Graph::set_topology` clears it, because an
  unpublished slot keeps an old phrase's value —
  `a_source_that_becomes_needed_starts_from_silence_not_from_an_old_phrase`.
- **The auto bend runs whether routed or not**, so it owes no reset when a route arrives —
  `the_auto_bend_runs_whether_routed_or_not`.
- **A route at zero depth counts for nothing in any predicate**: the resting cutoff an idle voice
  publishes, and `Routing::moves`, which the idle predicate reads —
  `a_zero_depth_route_neither_wakes_an_idle_voice_nor_moves_its_resting_cutoff`.
- **HOLD under a deep tremolo does not idle** — `hold_under_a_deep_tremolo_does_not_idle`.
- **The signed laws are continuous and keep the machine's limits**: wart 13 through zero, wart 4 from
  every sign, and an amplifier that never inverts nor more than doubles —
  `the_signed_laws_are_continuous_and_keep_the_machines_limits`.
- **A route at full reads the machine's own number** — `a_route_at_full_amount_reads_the_machines_own_number`.
- **Every pair at extreme amounts is finite and within `OUTPUT_BOUND`** from 1 kHz to 768 kHz; **a
  player-made cycle stays finite**; **audio-rate routes into the cutoff at full resonance stay bounded
  from silence** and do not grow — `every_pair_at_extreme_amounts_stays_finite_and_bounded`,
  `a_player_made_cycle_stays_finite`,
  `audio_rate_routes_into_the_cutoff_at_full_resonance_stay_bounded_from_silence`. The first holds
  finiteness and the stated bound, which is loose: the amplitude sum's bound of one is held by the
  signed-laws test, not by it.
- **`reset` clears both halves of the frame** — `reset_leaves_no_routed_tail`.

**Every state the nine retired controls could reach is reachable by routes but one.**
`tests/legacy_reachability.rs` carries the voice's own arithmetic from before the conversion, at
`ba3bd30`, and compares it with the routes plan §4 translates each patch to, over 20 000 randomised
patches, at the values the targets receive: worst **1.9e-6 semitones, 6.0e-8 of width, 1.9e-6
octaves**. It is what moved `BEND_FILTER_OCTAVES` from one octave to two: at one, the retired bender
sensitivity at a 24-semitone bend range was out of a route's reach. **The one that is not**: the
envelope switch's ENV FOL'R, which put the follower on the cutoff — removed with the external input
(the owner, 2026-09-26), and excluded from the test deliberately. That sensitivity's dependence on
the bend range was this implementation's and not the machine's, whose two sensitivities are
independent (`sh-2.md` §4.9, plan N1); the Bend source is the lever's position.

## Dependencies: one at runtime

**One runtime dependency, [`mxm-modulation`](https://github.com/mxm-audio/mxm-kit/blob/main/crates/mxm-modulation/AGENTS.md)**, which has none of its own
and holds this same 1.87 floor — the routing conversion added it, and the MSRV override still rests on
the shipped graph staying that small. `cargo tree -p mxm-mono-02-dsp -e normal` shows that crate and
nothing else.

`[dev-dependencies]` holds **`mxm-measure`**, the collection's measurement rulers — zero dependencies
at this same floor, reaching only tests and `examples/`, never a shipped `.clap`.
[`../mxm-measure/AGENTS.md`](https://github.com/mxm-audio/mxm-kit/blob/main/crates/mxm-measure/AGENTS.md)'s verification section checks that rather than
asserting it.

It also holds **`mxm-audio-file`**, which writes the listening demo, and **`mxm-audio-file-decode`**,
which its test reads the file back through — test-only edges on the same terms. The decoder's
MPL-2.0 symphonia therefore reaches this crate's tests and never its shipped graph.

## Everything else the collection's DSP already requires

No framework types; realtime rules on every per-sample path — the block is fixed arrays and
never allocates; denormals flushed in the DSP itself; `f32` in the audio path and `f64` for
prewarping; every saturator bounded exactly and monotonic; a stated `pub const` output bound;
deterministic seeded randomness. These are the parent's and
[`crates/mxm-mono-01-dsp/AGENTS.md`](https://github.com/mxm-audio/mxm-mono-01/blob/main/crates/mxm-mono-01-dsp/AGENTS.md)'s, not restated here.

**`MIN_SAMPLE_RATE` (1 kHz) is the lowest rate the plugin activates at.** `f32::clamp` panics on a
NaN or crossed bound, and the ladder's `10 Hz ..= 0.45 × rate` crosses below 22.2 Hz.

## Cross-crate comparison evidence

Keep this comparison until a shared-DSP proposal either uses or rejects it; resemblance alone does
not authorize extraction. `flush` and `Rng` match `mxm-mono-01-dsp` byte for byte:

| Candidate | Standing against `mxm-mono-01-dsp` |
|---|---|
| `flush`, `Rng` | **Identical**, byte for byte |
| The ladder: TPT core, Newton solve, `diode_clamp`, `tan_approx`, `tanh_approx` | **Same core, same clamp; four constants differ** (`K_MAX`, `CUTOFF_MIN_HZ`, `EXCITATION_THRESHOLD`, and the input stage's form — `INPUT_KNEE * tanh(x / INPUT_KNEE)` against `tanh(DRIVE * x)`, which is the same expression with `DRIVE` = 1 at one knee), **and the cascade's arithmetic is `mxm-poly-06-dsp`'s per-stage form** (four `G`s from four trims; mono-01's single `G` is that form with every trim at 1). So the three ladders are one module with a trim array and a saturator as its two differences — the shape an extractable module has |
| `measure_oscillation_threshold` | **Diverged**: the sustain floor, see above; mono-01 should take it. `measure_oscillation_threshold_of` takes the constructor, so the matched reference can be measured with the same harness |
| `poly_blep`, `Phasor`, `pulse`, `clamp_pulse_width` | **Identical** to mono-01's, except `Phasor::advance` reports the wrap (mono-00's form, which the sub needs) |
| `DcBlocker` | **Diverged**: the corner is the caller's (`set_corner`), not the type's. mono-01's 15 Hz is that machine's chosen number and this machine's is a capacitor on its schematic; the extraction takes this form and each voice supplies its corner |
| `saw` | **Diverged by design**: the discharge corner, two half residuals instead of one |
| `triangle` | **Identical** to mono-00's |
| The sub | `mxm-poly-06-dsp`'s divider, as a separate `Sub` clocked from `Vco::just_reset` rather than a field of the oscillator |

| `Adsr` | **Identical** to `mxm-poly-06-dsp`'s (the stall-fixed form); mono-01's still carries the stall |
| The modulator | New here: mono-01's LFO has five shapes and no delay, poly-06's a triangle with a wait-then-fade delay; this one has three shapes, a diode-rounded sine, an RC fade on one path and an un-delayed tap. Nothing to extract yet |

# Work Guidance

- Update this contract when module behavior changes; do not describe unimplemented behavior as
  current.
- Filter theory is [`docs/filters/`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/filters/README.md); oscillators
  [`docs/oscillators/`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/oscillators/README.md); envelopes, the LFO and glide
  [`docs/modulation/`](https://github.com/mxm-audio/mxm-kit/blob/main/docs/modulation/README.md).
- Prefer a clear implementation to a clever one. This is reference-quality open source.

# Verification

**The rulers are shared, the thresholds are not.** `mxm-measure` is a `[dev-dependencies]` entry —
zero dependencies at this same 1.87 floor, and **not in the shipped graph**. Measurements come from
there; every bound and its headroom stays in the test that argues for it.

```bash
cargo test -p mxm-mono-02-dsp
cargo clippy -p mxm-mono-02-dsp --all-targets
cargo tree -p mxm-mono-02-dsp -e normal                                  # mxm-modulation and nothing else
cargo +1.87.0 build -p mxm-mono-02-dsp                                   # and `test`, the harder floor
cargo run -p mxm-mono-02-dsp --release --example mono_02_filter_spike   # the filter's real numbers, calibration included
cargo run -p mxm-mono-02-dsp --release --example sh2_demo       # the mechanisms, played — writes mxm-mono-02-demo.wav
```

The demo renders 21 seconds. Rendering does not establish listening fidelity; that gate remains
manual.

Properties the tests must keep asserting, because each regresses silently:

- the keyboard block's rules, one test per rule (above)
- silence in gives **exactly** zero out below the excitation threshold — the denormal-flush test too
- no NaN or inf across a sample-rate × cutoff × resonance sweep
- output within the stated bound under overdrive past the oscillation threshold
- the filter's threshold at four rates, **and its onset on the control inside the service notes'
  7–9 band** at 40 Hz, 1 kHz and the top of the range
- the resonance loop clean below the knee and limiting above it, with the cube-law growth that
  separates the clamp from `tanh` — the test `tanh` fails; measured on the matched reference
- this unit's stage spread inside its stated band, moving the resonant peak measurably and by
  under 3 dB, and the onset not at all; `reset` leaving the unit's trims alone
- the input stage linear at full scale and bounded far past it
- the coupling: the schematic's first-order corner within 0.2 dB at the bottom of the 32' range,
  under 1.5 dB of loss there (the test 15 Hz fails), and before the amplifier — exact zero the
  moment the envelope closes on a narrow pulse (the test the stage after the amplifier fails)
- every waveform in tune to a cent at four rates; saw, pulse and sub aliasing against the trivial
  waveform; the discharge corner's roll-off located above the audio band, and present at the top
  (the test `RESET_S` = 0 fails); the sine's harmonics; the sub locked and blind to the width
- the voice: the tie asymmetry in both keyed modes; LFO mode's attack count; the width never
  past square whatever routes reach it; the delay and the auto bend restarting from below only; the
  glide fixed-time, never snapping, reaching the filter; the envelope's route clipping the cutoff and
  the inverted ratio; HOLD, GATE, the panic latch and its three ways out; a panic clearing the
  ladder and the coupling stage across two histories; activity from the
  output, a held key never idle, and a HOLD drone a route holds silent never idle; the three
  terminations; idle host-independent;
  exact zero after release and after `reset`; no NaN and bounded across a sweep at four rates;
  two instances bit-identical
- the routing: every obligation in `tests/routing.rs` above, the init routes at zero bit-identical
  to nothing routed, and every retired control's reach in `tests/legacy_reachability.rs`

**No hardware was measured**, here or in any source this instrument rests on. Fidelity is
UNVERIFIED until the plan's listening gate is run. Linux and macOS are unverified — there is no CI
(root *Windows, Linux and macOS*) — and the development machine is Windows.

# Child DOX Index

No child AGENTS.md files. `src/` and `tests/` are covered by this doc.
