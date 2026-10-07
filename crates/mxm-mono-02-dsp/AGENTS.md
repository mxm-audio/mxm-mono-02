# AGENTS.md — crates/mxm-mono-02-dsp

Parent: [`../../AGENTS.md`](../../AGENTS.md)

# Purpose

The mxm-mono-02 voice as plain Rust: keyboard block, calibrated filter, oscillators, envelope,
modulator, auto bend, portamento, and the modulation routing that wires them. Framework-free, so the
whole signal path is testable with `cargo test` and no host involved.

**A monophonic two-oscillator synthesizer inspired by the Roland SH-2**
(`research:instruments/sh-2.md`; filter `research:filters/machines/ba662-sh-2.md`). This document
owns the current DSP contract; the design record is in the private archive
([NOTES.md § The design record](NOTES.md#the-design-record)), and measurements, sabotage runs,
history and the cross-crate evidence are in [NOTES.md](NOTES.md). The hosting plugin is
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

## Keyboard block (`keyboard.rs`) — presses, not pitches
[NOTES.md § The keyboard block](NOTES.md#the-keyboard-block-is-the-instruments-feel-and-it-keeps-presses-not-pitches)

- **The lowest held key sounds; the trigger fires on the gate's rising edge and on every new lower
  key, and on nothing else.** A higher key over a held lower one produces no event at all. The one
  trigger reaches all three consumers: the envelope (GATE+TRIG), the LFO delay, the auto bend.
- Among equal keys the older press sounds and hands over with no trigger
  (`a_same_pitch_tie_hands_off_between_two_presses_without_a_gap`,
  `a_stack_keyed_by_pitch_would_fail_the_tie`). An id is authoritative when both sides have one;
  otherwise the key decides, and an id-less release retires the *oldest* press of that key.
- A non-finite expression is refused (`a_non_finite_expression_is_refused_and_the_pitch_stays_finite`).
- Bounded at `CAPACITY`: a full block evicts the oldest non-sounding press, never the sounding one;
  a release for an evicted press is dropped (`the_block_survives_exhaustion_without_a_stuck_note`,
  `exhaustion_never_evicts_the_sounding_press_even_when_it_is_the_newest`).
- The `Owner` (key, channel, velocity, expression) outlives its press until a new press sounds, and
  is a voice default before the first press since `reset`
  (`the_owner_outlives_its_press_and_starts_as_the_default`). The **Velocity source** is the press
  that last triggered the envelope, held by the voice, at full before any press
  (`velocity_is_the_press_that_last_triggered_the_envelope`).
- Note-off falls back without a trigger; All Notes Off releases everything and is never a panic; a
  choke carries `cut` when nothing remains (`choke_is_a_release_with_others_held_and_a_cut_alone`).

## Filter (`filter.rs`) — mono-01's ladder, this machine's calibration
[NOTES.md § The filter](NOTES.md#the-filter-is-mono-01s-diode-clamped-ladder-with-this-machines-constants-and-its-calibration-is-the-machines) (the constants table, each reason, the measurements)

- A verbatim copy of mxm-mono-01's `crates/mxm-mono-01-dsp/src/filter.rs`, kept diffable for the extraction plan.
  Only `K_MAX`, `CUTOFF_MIN_HZ`, `EXCITATION_THRESHOLD` and the input stage differ; `INPUT_KNEE` and
  `STAGE_SPREAD` (wart 16) are added; `CLAMP_KNEE` stays mono-01's.
- `K_MAX` puts self-oscillation's onset in the service notes' 7–9 band
  (`calibration_puts_self_oscillation_between_7_and_9`); `EXCITATION_THRESHOLD` sits below it.
- `Ladder::new()` is this unit with its spread, `Ladder::matched()` the reference; the clamp's shape
  test runs on `matched()` deliberately (`this_units_stage_spread_moves_the_peak_and_not_the_onset`).
- `NYQUIST_FRACTION` is a labelled deviation at 44.1 kHz **and** what holds the coefficients under
  audio-rate cutoff routes — without it and the voice's octave clamp the loop runs away.
- `measure_oscillation_threshold`'s sustain floor is `1e-3`, not mono-01's `1e-9`, deliberately.

## Oscillators (`oscillator.rs`)
[NOTES.md § The VCO](NOTES.md#the-vco-is-one-waveform-at-a-time-the-sub-is-a-divider-on-its-reset-and-three-constants-are-chosen)

- One waveform per VCO at a time, never a mix (wart 1). `RESET_S` (the saw's discharge corner) and
  `SINE_KNEE` are **chosen**; the sub is locked to VCO-1, blind to the pulse width and band-limited.
- **One noise source**, white and seeded, read by VCO-2's NOISE, the modulator's S&H and the Noise
  source; none draws a sample of its own.
- The waveforms' relative polarity and phase are inherited and chosen; presets and the golden score
  depend on them. The oscillator renders any width; never-past-square is the voice's clamp.

## Envelope and modulator (`envelope.rs`, `lfo.rs`)
[NOTES.md § One envelope](NOTES.md#one-envelope-and-the-trigger-modes-are-the-voices) ·
[NOTES.md § The modulator](NOTES.md#the-modulator-fades-the-sine-and-nothing-else-and-its-random-is-the-noise-held)

- `envelope.rs` is `mxm-poly-06-dsp`'s stall-fixed ADSR copied whole. Which events call `trigger` is
  the voice's business. Attack floor 1 ms, nothing de-clicks below it; exponential segments are a guess.
- The LFO delay is an RC fade on the **sine path only**; the width tap (LFO to width) is the sine
  before the attenuator, whatever MODE says. A trigger dumps the capacitor and never resets the
  phase. Idle settles the fade and freezes the phase and the hold. `DELAY_TAUS` is chosen.

## Voice (`voice.rs`)
[NOTES.md § The voice](NOTES.md#the-voice-wires-the-trigger-to-three-consumers-defines-idle-and-lets-a-panic-beat-hold)

- **The machine's external input is not built** (the owner, 2026-09-26): `Voice::process` takes the
  patch and the routes and nothing from outside.
- One trigger, three consumers (`ties_are_the_machines_legato_asymmetric_by_trigger_mode`); LFO mode
  waits for the square's next rising edge; the auto bend's depth is its route's amount, read continuously.
- The shared width is the slider plus its routes, clamped `0.05…0.5`, never past square (wart 4).
  The cutoff is a sum in octaves clamped at the ceiling before the exponential (warts 12, 13).
- **Portamento state is `f64`, load-bearing**: in `f32` the glide stalls short of the key.
- All Sound Off silences every mode and latches HOLD until a note-on, `reset`, or HOLD re-entered;
  a mixer move does not resume it. A choke alone cuts without a tail; All Notes Off leaves a HOLD drone.
- A panic resets the ladder and the coupling stage and keeps the owner and the free-running state;
  the portamento lag and settle are **undecided** (`plans/plan-sibling-audit.md` O4, private archive)
  (`panic_clears_the_filter_and_the_coupling_but_retains_the_owner`).
- **Activity follows the output** (`SILENCE_FLOOR` within `POST_TAIL_S`), except that a held key, and
  a HOLD drone an Amplitude route holds silent, never idle. `Routing::moves` asks for a depth, not a
  presence. Idle is host-independent
  (`idle_freezes_free_running_state_and_settles_targeted_state_host_independently`).
- Every constant the research does not have is a `pub const` labelled chosen, its reason in `voice.rs`.
- The AC coupling (`VCA_COUPLING_HZ`, derived) sits **before** the amplifier; it resets on a panic
  and at idle, **not** on a cut.

## Routing (`routing.rs`) — thirteen sources, four targets
[NOTES.md § Modulation is routing](NOTES.md#modulation-is-routing-thirteen-sources-four-targets-and-the-order-they-run-in) (the tables and the reachability measurements)

- Sources, in this order: LFO, LFO to width, Envelope, Auto bend, Key, Velocity, Wheel, Pressure,
  Bend, VCO-1, VCO-2, Sub, Noise. Targets: Pitch, Pulse width, Cutoff, Amplitude.
- **Nothing in the voice asks whether a route is the machine's own**: `MACHINE` is all eight
  hard-wired routes, `INIT_PRESENT` the six Init shows at zero depth (the pulse width's two are
  offered, not present). The routing travels **beside** `Params` (`Copy`, rebuilt every sample).
- Every path the SH-2 did not have takes the collection's standard reach; `conformance.rs`'s
  `Declared` runs the standard's checks, reused by the plugin through the `conformance` feature.
- Wart 13 is a law (`INVERTED_ENVELOPE_OCTAVES`), continuous through zero. Each route is
  `(amount × source) × scale`; a zero-depth route is skipped; `BOUND` is one on Amplitude.
- **The evaluation order is a contract**: VCO-1, VCO-2 and Sub reach Pitch and Pulse width a sample
  late, everything else is on time (`a_route_from_vco1_is_a_sample_late_into_pitch_and_on_time_into_cutoff`).
- The machine's routes at zero render bit-identically to nothing routed
  (`the_machines_own_routes_at_zero_render_bit_identically_to_nothing_routed`).
- `tests/routing.rs` owes: a newly read source starts from silence; the auto bend runs unrouted; a
  zero-depth route counts in no predicate; HOLD under a deep tremolo does not idle; the signed laws
  keep the machine's limits; a full route reads the machine's number; extreme amounts, player cycles
  and audio-rate cutoff routes stay finite and bounded 1 kHz–768 kHz; `reset` clears the frame.
- `tests/legacy_reachability.rs` holds every retired control's reach, but ENV FOL'R (removed).

## Dependencies and the collection's DSP rules
[NOTES.md § Dependencies](NOTES.md#dependencies-one-at-runtime) ·
[NOTES.md § Everything else](NOTES.md#everything-else-the-collections-dsp-already-requires)

- **One runtime dependency, `mxm-modulation`**, which has none and holds 1.87; the MSRV rests on
  that. `mxm-measure`, `mxm-audio-file` and `mxm-audio-file-decode` are dev-dependencies only, so
  MPL-2.0 symphonia never reaches the shipped graph.
- No framework types; realtime rules on every per-sample path (the block is fixed arrays);
  denormals flushed in the DSP; `f32` audio, `f64` prewarping; bounded monotonic saturators; a
  `pub const` output bound; seeded randomness.
- **`MIN_SAMPLE_RATE` (1 kHz) is the lowest rate the plugin activates at**: `f32::clamp` panics on
  a crossed bound, and the ladder's range crosses below 22.2 Hz.
- Keep [NOTES.md § Cross-crate comparison evidence](NOTES.md#cross-crate-comparison-evidence) until
  a shared-DSP proposal uses or rejects it; resemblance alone does not authorize extraction.

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

Properties the tests must keep asserting, because each regresses silently (the full list:
[NOTES.md § Properties the tests must keep asserting](NOTES.md#properties-the-tests-must-keep-asserting)):

- every keyboard-block, voice and routing rule above, one test per rule
- **exact** zero: from silence below the excitation threshold (denormal flush included), after
  release, after `reset`, and the moment the envelope closes on a narrow pulse
- no NaN or inf and output within the stated bound across sample-rate × cutoff × resonance sweeps
  at four rates and under overdrive past the oscillation threshold
- the filter's threshold at four rates and its onset inside the 7–9 band; the clamp's cube-law
  growth (the test `tanh` fails, on the matched reference); the stage spread moving the peak under
  3 dB and the onset not at all, `reset` leaving the trims alone; the input stage linear at full scale
- the coupling's corner within 0.2 dB and under 1.5 dB of loss at the bottom of 32' (the test 15 Hz
  fails), placed before the amplifier (the test the stage after the amplifier fails)
- every waveform in tune to a cent at four rates; aliasing against the trivial waveform; the
  discharge corner (the test `RESET_S` = 0 fails); the sine's harmonics; the sub locked and blind
- two instances bit-identical

**No hardware was measured**, here or in any source this instrument rests on. Fidelity is
UNVERIFIED until the plan's listening gate is run. The development machine is Windows; Linux and
macOS are checked later, together, and by CI when started by hand (root *Windows, Linux and
macOS*).

# Child DOX Index

No child AGENTS.md files. `src/` and `tests/` are covered by this doc.
