# mxm-mono-02

A monophonic synthesizer. Architecture inspired by the Roland SH-2 — two free-running oscillators,
a sub-oscillator, one envelope and one modulator — but the interface is not,
and the name is not. Not affiliated with or endorsed by Roland.

**The synth only.** The hardware's keyboard and bender lever are [MXM Player](https://github.com/mxm-audio/mxm-player/blob/main/apps/mxm-player)'s
or your DAW's, by the collection's rule that an instrument does not carry what the host already
does. No effects, because the machine has none.

## What it is

| | |
|---|---|
| Oscillators | Two, each with a five-position octave range and **one waveform at a time**: VCO-1 sine, sawtooth, square or pulse; VCO-2 noise, sawtooth, square or pulse. The sawtooth has the discharge corner of a real reset, the sine is a triangle rounded through a diode. VCO-2 has its own tune with a narrow or a wide range; the bender reaches VCO-1 by a switch |
| Sub-oscillator | One octave below VCO-1, locked to its reset |
| Pulse width | One width for both oscillators, set by hand, never wider than square — and moved by its routes: the envelope and the modulator's un-delayed sine narrow it as the machine's PWM section did |
| Mixer | Three levels — the sub, VCO-1, VCO-2 — into the filter |
| Filter | The four-OTA cascade with the diode clamp its own schematic shows: self-oscillation begins at four-fifths of the resonance control, and the bass does not come back as resonance rises. Cutoff and resonance, with the envelope, the modulator, keyboard tracking and the bender as routes on the cutoff — the envelope's amount signed, its downward half reaching less far than its upward, as the machine's inverter did |
| Envelope | **One ADSR, shared three ways** — the filter, the amplifier, and the pulse width — with three trigger modes: gate plus trigger, gate only, or refiring on the modulator while a key is held |
| Amplifier | A three-way switch: HOLD leaves it open with no key down, ENV follows the envelope, GATE follows the key |
| LFO | Sine, square or a random output, with a delay that **fades the sine only**; the square and the random arrive at once |
| Auto bend | A pitch dip on every trigger, recovering in about seventy milliseconds; how deep is its route's amount |
| Keyboard | **Low-note priority, and the trigger fires only from below**: a higher key pressed over a held one is ignored until the lower is released. Portamento from the held key's pitch |

## The controls

**Twenty-six controls from the machine's panel**, twenty-five in seven cards in the panel's own
order — LFO, Oscillator, Mixer, Filter, Envelope, Amplifier, Voice — and Volume in the app bar
beside the output meter, **and the machine's modulation as routing**. Four destinations — pitch, pulse width, cutoff and amplitude — can each take any of
thirteen sources at a signed amount: the modulator, its pulse-width tap, the envelope, the auto
bend, the key, velocity, the mod wheel, channel pressure, the bender, both oscillators, the sub and
the noise. That is 131 parameters.

**The routes the SH-2 wired itself are present in a fresh instance, at zero**, so Init is the machine
and every route it never had is one added beneath the control it moves. Each route's amount reads
what it does in the destination's own unit — semitones, octaves, a percentage of width — so a route
at full reads the number the machine's own control reached. The `Synth` view draws the controls as
knobs and switches, the routes as a stack under what they move, and the filter's response in the
Filter card; the `Parameters` view lists every parameter as a slider with direct text entry.

**Every amount starts at zero.** The init patch is VCO-1's sawtooth into an open filter, the
envelope on the amplifier, and VCO-2 three and a half cents sharp so the moment its level rises the
two beat.

The **mod wheel** pushes the modulator's pitch route at play time and writes no parameter, as it did
before there was routing; the machine had no wheel. Velocity, the wheel and channel pressure are
sources as well, taken from the note that is sounding.

**Projects saved before routing** keep every control that survived and lose what the nine retired
ones held — the modulator's two depths, the auto bend, the pulse-width depth and its source switch,
the envelope's depth and its polarity switch, keyboard tracking and the bender's cutoff sensitivity.
Every sound they made can be made again with routes.

## Presets

Fifty factory sounds, compiled into the plugin, the first twenty one per mechanism: beating saws, the sub bass, the
square lead, a pulse-width sweep, the sine flute, a noise wash, delayed vibrato, a random filter, a
gate stab, the auto-bend dip, hollow fifths, an inverted pluck, a slow glide, an LFO trill, and the
rest. **Init is not a file**: it is generated from the parameter defaults, so it cannot be deleted
and cannot drift from them. Your own presets are saved as readable JSON under the platform config
directory.

## Status

**Parameters, MIDI, presets, telemetry, the editor and modulation routing shipped**, and the plugin
plays through MXM Player's own hosting path. The design brief's recognisability trial and the design
system's §15 gate have not been run, and nobody has listened to the routing conversion yet; see
[`docs/briefs/mxm-mono-02.md`](../../docs/briefs/mxm-mono-02.md).

**Fidelity is UNVERIFIED.** No unit was measured. The constants are read off the service notes and
pinned by the DSP crate's own harness — see
[`crates/mxm-mono-02-dsp`](../../crates/mxm-mono-02-dsp/NOTES.md) for every one and its reason.

## Building

```bash
cargo xtask bundle mxm-mono-02 --release
clap-validator validate "target/bundled/mxm-mono-02.clap"
```

GPL-3.0-or-later — see the repository's [`LICENSE`](../../LICENSE). All code is original.
