//! mxm-mono-02 filter spike: measure what the SH-2's cascade actually does.
//!
//! mono-01's spike with two sections added: the service notes' calibration (7), and
//! the DC droop against resonance that mono-01 keeps in `resonance_gain` (8).
//!
//! A discretised loop with a saturator in it does not inherit the ideal ladder's
//! numbers, so this reports the real ones instead of assuming `k = 4`. Sections 5
//! and 6 were added with the diode clamp (2026-09-03): they measure the two things
//! the clamp changes — how loud the filter sings, and how clean the loop stays
//! below the knee — which are the numbers the docs quote.
//!
//! Run with: `cargo run -p mxm-mono-02-dsp --release --example mono_02_filter_spike`

use mxm_mono_02_dsp::filter::{
    CLAMP_KNEE, K_MAX, Ladder, OUTPUT_BOUND, STAGE_SPREAD, measure_oscillation_frequency,
    measure_oscillation_threshold, measure_oscillation_threshold_of,
};
use std::f32::consts::PI;

const RATES: [f32; 4] = [44_100.0, 48_000.0, 96_000.0, 192_000.0];

/// Peak gain at `freq`, measured small-signal so the saturators stay linear.
fn magnitude_at(cutoff: f32, resonance: f32, freq: f32, fs: f32) -> f32 {
    magnitude_of(&mut Ladder::new(), cutoff, resonance, freq, fs)
}

/// The same on a given filter — this unit's, or the matched reference (section 9).
fn magnitude_of(f: &mut Ladder, cutoff: f32, resonance: f32, freq: f32, fs: f32) -> f32 {
    const AMP: f32 = 1e-3;
    let settle = (fs * 0.5) as usize;
    let measure = (fs / freq * 8.0) as usize + 64;
    for n in 0..settle {
        let t = n as f32 / fs;
        f.process(AMP * (2.0 * PI * freq * t).sin(), cutoff, resonance, fs);
    }
    let mut peak = 0.0f32;
    for n in settle..settle + measure {
        let t = n as f32 / fs;
        peak = peak.max(
            f.process(AMP * (2.0 * PI * freq * t).sin(), cutoff, resonance, fs)
                .abs(),
        );
    }
    peak / AMP
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// Peak gain at `freq` for an input of amplitude `amp` — *not* small-signal, so
/// the resonance loop's saturator is part of what is measured.
fn gain_at_level(cutoff: f32, resonance: f32, freq: f32, amp: f32, fs: f32) -> f32 {
    let mut f = Ladder::new();
    let settle = (fs * 0.5) as usize;
    let measure = (fs / freq * 8.0) as usize + 64;
    for n in 0..settle {
        let t = n as f32 / fs;
        f.process(amp * (2.0 * PI * freq * t).sin(), cutoff, resonance, fs);
    }
    let mut peak = 0.0f32;
    for n in settle..settle + measure {
        let t = n as f32 / fs;
        peak = peak.max(
            f.process(amp * (2.0 * PI * freq * t).sin(), cutoff, resonance, fs)
                .abs(),
        );
    }
    peak / amp
}

/// Steady self-oscillation peak with no input, after two seconds to settle.
fn self_oscillation_peak(cutoff: f32, resonance: f32, fs: f32) -> f32 {
    let mut f = Ladder::new();
    for _ in 0..(fs * 2.0) as usize {
        f.process(0.0, cutoff, resonance, fs);
    }
    let mut peak = 0.0f32;
    for _ in 0..(fs * 0.25) as usize {
        peak = peak.max(f.process(0.0, cutoff, resonance, fs).abs());
    }
    peak
}

fn main() {
    println!("mxm-mono-02 filter spike (M2a)\n");
    println!(
        "k = {K_MAX} * resonance,  diode clamp knee = {CLAMP_KNEE},  output bound = {OUTPUT_BOUND}\n"
    );

    println!("1. Measured self-oscillation threshold (resonance, and the k it implies)");
    println!("   cutoff      44.1 kHz        48 kHz         96 kHz        192 kHz");
    for fc in [110.0f32, 440.0, 1_000.0, 4_000.0] {
        print!("   {fc:>6.0} Hz  ");
        for fs in RATES {
            let t = measure_oscillation_threshold(fc, fs);
            if t.is_finite() {
                print!("{:.3} (k={:.2})  ", t, t * K_MAX);
            } else {
                print!("    none      ");
            }
        }
        println!();
    }

    println!("\n2. Self-oscillation frequency vs cutoff (resonance = 1.0, 48 kHz)");
    for fc in [110.0f32, 220.0, 440.0, 1_000.0, 2_000.0] {
        let measured = measure_oscillation_frequency(fc, 48_000.0);
        println!(
            "   cutoff {fc:>6.0} Hz -> {measured:>7.1} Hz   (ratio {:.3})",
            measured / fc
        );
    }

    println!("\n3. Magnitude response at 48 kHz, cutoff 1 kHz");
    println!("   freq        res=0.0    res=0.5    res=0.7    res=0.9");
    for f in [62.5f32, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0] {
        print!("   {f:>7.1} Hz");
        for res in [0.0f32, 0.5, 0.7, 0.9] {
            print!("  {:>8.2}", db(magnitude_at(1_000.0, res, f, 48_000.0)));
        }
        println!();
    }

    println!("\n4. Worst-case output, resonance = 1.0, cutoff swept 20 Hz -> 20 kHz,");
    println!("   driven with a 110 Hz sine at amplitude 2.0 (60 s per rate)");
    for fs in RATES {
        let mut f = Ladder::new();
        let total = (fs * 60.0) as usize;
        let mut peak = 0.0f32;
        let mut nonfinite = false;
        for n in 0..total {
            let t = n as f32 / total as f32;
            let cutoff = 20.0 * (20_000.0f32 / 20.0).powf(t);
            let x = ((n as f32 / fs) * 2.0 * PI * 110.0).sin() * 2.0;
            let y = f.process(x, cutoff, 1.0, fs);
            if !y.is_finite() {
                nonfinite = true;
                break;
            }
            peak = peak.max(y.abs());
        }
        println!(
            "   {fs:>9.0} Hz  peak |y| = {peak:.4}  ({:.2} dB)  bound {OUTPUT_BOUND}  {}",
            db(peak),
            if nonfinite {
                "NON-FINITE"
            } else if peak < OUTPUT_BOUND {
                "OK"
            } else {
                "EXCEEDED"
            }
        );
    }

    println!("\n5. Self-oscillation level, no input, 48 kHz (the clamp sets this)");
    println!("   resonance   k       440 Hz peak    1 kHz peak");
    for res in [0.9f32, 0.92, 0.95, 1.0] {
        println!(
            "   {res:>6.2}    {:>5.2}    {:>8.4}       {:>8.4}",
            res * K_MAX,
            self_oscillation_peak(440.0, res, 48_000.0),
            self_oscillation_peak(1_000.0, res, 48_000.0)
        );
    }

    println!("\n6. Gain at cutoff against input level, 48 kHz, cutoff 1 kHz");
    println!("   (a clamp is flat below its knee and then limits; tanh compresses everywhere)");
    println!("   resonance   amp=0.001   amp=0.03   amp=0.08   amp=0.3    amp=0.6    amp=1.0");
    for res in [0.3f32, 0.6, 0.8] {
        print!("   {res:>6.2}   ");
        for amp in [0.001f32, 0.03, 0.08, 0.3, 0.6, 1.0] {
            print!(
                "  {:>7.2} dB",
                db(gain_at_level(1_000.0, res, 1_000.0, amp, 48_000.0))
            );
        }
        println!();
    }

    println!(
        "
7. Calibration: onset of self-oscillation on the control (the SH-2's 7-9)"
    );
    println!("   cutoff        44.1 kHz   48 kHz   96 kHz   192 kHz");
    for fc in [40.0f32, 100.0, 1_000.0, 10_000.0, 19_000.0] {
        print!("   {fc:>8.0} Hz  ");
        for fs in RATES {
            let t = measure_oscillation_threshold(fc, fs);
            print!("   {t:>6.3}");
        }
        println!();
    }

    println!(
        "
8. DC droop: level of a 110 Hz tone, cutoff wide open, against 1/(1+k)"
    );
    println!("   resonance     k      level     vs k=0     1/(1+k)");
    let level = |res: f32| gain_at_level(19_000.0, res, 110.0, 0.1, 48_000.0);
    let base = level(0.0);
    for res in [0.0f32, 0.2, 0.4, 0.6, 0.7] {
        let k = res * K_MAX;
        let l = level(res);
        println!(
            "   {res:>6.2}    {k:>5.2}   {l:>7.4}   {:>7.2} dB   {:>7.2} dB",
            db(l / base),
            db(1.0 / (1.0 + k))
        );
    }

    println!(
        "
9. This unit's stage spread (±{STAGE_SPREAD}) against a matched reference, 48 kHz, cutoff 1 kHz"
    );
    println!("   trims: {:?}", Ladder::new().trim());
    println!("   resonance    peak at cutoff: unit    matched    difference");
    for res in [0.3f32, 0.5, 0.7, 0.78] {
        let unit = db(magnitude_of(
            &mut Ladder::new(),
            1_000.0,
            res,
            1_000.0,
            48_000.0,
        ));
        let matched = db(magnitude_of(
            &mut Ladder::matched(),
            1_000.0,
            res,
            1_000.0,
            48_000.0,
        ));
        println!(
            "   {res:>6.2}     {unit:>9.2} dB   {matched:>9.2} dB   {:>+7.3} dB",
            unit - matched
        );
    }
    println!("   onset on the control:  unit    matched");
    for fc in [40.0f32, 1_000.0, 19_000.0] {
        println!(
            "   {fc:>8.0} Hz            {:.4}  {:.4}",
            measure_oscillation_threshold(fc, 48_000.0),
            measure_oscillation_threshold_of(Ladder::matched, fc, 48_000.0)
        );
    }
}
