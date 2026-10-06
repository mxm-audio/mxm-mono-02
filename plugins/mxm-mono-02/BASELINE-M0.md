# mxm-mono-02 — pre-conversion reference, captured at M0

`plans/plan-mxm-mono-02-modulation.md` M0. **These figures stop existing once the routing conversion
starts**, which is why they are captured first and recorded rather than re-derived later.

Produced by `plugins/mxm-mono-02/src/lib.rs`'s `baseline` module, on the tree at `ba3bd30` with only
the measurement seam and that module added:

```bash
cargo test -p mxm-mono-02 --release --lib baseline -- --ignored --nocapture --test-threads=1
```

Release only.

## Throughput — the cost gate, not yet measured

**No figure is recorded here, deliberately.** Every timing taken during this conversion was taken
while another conversion was building on the same machine, and `docs/code-review-notes.md` §3 is
plain that a timing taken during a build is not a measurement. The figures this document first
carried were taken the same way and are withdrawn. The digests below are deterministic and are
unaffected.

**What the gate needs:** the pre-conversion figure taken from **the M0 revision** — the tree at
`ba3bd30` with only the measurement seam and this module added — and the converted tree's,
**measured together, alternately and under the same conditions, on a quiet machine**, best of several
runs each. The `#[ignore]`d test is `throughput_of_init_and_a_routed_patch`:

```bash
cargo test -p mxm-mono-02 --release --lib baseline::throughput -- --ignored --nocapture
```

| Case | Why it is measured |
|---|---|
| Init, held note 48 | The init patch: inline multiply-adds before the conversion, nine live routes at zero depth after |
| Init with nothing routed — converted tree only | Even the machine's own routes absent, so no frame opens and no sum is taken: its distance from Init is what those nine zero-depth routes cost |
| `Random arp`, held note 48 | **The routed patch.** Every factory sound overrides at most two of the controls that retire; of those it reaches two targets — the LFO on pitch and the envelope on the cutoff — with the LFO retriggering the envelope |

All at 48 000 Hz in 64-sample blocks, through the plugin's own per-sample path — `next_patch()` per
sample, which advances every smoother and rebuilds the patch, then `Voice::process` with the external
input silent — omitting the wrapper's per-block event handling and buffer plumbing, which is not
where the routing work lands. At M0 the seam was the loop itself:

```rust
pub fn render_block_for_test(&mut self, out: &mut [f32]) {
    for sample in out.iter_mut() {
        let patch = self.next_patch();
        *sample = self.voice.process(&patch, 0.0);
    }
}
```

On the converted tree it calls `begin_interval` once and `render_sample` per sample, which are the two
functions `process()` calls. **Not portable across machines**: what a figure is good for is the
before/after comparison on the machine that took both.

## Factory bank — fifty-one reference digests

FNV-1a over the raw sample bits, the digest the player's golden tests use. One note (48) held 1.5 s
then released with 2.5 s of tail, identical for every sound. The last column is how many parameters
the file applied: all thirty-five, every one.

**Verified reproducible**: `the_bank_against_the_m0_dump`, run against the renders the first run
dumped, reported *moved 0 of 51*.

| Sound | Digest | Peak | Applied |
|---|---|---|---|
| `init` | `dd50dbf4dc1d9353` | 0.2533 | 0 |
| `init-saw` | `ca786fd247e8fbdd` | 0.2481 | 35 |
| `beating-saws` | `d337e73681b1d7e0` | 0.4582 | 35 |
| `sub-bass` | `2225b4a050298e27` | 0.4143 | 35 |
| `square-lead` | `3e2c00a22ada9034` | 0.3331 | 35 |
| `pulse-sweep` | `4dcb728202460129` | 0.7418 | 35 |
| `sine-flute` | `fea4ad91fc4f2944` | 0.2430 | 35 |
| `noise-wash` | `2ec50cb4f2869679` | 0.1147 | 35 |
| `bent-brass` | `4eac04f2771623d3` | 0.4420 | 35 |
| `delayed-vibrato` | `e2287b896b195ab1` | 0.3367 | 35 |
| `shuffle-bass` | `46f2620ae22e70b1` | 0.0993 | 35 |
| `organ-hold` | `e8afeae9363307c5` | 0.5025 | 35 |
| `random-filter` | `bd0d1fc876856b7b` | 0.1962 | 35 |
| `gate-stab` | `faf12e4e3dd280f1` | 0.2919 | 35 |
| `auto-dip` | `83cc49296d9af864` | 0.1333 | 35 |
| `hollow-fifths` | `e676635fd1a0eb98` | 0.5009 | 35 |
| `wide-detune` | `ae3a40fc777019e1` | 0.3183 | 35 |
| `inverted-pluck` | `3dd75ccbf760bed7` | 0.1149 | 35 |
| `tracking-sweep` | `307bf4f43affc817` | 0.1531 | 35 |
| `slow-glide` | `fe9870c245da00eb` | 0.2812 | 35 |
| `lfo-trill` | `1570973651a3be86` | 0.2394 | 35 |
| `octave-bass` | `6d230538c5c865dc` | 0.2493 | 35 |
| `rubber-sub` | `35b91bd6144c83fb` | 0.2178 | 35 |
| `reese` | `e58ff7a66197f3ca` | 0.2478 | 35 |
| `punch-bass` | `50aae7624bcce3fc` | 0.1563 | 35 |
| `hollow-bass` | `68b8b78190089b65` | 0.3973 | 35 |
| `fat-saw-lead` | `e10c5edafdec360c` | 0.3344 | 35 |
| `screamer` | `52539a9f393973f8` | 0.2650 | 35 |
| `whistle` | `cef25a5aff2ea73e` | 0.2440 | 35 |
| `pulse-lead` | `7aacc3480dc6fdd7` | 0.2213 | 35 |
| `octave-lead` | `3f82097d58162ce8` | 0.3182 | 35 |
| `glide-brass` | `3a9b6bd785b70af1` | 0.3190 | 35 |
| `soft-horn` | `83a19955ba627384` | 0.3432 | 35 |
| `warm-pad` | `0b76874c38f8db50` | 0.2694 | 35 |
| `square-pad` | `4ee6a32b0e9e1048` | 0.3727 | 35 |
| `sweep-pad` | `b5eb531f5b9c8450` | 0.2700 | 35 |
| `breathing-pad` | `6d02aa14477713bc` | 0.2184 | 35 |
| `choir` | `ecbc65e731749be7` | 0.4773 | 35 |
| `electric-piano` | `a13b5bf244357dbd` | 0.3563 | 35 |
| `clav` | `baeb3956f2ecb156` | 0.1391 | 35 |
| `bell` | `08b3cc1f5899d24b` | 0.3791 | 35 |
| `marimba` | `6b3c41af584ce000` | 0.2355 | 35 |
| `kick` | `e746d4b2ac6f82c8` | 0.2207 | 35 |
| `noise-hat` | `798378fc9c514efb` | 0.0654 | 35 |
| `snare` | `1914b69ecd69bf04` | 0.1923 | 35 |
| `zap` | `dfb754dcbf1d3050` | 0.0364 | 35 |
| `siren` | `865932b4c2a2f0ba` | 0.2758 | 35 |
| `random-arp` | `a68808613c2677e6` | 0.2298 | 35 |
| `pulse-gate` | `c78db63cb39376a0` | 0.4265 | 35 |
| `follower-drone` | `4837b279d6a3880f` | 0.2540 | 35 |
| `low-drone` | `5636123a4d8a0c75` | 0.4735 | 35 |

## The player golden

`apps/mxm-player/tests/t4_golden_audio_mono_02.rs`, pinned against the release bundle built from this
tree: **`dfafd491b58bb2ed`**, 96 blocks of stereo. Its sensitivity check — the same score with the
vibrato depth at 0.9 — moves the digest, as it must.

## After the conversion — 2026-09-15

The same `baseline` module on the converted tree. Deterministic, so the machine being busy does not
touch any figure below.

### Factory bank

Rendered by `the_bank_digests` and compared sample by sample with the M0 renders by
`the_bank_against_the_m0_dump`. **Every file now applies 138 parameters**, the routes and
their presences included.

- **18 of 51 unchanged to the bit**, Init among them — the nine init routes at zero render
  exactly what nothing routed renders, which
  `the_machines_own_routes_at_zero_render_bit_identically_to_nothing_routed` holds in the DSP.
- **33 moved, and every move is rounding in size.** The largest is `fat-saw-lead`:
  1.35e-05 absolute, 4.04e-05 of its peak (-88 dB). Consistent with what
  plan §4 predicted: a route multiplies `(amount × source) × scale` where the retired control
  multiplied in its own order, and a design's depth arrives as a signed amount stored `(d + 1) / 2`.
  Nothing moved by more than that, which is §4's line between re-association and a bug.
- **51 of 51 peaks are identical to four decimal places.**
- **The settled-route read and the zero-depth skip that came after the conversion moved nothing**:
  all 51 digests were re-rendered after them and match the conversion's own renders.

| Sound | Digest | Peak | Largest difference from M0: absolute, and of the peak |
|---|---|---|---|
| `init` | `dd50dbf4dc1d9353` | 0.2533 | unchanged |
| `init-saw` | `ca786fd247e8fbdd` | 0.2481 | unchanged |
| `beating-saws` | `d337e73681b1d7e0` | 0.4582 | unchanged |
| `sub-bass` | `177da02ed8393353` | 0.4143 | 2.24e-07, 5.39e-07 |
| `square-lead` | `9544d757092ca3ca` | 0.3331 | 3.63e-06, 1.09e-05 |
| `pulse-sweep` | `e2ef37329054095d` | 0.7418 | 5.36e-07, 7.23e-07 |
| `sine-flute` | `08593ae176455a22` | 0.2430 | 6.00e-07, 2.47e-06 |
| `noise-wash` | `d7185606615234f7` | 0.1147 | 4.52e-07, 3.94e-06 |
| `bent-brass` | `f902a2cbce143f69` | 0.4420 | 1.65e-06, 3.74e-06 |
| `delayed-vibrato` | `15beaf532993e316` | 0.3367 | 1.15e-06, 3.43e-06 |
| `shuffle-bass` | `46f2620ae22e70b1` | 0.0993 | unchanged |
| `organ-hold` | `e8afeae9363307c5` | 0.5025 | unchanged |
| `random-filter` | `bd0d1fc876856b7b` | 0.1962 | unchanged |
| `gate-stab` | `5fa7d4e0073bb721` | 0.2919 | 1.49e-07, 5.10e-07 |
| `auto-dip` | `af6d45c0aa5fd3a9` | 0.1333 | 4.38e-07, 3.28e-06 |
| `hollow-fifths` | `e676635fd1a0eb98` | 0.5009 | unchanged |
| `wide-detune` | `ae3a40fc777019e1` | 0.3183 | unchanged |
| `inverted-pluck` | `de14bcb5c051d532` | 0.1149 | 2.24e-08, 1.95e-07 |
| `tracking-sweep` | `650e44115693c6ba` | 0.1531 | 1.19e-07, 7.79e-07 |
| `slow-glide` | `fe9870c245da00eb` | 0.2812 | unchanged |
| `lfo-trill` | `1570973651a3be86` | 0.2394 | unchanged |
| `octave-bass` | `0d05bdacabff3ae3` | 0.2493 | 7.45e-08, 2.99e-07 |
| `rubber-sub` | `c8f4c73a1c1108cf` | 0.2178 | 7.45e-08, 3.42e-07 |
| `reese` | `e58ff7a66197f3ca` | 0.2478 | unchanged |
| `punch-bass` | `4cb499aa30d47458` | 0.1563 | 2.98e-08, 1.91e-07 |
| `hollow-bass` | `708121fbda366697` | 0.3973 | 1.79e-07, 4.50e-07 |
| `fat-saw-lead` | `12ac09336db9ff8a` | 0.3344 | 1.35e-05, 4.04e-05 |
| `screamer` | `5b20ea4867dd2466` | 0.2650 | 1.74e-06, 6.58e-06 |
| `whistle` | `e43a2e348512f3bd` | 0.2440 | 5.44e-07, 2.23e-06 |
| `pulse-lead` | `7aacc3480dc6fdd7` | 0.2213 | unchanged |
| `octave-lead` | `3f82097d58162ce8` | 0.3182 | unchanged |
| `glide-brass` | `3a9b6bd785b70af1` | 0.3190 | unchanged |
| `soft-horn` | `3bae011e0686791e` | 0.3432 | 2.73e-06, 7.96e-06 |
| `warm-pad` | `4d8c0d17b642a8aa` | 0.2694 | 1.73e-06, 6.43e-06 |
| `square-pad` | `c9ca731eca59ed33` | 0.3727 | 2.06e-06, 5.53e-06 |
| `sweep-pad` | `c4433e8212d04214` | 0.2700 | 1.57e-07, 5.80e-07 |
| `breathing-pad` | `c0667fa9003e81e6` | 0.2184 | 1.49e-07, 6.82e-07 |
| `choir` | `1b6d2dab5c8f0548` | 0.4773 | 2.09e-07, 4.37e-07 |
| `electric-piano` | `ec272b335699e1ca` | 0.3563 | 1.79e-07, 5.02e-07 |
| `clav` | `74e43fd96182758d` | 0.1391 | 7.82e-08, 5.62e-07 |
| `bell` | `08b3cc1f5899d24b` | 0.3791 | unchanged |
| `marimba` | `26ff8d097512ff23` | 0.2355 | 2.98e-08, 1.27e-07 |
| `kick` | `90374a82bd9ce65f` | 0.2207 | 1.49e-08, 6.75e-08 |
| `noise-hat` | `798378fc9c514efb` | 0.0654 | unchanged |
| `snare` | `6364fea1cae3cf5d` | 0.1923 | 2.31e-07, 1.20e-06 |
| `zap` | `873924675ede5fbb` | 0.0364 | 2.61e-07, 7.16e-06 |
| `siren` | `d152109af0271c95` | 0.2758 | 6.96e-06, 2.52e-05 |
| `random-arp` | `a68808613c2677e6` | 0.2298 | unchanged |
| `pulse-gate` | `5ed2fc2aee65d632` | 0.4265 | 2.09e-07, 4.89e-07 |
| `follower-drone` | `4837b279d6a3880f` | 0.2540 | unchanged |
| `low-drone` | `a70fbb1fc466e9c5` | 0.4735 | 2.09e-07, 4.41e-07 |

### The player golden

**`cc5225cc63e39d8d`, provisionally repinned**, where M0 pinned `dfafd491b58bb2ed`. Against the M0
render the largest sample difference is 2.2e-5, 9.6e-5 of the score's peak (−80 dB), on 69 946 of
98 304 samples — rounding in size, as the bank's are. The WAV was byte-identical before and after
the settled-route change. The sensitivity check still moves the digest. **No listening is claimed**:
the owner's pass at plan M5 confirms the pin or replaces it.

### Throughput

**Not recorded**, for the reason *Throughput* above gives.
