//! Drawing the canvas: grid, components, beams and overlays.

use crate::panels::units::PowerUnit;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};

use super::model::{GRID, OpticComponentKind};
use super::view::View;
use crate::panels::beam_sim::Beam;
use crate::panels::ui_kit::CanvasTheme;

/// Grid spacing in cells so lines stay at least `min_px` apart, Blender-style
/// (1, 5, 25, ... cells), and the spacing of the emphasised lines.
pub fn grid_steps(zoom: f32, min_px: f32) -> (i32, i32) {
    let mut step = 1;
    while (step as f32) * GRID * zoom < min_px && step < 1_000_000 {
        step *= 5;
    }
    (step, step * 5)
}

pub fn draw_grid(painter: &Painter, canvas: Rect, view: &View, theme: &CanvasTheme) {
    let (step, major) = grid_steps(view.zoom, 8.0);
    let visible = view.visible(canvas);
    let first_i = (visible.min.x / GRID).floor() as i32;
    let last_i = (visible.max.x / GRID).ceil() as i32;
    let first_j = (visible.min.y / GRID).floor() as i32;
    let last_j = (visible.max.y / GRID).ceil() as i32;
    let stroke = |k: i32| {
        if k.rem_euclid(major) == 0 {
            Stroke::new(1.0, theme.major_line)
        } else {
            Stroke::new(1.0, theme.minor_line)
        }
    };
    for i in (first_i..=last_i).filter(|i| i.rem_euclid(step) == 0) {
        let x = view.to_screen(canvas, Pos2::new(i as f32 * GRID, 0.0)).x;
        painter.line_segment(
            [Pos2::new(x, canvas.top()), Pos2::new(x, canvas.bottom())],
            stroke(i),
        );
    }
    for j in (first_j..=last_j).filter(|j| j.rem_euclid(step) == 0) {
        let y = view.to_screen(canvas, Pos2::new(0.0, j as f32 * GRID)).y;
        painter.line_segment(
            [Pos2::new(canvas.left(), y), Pos2::new(canvas.right(), y)],
            stroke(j),
        );
    }
}

pub fn component_color(kind: &OpticComponentKind) -> Color32 {
    match kind {
        OpticComponentKind::Laser { .. } => Color32::RED,
        OpticComponentKind::BeamSplitter { .. } => Color32::BLUE,
        OpticComponentKind::Mirror { .. } => Color32::from_rgb(0, 160, 0),
        OpticComponentKind::Lens => Color32::from_rgb(100, 150, 255),
        OpticComponentKind::PhaseShifter { .. } => Color32::from_rgb(255, 150, 0),
        OpticComponentKind::Attenuator { .. } => Color32::from_gray(120),
        OpticComponentKind::Detector { .. } => Color32::from_rgb(40, 40, 40),
        OpticComponentKind::InputPort { .. } => Color32::from_rgb(0, 137, 123),
        OpticComponentKind::OutputPort => Color32::from_rgb(123, 31, 162),
        OpticComponentKind::SignalIn | OpticComponentKind::SignalOut => SIGNAL_HIGH,
    }
}

/// Draw a component centred at `pos` (screen space), `scale` = view zoom.
pub fn draw_component(painter: &Painter, pos: Pos2, kind: &OpticComponentKind, alpha: f32, scale: f32) {
    draw_component_tinted(painter, pos, kind, alpha, scale, None);
}

/// [`draw_component`] in a given colour (a laser in its light's colour).
pub fn draw_component_tinted(
    painter: &Painter,
    pos: Pos2,
    kind: &OpticComponentKind,
    alpha: f32,
    scale: f32,
    tint: Option<Color32>,
) {
    let color = tint.unwrap_or_else(|| component_color(kind)).gamma_multiply(alpha);
    let white = Color32::WHITE.gamma_multiply(alpha);
    let s = 15.0 * scale;
    let show_text = scale >= 0.5;
    match *kind {
        OpticComponentKind::Laser { dir, .. } => {
            let side = Vec2::new(-dir.y, dir.x);
            painter.add(Shape::convex_polygon(
                vec![
                    pos + dir * s,
                    pos + side * (s * 0.5) - dir * (s * 0.5),
                    pos - side * (s * 0.5) - dir * (s * 0.5),
                ],
                color,
                Stroke::NONE,
            ));
        }
        OpticComponentKind::BeamSplitter { normal, .. } => {
            let tangent = Vec2::new(-normal.y, normal.x);
            let t = 4.0 * scale;
            painter.add(Shape::convex_polygon(
                vec![
                    pos + tangent * s + normal * t,
                    pos - tangent * s + normal * t,
                    pos - tangent * s - normal * t,
                    pos + tangent * s - normal * t,
                ],
                color,
                Stroke::new(1.0, white),
            ));
        }
        OpticComponentKind::Mirror { normal } => {
            let tangent = Vec2::new(-normal.y, normal.x);
            let t = 4.0 * scale;
            let (f1, f2) = (pos + tangent * s, pos - tangent * s);
            let (b1, b2) = (f1 - normal * t, f2 - normal * t);
            painter.add(Shape::convex_polygon(
                vec![f1, f2, b2, b1],
                color,
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                vec![b1, b2, b2 - normal * t, b1 - normal * t],
                Color32::BLACK.gamma_multiply(alpha),
                Stroke::NONE,
            ));
        }
        OpticComponentKind::Lens => {
            painter.circle_filled(pos, s, color);
            if show_text {
                painter.text(
                    pos,
                    Align2::CENTER_CENTER,
                    "Lens",
                    FontId::proportional(12.0 * scale),
                    white,
                );
            }
        }
        OpticComponentKind::PhaseShifter { .. } | OpticComponentKind::Attenuator { .. } => {
            painter.rect_filled(
                Rect::from_center_size(pos, Vec2::new(16.0, 24.0) * scale),
                3.0 * scale,
                color,
            );
            if show_text {
                let label = if matches!(kind, OpticComponentKind::PhaseShifter { .. }) {
                    "φ"
                } else {
                    "A"
                };
                painter.text(
                    pos,
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(13.0 * scale),
                    white,
                );
            }
        }
        OpticComponentKind::InputPort { dir } => {
            let body = Rect::from_center_size(pos, Vec2::splat(22.0 * scale));
            painter.rect_filled(body, 5.0 * scale, color);
            let side = Vec2::new(-dir.y, dir.x);
            let tip = pos + dir * (7.0 * scale);
            painter.add(Shape::convex_polygon(
                vec![
                    tip,
                    tip - dir * (9.0 * scale) + side * (5.0 * scale),
                    tip - dir * (9.0 * scale) - side * (5.0 * scale),
                ],
                white,
                Stroke::NONE,
            ));
        }
        OpticComponentKind::OutputPort => {
            let body = Rect::from_center_size(pos, Vec2::splat(22.0 * scale));
            painter.rect_filled(body, 5.0 * scale, color);
            painter.circle_stroke(pos, 6.0 * scale, Stroke::new(2.0 * scale, white));
        }
        OpticComponentKind::Detector { threshold } => {
            painter.rect_filled(Rect::from_center_size(pos, Vec2::splat(24.0 * scale)), 2.0 * scale, color);
            painter.circle_filled(pos, 7.0 * scale, Color32::from_rgb(90, 200, 120).gamma_multiply(alpha));
            if threshold > 0.0 {
                // A comparator mark: this detector outputs bits.
                let y = pos.y + 9.0 * scale;
                painter.line_segment(
                    [Pos2::new(pos.x - 6.0 * scale, y), Pos2::new(pos.x + 6.0 * scale, y)],
                    Stroke::new(1.5 * scale, SIGNAL_HIGH.gamma_multiply(alpha)),
                );
            }
        }
        OpticComponentKind::SignalIn | OpticComponentKind::SignalOut => {
            let body = Rect::from_center_size(pos, Vec2::new(22.0, 18.0) * scale);
            painter.rect_filled(body, 4.0 * scale, color);
            let down = matches!(kind, OpticComponentKind::SignalIn);
            let (tip, base) = if down { (5.0, -4.0) } else { (-5.0, 4.0) };
            painter.add(Shape::convex_polygon(
                vec![
                    pos + Vec2::new(0.0, tip) * scale,
                    pos + Vec2::new(-5.0, base) * scale,
                    pos + Vec2::new(5.0, base) * scale,
                ],
                Color32::from_rgb(60, 40, 0).gamma_multiply(alpha),
                Stroke::NONE,
            ));
        }
    }
}

/// Colours of electrical signals: low, high, and values between.
pub const SIGNAL_LOW: Color32 = Color32::from_rgb(140, 146, 158);
pub const SIGNAL_HIGH: Color32 = Color32::from_rgb(232, 163, 61);
pub const SIGNAL_ANALOG: Color32 = Color32::from_rgb(86, 150, 220);

pub fn signal_color(value: Option<f64>) -> Color32 {
    match value {
        None | Some(0.0) => SIGNAL_LOW,
        Some(1.0) => SIGNAL_HIGH,
        Some(_) => SIGNAL_ANALOG,
    }
}

/// An electrical terminal: a small square, filled when it is an output.
pub fn draw_terminal(painter: &Painter, at: Pos2, output: bool, value: Option<f64>, hovered: bool, scale: f32, background: Color32) {
    let size = if hovered { 10.0 } else { 7.0 } * scale.clamp(0.6, 1.4);
    let rect = Rect::from_center_size(at, Vec2::splat(size));
    let color = signal_color(value);
    if output || value.is_some() {
        painter.rect_filled(rect, 1.5, color);
    } else {
        painter.rect(rect, 1.5, background, Stroke::new(1.5, color), egui::StrokeKind::Inside);
    }
}

/// Right-angled route for a signal wire from an output (bottom edge) to an
/// input (top edge).
pub fn signal_route(from: Pos2, to: Pos2, scale: f32) -> Vec<Pos2> {
    signal_route_in_lane(from, to, 0, 1, scale)
}

/// Like [`signal_route`], with the horizontal run in `lane` of `lanes`, so
/// parallel wires between the same rows stay apart instead of overlapping.
pub fn signal_route_in_lane(from: Pos2, to: Pos2, lane: usize, lanes: usize, scale: f32) -> Vec<Pos2> {
    let step = 10.0 * scale;
    let lanes = lanes.max(1) as f32;
    let t = (lane as f32 + 0.5) / lanes;
    let a = from + Vec2::new(0.0, step);
    let b = to - Vec2::new(0.0, step);
    if b.y >= a.y {
        let mid = a.y + (b.y - a.y) * t;
        vec![from, Pos2::new(from.x, mid), Pos2::new(to.x, mid), to]
    } else {
        // Going back up: drop out of the source, run beside it (on the
        // boundary between grid columns), then cross over above the target.
        let spread = (3.0 * scale).min(16.0 * scale / lanes);
        let down = a.y + spread * lane as f32;
        let up = b.y - spread * lane as f32;
        let x = from.x - 20.0 * scale + spread * lane as f32 * 0.5;
        vec![from, Pos2::new(from.x, down), Pos2::new(x, down), Pos2::new(x, up), Pos2::new(to.x, up), to]
    }
}

/// Approximate display colour of a wavelength; infrared shows as dark red.
pub fn wavelength_color(nm: f64) -> Color32 {
    let (r, g, b) = match nm {
        w if w < 380.0 => (0.5, 0.0, 1.0),
        w if w < 440.0 => ((440.0 - w) / 60.0, 0.0, 1.0),
        w if w < 490.0 => (0.0, (w - 440.0) / 50.0, 1.0),
        w if w < 510.0 => (0.0, 1.0, (510.0 - w) / 20.0),
        w if w < 580.0 => ((w - 510.0) / 70.0, 1.0, 0.0),
        w if w < 645.0 => (1.0, (645.0 - w) / 65.0, 0.0),
        w if w <= 780.0 => (1.0, 0.0, 0.0),
        _ => (0.55, 0.0, 0.0),
    };
    let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgb(c(r), c(g), c(b))
}

/// Text describing a beam, for tooltips.
pub fn beam_label(beam: &Beam, unit: PowerUnit) -> String {
    format!(
        "λ: {:.1} nm\nPower: {}\nAmplitude: {:.4}\nPhase: {:.3} π",
        beam.wavelength_nm,
        crate::panels::units::format_both(beam.field.norm_sqr(), unit),
        beam.field.norm(),
        beam.field.arg().rem_euclid(std::f64::consts::TAU) / std::f64::consts::PI,
    )
}

/// Distance from `p` to the segment `a`-`b`.
pub fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let v = b - a;
    let len_sq = v.length_sq();
    if len_sq == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(v) / len_sq).clamp(0.0, 1.0);
    p.distance(a + v * t)
}

/// How beams are drawn.
pub struct BeamStyle<'a> {
    pub palette: &'a crate::panels::prefs::Palette,
    /// Several channels on one segment side by side.
    pub strands: bool,
    pub arrows: bool,
    /// Unit for powers in tooltips.
    pub unit: PowerUnit,
}

/// Draw beams; returns labels of beams under `hover` (screen space).
pub fn draw_beams(
    painter: &Painter,
    canvas: Rect,
    view: &View,
    beams: &[Beam],
    hover: Option<Pos2>,
    style: &BeamStyle,
) -> Vec<String> {
    let BeamStyle { palette, strands, arrows, unit } = *style;
    let key = |b: &Beam| {
        let r = |v: f32| (v * 4.0).round() as i32;
        (r(b.start.x), r(b.start.y), r(b.end.x), r(b.end.y))
    };
    let mut per_segment: std::collections::HashMap<(i32, i32, i32, i32), (usize, usize)> = std::collections::HashMap::new();
    for b in beams {
        per_segment.entry(key(b)).or_default().1 += 1;
    }
    let mut hovered = Vec::new();
    for beam in beams {
        let color = palette
            .color(beam.wavelength_nm, beam.color)
            .gamma_multiply((beam.field.norm() as f32).clamp(0.15, 1.0));
        let side = Vec2::new(-beam.dir.y, beam.dir.x);
        let slot = per_segment.get_mut(&key(beam)).expect("counted");
        let offset = if strands && slot.1 > 1 { (slot.0 as f32 - (slot.1 - 1) as f32 / 2.0) * 2.5 } else { 0.0 };
        slot.0 += 1;
        let start = view.to_screen(canvas, beam.start) + side * offset;
        let end = view.to_screen(canvas, beam.end) + side * offset;
        painter.line_segment([start, end], Stroke::new(2.0, color));

        if hover.is_some_and(|m| distance_to_segment(m, start, end) < 5.0) {
            hovered.push(beam_label(beam, unit));
        }
        if !arrows {
            continue;
        }
        // Direction arrows roughly every 40 screen points.
        let length = start.distance(end);
        let count = ((length / 40.0).floor() as usize).clamp(1, 200);
        let size = 6.0;
        for i in 1..=count {
            let t = if count == 1 { 0.5 } else { i as f32 / (count + 1) as f32 };
            let at = start + beam.dir * (length * t);
            if !canvas.contains(at) {
                continue;
            }
            painter.add(Shape::convex_polygon(
                vec![
                    at + beam.dir * size,
                    at + side * (size * 0.5) - beam.dir * (size * 0.5),
                    at - side * (size * 0.5) - beam.dir * (size * 0.5),
                ],
                color,
                Stroke::NONE,
            ));
        }
    }
    hovered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photonics::C64;

    #[test]
    fn grid_steps_adapt_to_zoom() {
        assert_eq!(grid_steps(1.0, 8.0), (1, 5));
        assert_eq!(grid_steps(0.1, 8.0), (5, 25));
        assert_eq!(grid_steps(0.01, 8.0), (25, 125));
        assert_eq!(grid_steps(8.0, 8.0), (1, 5));
    }

    #[test]
    fn wavelength_colors() {
        let c = wavelength_color;
        assert_eq!(c(700.0), Color32::from_rgb(255, 0, 0));
        assert_eq!(c(450.0), Color32::from_rgb(0, 51, 255));
        assert_eq!(c(500.0), Color32::from_rgb(0, 255, 127));
        assert_eq!(c(545.0), Color32::from_rgb(127, 255, 0));
        assert_eq!(c(600.0), Color32::from_rgb(255, 176, 0));
        assert_eq!(c(400.0), Color32::from_rgb(170, 0, 255));
        assert_eq!(c(300.0), Color32::from_rgb(127, 0, 255));
        assert_eq!(c(1550.0), Color32::from_rgb(140, 0, 0));
    }

    #[test]
    fn segment_distance() {
        let (a, b) = (Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0));
        assert_eq!(distance_to_segment(Pos2::new(5.0, 3.0), a, b), 3.0);
        assert_eq!(distance_to_segment(Pos2::new(-4.0, 3.0), a, b), 5.0);
        assert_eq!(distance_to_segment(Pos2::new(13.0, 4.0), a, b), 5.0);
        assert_eq!(distance_to_segment(Pos2::new(3.0, 4.0), a, a), 5.0);
    }

    #[test]
    fn beam_labels() {
        let beam = Beam {
            start: Pos2::ZERO,
            end: Pos2::new(100.0, 0.0),
            dir: Vec2::new(1.0, 0.0),
            wavelength_nm: 650.0,
            field: C64::new(0.0, -0.5),
            color: None,
        };
        assert_eq!(
            beam_label(&beam, PowerUnit::MilliWatt),
            "λ: 650.0 nm\nPower: 0.250 mW (-6.02 dBm)\nAmplitude: 0.5000\nPhase: 1.500 π"
        );
    }

    #[test]
    fn drawing_reports_hovered_beams() {
        let ctx = egui::Context::default();
        let canvas = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0));
        let view = View {
            pan: Vec2::ZERO,
            zoom: 1.0,
        };
        let beams = [Beam {
            start: Pos2::new(0.0, 100.0),
            end: Pos2::new(300.0, 100.0),
            dir: Vec2::new(1.0, 0.0),
            wavelength_nm: 532.0,
            field: C64::ONE,
            color: None,
        }];
        let palette = crate::panels::prefs::Palette::new(&Default::default(), [532.0, 1550.0]);
        let style = BeamStyle { palette: &palette, strands: true, arrows: true, unit: PowerUnit::Dbm };
        ctx.run_ui(Default::default(), |ui| {
            let painter = ui.painter_at(canvas);
            assert_eq!(
                draw_beams(
                    &painter,
                    canvas,
                    &view,
                    &beams,
                    Some(Pos2::new(50.0, 102.0)),
                    &style
                )
                .len(),
                1
            );
            assert!(
                draw_beams(
                    &painter,
                    canvas,
                    &view,
                    &beams,
                    Some(Pos2::new(50.0, 150.0)),
                    &style
                )
                .is_empty()
            );
            assert!(draw_beams(&painter, canvas, &view, &beams, None, &BeamStyle { palette: &palette, strands: false, arrows: false, unit: PowerUnit::MilliWatt }).is_empty());
            // Two channels on one segment are drawn side by side.
            let mut two = beams.to_vec();
            two.push(Beam { wavelength_nm: 1550.0, ..beams[0].clone() });
            assert_eq!(draw_beams(&painter, canvas, &view, &two, Some(Pos2::new(50.0, 101.0)), &style).len(), 2);
            draw_grid(&painter, canvas, &view, &CanvasTheme::light());
            draw_grid(
                &painter,
                canvas,
                &View {
                    pan: Vec2::ZERO,
                    zoom: 0.05,
                },
                &CanvasTheme::dark(),
            );
            for kind in OpticComponentKind::palette() {
                draw_component(&painter, Pos2::new(50.0, 50.0), &kind, 1.0, 1.0);
                draw_component(&painter, Pos2::new(50.0, 50.0), &kind, 0.5, 0.2);
            }
        })
        .textures_delta
        .clear();
    }
}
