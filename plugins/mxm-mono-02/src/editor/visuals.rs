//! The brief's §8 display: the filter's response, and where the sum has taken the cutoff.
//!
//! Draws with theme tokens and never with a literal colour: `crates/ui/AGENTS.md` is explicit that
//! a consumer needing a value the theme does not expose adds the token there rather than the
//! literal here.

use egui::{Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2};
use mxm_ui::theme::Tokens;

/// The height the filter display gets.
pub const HEIGHT: f32 = 84.0;

/// The height the mixer's scope gets: a little more than the filter's, so a waveform reads.
pub const SCOPE_HEIGHT: f32 = 96.0;

/// How many samples the scope shows after its trigger.
const SCOPE_SHOW: usize = 1024;

/// The lowest peak the scope scales to, so silence and near-silence are a flat line rather than
/// a magnified noise floor.
const SCOPE_FLOOR: f32 = 0.05;

/// Where in the buffer a trigger is looked for: the first part, so a full window follows it.
const SCOPE_TRIGGER_SPAN: usize = super::super::telemetry::SCOPE_LEN - SCOPE_SHOW;

/// The plotted frequency span: the hardware's cutoff range, edge to edge.
const PLOT_LOW_HZ: f32 = 10.0;
const PLOT_HIGH_HZ: f32 = 20_000.0;

/// The bottom of the plotted magnitude span, in dB. Fixed: the stopband is always worth seeing.
const PLOT_BOTTOM_DB: f32 = -48.0;

/// The **least** headroom above the passband, in dB. The top is scaled to what the curves reach,
/// because a resonant peak grows without bound as the loop gain approaches threshold.
const PLOT_MIN_TOP_DB: f32 = 12.0;

/// Breathing room above the tallest peak, so it does not touch the frame.
const PLOT_HEADROOM_DB: f32 = 3.0;

/// How finely a curve is sampled.
const POINTS: usize = 160;

/// The four-OTA cascade's response at the slider's position, and — faded — at wherever the
/// modulation sum has actually put it.
///
/// # Why this display earns its place
///
/// The cutoff control shows a **position**, and on this machine the envelope dominates the sum so
/// hard that full depth sends the cutoff past the ceiling before the attack ends (wart 12). The
/// second curve is where the DSP's cutoff *is*, published once per block, so the clip at the top
/// is visible as the curve pinning against the right edge rather than left to be inferred.
///
/// # Declared approximation
///
/// The **linear analytic response** of the ladder, `1 / ((1 + s)⁴ + k)`, normalised so the
/// passband sits at 0 dB. It ignores the input stage and the diode clamp, so a measured sweep at
/// high resonance will not match it exactly, and it hides the `1/(1+k)` droop that is the topology
/// — a curve that sank off the bottom as resonance rose would show it by becoming unreadable.
pub fn filter_response(
    ui: &mut Ui,
    tokens: &Tokens,
    set_hz: f32,
    actual_hz: f32,
    resonance: f32,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, tokens.surface_2);

    let mut curves: Vec<(f32, Color32, f32)> = Vec::new();
    if (actual_hz / set_hz).log2().abs() > 0.05 {
        curves.push((actual_hz, faded(tokens.mod_envelope), 1.0));
    }
    curves.push((set_hz, tokens.accent, 1.5));

    let samples: Vec<Vec<f32>> = curves
        .iter()
        .map(|(hz, _, _)| response_db(*hz, resonance))
        .collect();
    let peak = samples
        .iter()
        .flat_map(|curve| curve.iter().copied())
        .fold(f32::NEG_INFINITY, f32::max);
    let top_db = (peak + PLOT_HEADROOM_DB).max(PLOT_MIN_TOP_DB);

    for (curve, (_, colour, width)) in samples.iter().zip(&curves) {
        plot(&painter, rect, curve, top_db, *colour, *width);
    }

    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, tokens.border),
        egui::StrokeKind::Inside,
    );
}

/// One curve's magnitude, in dB, sampled evenly in log frequency across the plot.
fn response_db(cutoff_hz: f32, resonance: f32) -> Vec<f32> {
    (0..POINTS)
        .map(|i| {
            let t = i as f32 / (POINTS - 1) as f32;
            let hz = PLOT_LOW_HZ * (PLOT_HIGH_HZ / PLOT_LOW_HZ).powf(t);
            magnitude_db(cutoff_hz, resonance, hz)
        })
        .collect()
}

fn plot(
    painter: &egui::Painter,
    rect: Rect,
    curve: &[f32],
    top_db: f32,
    colour: Color32,
    width: f32,
) {
    let span = top_db - PLOT_BOTTOM_DB;
    let points: Vec<Pos2> = curve
        .iter()
        .enumerate()
        .map(|(i, db)| {
            let t = i as f32 / (curve.len() - 1) as f32;
            let y = (top_db - db) / span;
            pos2(
                rect.left() + t * rect.width(),
                rect.top() + y.clamp(0.0, 1.0) * rect.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(width, colour)));
}

/// The ladder's magnitude at one frequency, in dB, passband-normalised.
///
/// `H(jω) = 1 / ((1 + jω/ωc)⁴ + k)` with `k = K_MAX · resonance` — the analogue prototype rather
/// than the running digital filter; the difference is prewarping near Nyquist, off the plot's
/// right edge at every rate the collection supports.
fn magnitude_db(cutoff_hz: f32, resonance: f32, hz: f32) -> f32 {
    let k = mxm_mono_02_dsp::filter::K_MAX * resonance.clamp(0.0, 1.0);
    let ratio = hz / cutoff_hz.max(1.0);
    // (1 + j·ratio)⁴ by repeated complex multiplication.
    let (mut re, mut im) = (1.0f32, 0.0f32);
    for _ in 0..4 {
        let (nr, ni) = (re - im * ratio, re * ratio + im);
        re = nr;
        im = ni;
    }
    re += k;
    let magnitude = 1.0 / (re * re + im * im).sqrt().max(1e-12);
    let dc = 1.0 / (1.0 + k);
    20.0 * (magnitude / dc).max(1e-6).log10()
}

/// A secondary trace's colour: the same hue, quieter, so the base curve reads as the base.
fn faded(colour: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 110)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_passband_sits_at_zero_whatever_the_resonance() {
        for step in 0..=10 {
            let resonance = step as f32 / 10.0;
            let low = magnitude_db(1_000.0, resonance, PLOT_LOW_HZ);
            assert!(low.abs() < 1.0, "at resonance {resonance}: {low} dB");
        }
    }

    #[test]
    fn the_curve_stays_inside_the_plot_at_every_resonance() {
        for step in 0..=20 {
            let resonance = step as f32 / 20.0;
            let curve = response_db(800.0, resonance);
            let peak = curve.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let top_db = (peak + PLOT_HEADROOM_DB).max(PLOT_MIN_TOP_DB);
            let highest = (top_db - peak) / (top_db - PLOT_BOTTOM_DB);
            assert!((0.0..=1.0).contains(&highest) && highest > 0.0);
        }
    }

    #[test]
    fn the_cutoff_is_twelve_db_down_at_zero_resonance() {
        let db = magnitude_db(1_000.0, 0.0, 1_000.0);
        assert!(
            (db + 12.04).abs() < 0.1,
            "four one-poles at their corner: {db} dB"
        );
    }
}

/// The mixer's output, as an oscilloscope: the sum of the three sources, before the filter. What the mixer *does* is add waveforms, and a level knob's number cannot show
/// two saws beating or a sub under them; the trace can.
///
/// **Triggered** on a rising zero crossing found in the buffer's first part, so a periodic wave
/// stands still; with none found — silence, or a wave slower than the window — the trace starts
/// at the buffer's beginning. **Scaled to its own peak**, with a floor, so the shape reads at any
/// level; §7.5's neutral zero line is drawn so the trace's symmetry can be judged.
pub fn mixer_scope(ui: &mut Ui, tokens: &Tokens, samples: &[f32], height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, tokens.surface_2);

    let centre = rect.center().y;
    painter.line_segment(
        [pos2(rect.left(), centre), pos2(rect.right(), centre)],
        Stroke::new(1.0, tokens.border),
    );

    let start = scope_trigger(samples);
    let window = &samples[start..samples.len().min(start + SCOPE_SHOW)];
    if window.len() >= 2 {
        let peak = window
            .iter()
            .fold(0.0f32, |p, s| p.max(s.abs()))
            .max(SCOPE_FLOOR);
        let half = rect.height() / 2.0 - 4.0;
        let points: Vec<Pos2> = window
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let t = i as f32 / (window.len() - 1) as f32;
                pos2(
                    rect.left() + t * rect.width(),
                    centre - (s / peak).clamp(-1.0, 1.0) * half,
                )
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(1.5, tokens.accent)));
    }

    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, tokens.border),
        egui::StrokeKind::Inside,
    );
}

/// The first rising zero crossing in the buffer's first part, or zero when there is none.
fn scope_trigger(samples: &[f32]) -> usize {
    let span = samples.len().min(SCOPE_TRIGGER_SPAN);
    (1..span)
        .find(|&i| samples[i - 1] < 0.0 && samples[i] >= 0.0)
        .unwrap_or(0)
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    #[test]
    fn the_trigger_is_the_first_rising_crossing_in_the_first_part() {
        let mut samples = vec![0.5f32; 2048];
        for (i, s) in samples.iter_mut().enumerate() {
            *s = ((i as f32) * 0.05).sin();
        }
        let start = scope_trigger(&samples);
        assert!(start > 0 && samples[start - 1] < 0.0 && samples[start] >= 0.0);
        assert!(start < SCOPE_TRIGGER_SPAN);
    }

    #[test]
    fn silence_has_no_trigger_and_draws_from_the_start() {
        assert_eq!(scope_trigger(&[0.0; 2048]), 0);
        assert_eq!(scope_trigger(&[]), 0);
    }
}
