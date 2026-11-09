//! The grid: hit testing, drawing, and mouse interaction.

use std::collections::HashMap;

use egui::{Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};

use super::library::{badge_fill, block_tooltip, fmt_value, part_description};
use super::model::{
    self, BlockRef, Builtin, GRID, Instance, Item, Line, OpticComponentKind, PortRef, Terminal, box_geometry,
    part_signal_in, part_signal_out,
};
use super::render::{self, signal_color};
use crate::panels::prefs::Palette;
use super::{Drag, OpticsPanel, Placeable, Request, Tool, WireRef, input_number, output_number};
use crate::panels::circuit::sim::Outcome;
use crate::panels::project::BlockDisplay;
use crate::panels::ui_kit::{self, CanvasTheme};

/// What the cursor is over, most specific first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    SignalOut(Terminal),
    SignalIn(Terminal),
    LightOut(PortRef),
    LightIn(PortRef),
    Part(usize),
    /// Index into [`model::Layout::instances`].
    Box(usize),
    Waveguide(usize),
    Signal(usize),
    Empty,
}

impl Hit {
    fn is_port(self) -> bool {
        matches!(self, Hit::SignalOut(_) | Hit::SignalIn(_) | Hit::LightOut(_) | Hit::LightIn(_))
    }
}

/// Points along a waveguide from an output (right edge) to an input (left edge).
pub fn waveguide_path(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let dx = ((b.x - a.x).abs() / 2.0).max(GRID);
    let (c1, c2) = (a + Vec2::new(dx, 0.0), b - Vec2::new(dx, 0.0));
    (0..=24)
        .map(|k| {
            let t = k as f32 / 24.0;
            let u = 1.0 - t;
            (a.to_vec2() * (u * u * u) + c1.to_vec2() * (3.0 * u * u * t) + c2.to_vec2() * (3.0 * u * t * t) + b.to_vec2() * (t * t * t))
                .to_pos2()
        })
        .collect()
}

fn near_path(p: Pos2, path: &[Pos2], tolerance: f32) -> bool {
    path.windows(2).any(|w| render::distance_to_segment(p, w[0], w[1]) < tolerance)
}

impl OpticsPanel {
    fn geometry(&self, inst: &Instance) -> model::BoxGeometry {
        box_geometry(inst.cell, self.counts(&inst.block))
    }

    /// Where a signal terminal sits, in world units.
    pub fn terminal_pos(&self, t: Terminal, output: bool) -> Option<Pos2> {
        match t.item {
            Item::Part(i) => {
                let pos = self.layout.components.get(i)?.pos;
                Some(if output { part_signal_out(pos) } else { part_signal_in(pos) })
            }
            Item::Box(id) => {
                let g = self.geometry(self.layout.instance(id)?);
                if output { g.signal_out.get(t.port).copied() } else { g.signal_in.get(t.port).copied() }
            }
        }
    }

    /// Where a box's light port sits, in world units.
    pub fn light_port_pos(&self, p: PortRef, output: bool) -> Option<Pos2> {
        let g = self.geometry(self.layout.instance(p.instance)?);
        if output { g.optical_out.get(p.port).copied() } else { g.optical_in.get(p.port).copied() }
    }

    /// Every port and terminal with its world position.
    pub fn ports(&self) -> Vec<(Hit, Pos2)> {
        let mut out = Vec::new();
        for (i, c) in self.layout.components.iter().enumerate() {
            for k in 0..c.kind.signal_inputs().len() {
                out.push((Hit::SignalIn(Terminal { item: Item::Part(i), port: k }), part_signal_in(c.pos)));
            }
            for k in 0..c.kind.signal_outputs().len() {
                out.push((Hit::SignalOut(Terminal { item: Item::Part(i), port: k }), part_signal_out(c.pos)));
            }
        }
        for inst in &self.layout.instances {
            let g = self.geometry(inst);
            let t = |port| Terminal { item: Item::Box(inst.id), port };
            let p = |port| PortRef { instance: inst.id, port };
            out.extend(g.signal_in.iter().enumerate().map(|(k, &at)| (Hit::SignalIn(t(k)), at)));
            out.extend(g.signal_out.iter().enumerate().map(|(k, &at)| (Hit::SignalOut(t(k)), at)));
            out.extend(g.optical_in.iter().enumerate().map(|(k, &at)| (Hit::LightIn(p(k)), at)));
            out.extend(g.optical_out.iter().enumerate().map(|(k, &at)| (Hit::LightOut(p(k)), at)));
        }
        out
    }

    pub fn waveguide_points(&self, w: usize) -> Option<Vec<Pos2>> {
        let wire = self.layout.wires.get(w)?;
        Some(waveguide_path(self.light_port_pos(wire.from, true)?, self.light_port_pos(wire.to, false)?))
    }

    pub fn signal_points(&self, w: usize) -> Option<Vec<Pos2>> {
        self.signal_routes().into_iter().nth(w).flatten()
    }

    /// Routes of every signal wire. Wires running between the same two rows
    /// get their own lanes, shared by wires from the same output (a bus).
    pub fn signal_routes(&self) -> Vec<Option<Vec<Pos2>>> {
        let ends: Vec<Option<(Pos2, Pos2)>> = self
            .layout
            .signals
            .iter()
            .map(|w| Some((self.terminal_pos(w.from, true)?, self.terminal_pos(w.to, false)?)))
            .collect();
        let band = |(a, b): (Pos2, Pos2)| ((a.y / 4.0).round() as i32, (b.y / 4.0).round() as i32, b.y < a.y + 20.0);
        let mut sources: HashMap<(i32, i32, bool), Vec<(i32, Terminal)>> = HashMap::new();
        for (w, end) in self.layout.signals.iter().zip(&ends) {
            if let Some(end) = *end {
                let list = sources.entry(band(end)).or_default();
                let key = ((end.0.x * 4.0).round() as i32, w.from);
                if !list.contains(&key) {
                    list.push(key);
                }
            }
        }
        for list in sources.values_mut() {
            list.sort_by_key(|k| k.0);
        }
        self.layout
            .signals
            .iter()
            .zip(ends)
            .map(|(w, end)| {
                let end = end?;
                let list = &sources[&band(end)];
                let lane = list.iter().position(|k| k.1 == w.from).unwrap_or(0);
                Some(render::signal_route_in_lane(end.0, end.1, lane, list.len(), 1.0))
            })
            .collect()
    }

    /// What is at `world`, with tolerances in screen points.
    pub fn hit(&self, world: Pos2) -> Hit {
        let tolerance = 8.0 / self.view.zoom;
        if let Some((hit, _)) = self
            .ports()
            .into_iter()
            .map(|(h, at)| (h, at.distance(world)))
            .filter(|(_, d)| *d < tolerance)
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            return hit;
        }
        if let Some(i) = self.layout.component_at(world) {
            return Hit::Part(i);
        }
        let catalog = &self.catalog;
        if let Some(b) = self.layout.instance_at(world, &|b| catalog.counts(b)) {
            return Hit::Box(b);
        }
        let wire_tolerance = 5.0 / self.view.zoom;
        for (w, path) in self.signal_routes().into_iter().enumerate() {
            if path.is_some_and(|path| near_path(world, &path, wire_tolerance)) {
                return Hit::Signal(w);
            }
        }
        for w in 0..self.layout.wires.len() {
            if self.waveguide_points(w).is_some_and(|path| near_path(world, &path, wire_tolerance)) {
                return Hit::Waveguide(w);
            }
        }
        Hit::Empty
    }

    fn signal_value(&self, t: Terminal, output: bool) -> Option<f64> {
        let o = self.outcome().filter(|o| o.error.is_none())?;
        if output {
            Some(o.signal(t.item, t.port))
        } else {
            self.layout.signal_source(t).map(|from| o.signal(from.item, from.port))
        }
    }

    fn terminal_name(&self, t: Terminal, output: bool) -> String {
        match t.item {
            Item::Part(i) => {
                let Some(c) = self.layout.components.get(i) else { return String::new() };
                let names = if output { c.kind.signal_outputs() } else { c.kind.signal_inputs() };
                format!("{} · {}", c.kind.name(), names.get(t.port).unwrap_or(&""))
            }
            Item::Box(id) => {
                let Some(inst) = self.layout.instance(id) else { return String::new() };
                let d = self.catalog.display(&inst.block);
                let names = if output { &d.signal_outputs } else { &d.signal_inputs };
                format!("{} · {}", d.name, names.get(t.port).map_or("", String::as_str))
            }
        }
    }

    /// Tooltip for whatever is under the cursor.
    pub fn hover_text(&self, hit: Hit) -> Option<String> {
        let o = self.outcome().filter(|o| o.error.is_none());
        match hit {
            Hit::Part(i) => {
                let c = self.layout.components.get(i)?;
                let mut text = format!("{}\n{}", c.kind.name(), part_description(&c.kind));
                if let Some(v) = self.pin_value(i) {
                    text += &format!("\n\nNow {}: click to toggle and try this layout on its own", fmt_value(v));
                }
                if let Some(p) = self.detector_reading(i) {
                    text += &format!("\n\nPower {}", crate::panels::units::format_both(p, self.prefs.power_unit));
                }
                Some(text)
            }
            Hit::Box(b) => {
                let inst = self.layout.instances.get(b)?;
                let d = self.catalog.display(&inst.block);
                let mut text = block_tooltip(&d);
                if d.component.is_some() {
                    text += "\n\nDouble-click or Tab to open its definition";
                }
                if matches!(inst.block, BlockRef::Builtin(Builtin::NumberInput { .. })) {
                    text += "\n\nClick a bit to toggle it";
                }
                Some(text)
            }
            Hit::SignalIn(t) | Hit::SignalOut(t) => {
                let output = matches!(hit, Hit::SignalOut(_));
                let mut text = format!("{} ({})", self.terminal_name(t, output), if output { "signal out" } else { "signal in" });
                match self.signal_value(t, output) {
                    Some(v) => text += &format!("\nValue {}", fmt_value(v)),
                    None if !output => text += "\nNot connected",
                    None => {}
                }
                Some(text + "\n\nDrag to connect")
            }
            Hit::LightIn(p) | Hit::LightOut(p) => {
                let output = matches!(hit, Hit::LightOut(_));
                let d = self.catalog.display(&self.layout.instance(p.instance)?.block);
                let names = if output { &d.outputs } else { &d.inputs };
                let mut text = format!(
                    "{} · {} (light {})",
                    d.name,
                    names.get(p.port).map_or("", String::as_str),
                    if output { "out" } else { "in" }
                );
                if let Some(pw) = o.and_then(|o| o.port_power.get(&(p.instance, p.port))).filter(|_| output) {
                    text += &format!("\nPower {pw:.4}");
                }
                Some(text + "\n\nDrag to connect with a waveguide")
            }
            Hit::Waveguide(w) => {
                let mut text = "Waveguide".to_owned();
                if let Some(o) = o {
                    for (field, wl) in o.wire_fields.get(w).into_iter().flatten().zip(&o.wavelengths) {
                        text += &format!("\n{wl:.1} nm: power {:.4}", field.norm_sqr());
                    }
                }
                Some(text)
            }
            Hit::Signal(w) => {
                let wire = self.layout.signals.get(w)?;
                let mut text = format!("{} → {}", self.terminal_name(wire.from, true), self.terminal_name(wire.to, false));
                if let Some(v) = self.signal_value(wire.from, true) {
                    text += &format!("\nValue {}", fmt_value(v));
                }
                Some(text)
            }
            Hit::Empty => None,
        }
    }

    // ---- drawing ----------------------------------------------------------

    pub fn canvas_ui(&mut self, ui: &mut Ui) {
        let theme = CanvasTheme::from_visuals(ui.visuals());
        egui::Frame::canvas(ui.style())
            .fill(theme.background)
            .inner_margin(0.0)
            .show(ui, |ui| self.canvas_contents(ui, &theme));
    }

    fn canvas_contents(&mut self, ui: &mut Ui, theme: &CanvasTheme) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.canvas = Some(rect);
        if self.frame_pending {
            self.frame_pending = false;
            self.frame(false);
        }
        // The canvas only has the pointer when nothing (a menu, a popup, a
        // window) is on top of it there; otherwise wheel and clicks are theirs.
        let layer = ui.layer_id();
        let pointer = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p) && ui.ctx().layer_id_at(*p).is_none_or(|l| l == layer));
        self.mouse_screen = pointer;
        self.mouse_world = pointer.map(|p| self.view.to_world(rect, p));

        super::view::navigate(&mut self.view, ui, rect, &response, pointer);
        self.mouse_world = pointer.map(|p| self.view.to_world(rect, p));
        self.interact(ui, rect, &response, pointer);

        let painter = ui.painter_at(rect);
        let zoom = self.view.zoom;
        let to_screen = |p: Pos2| self.view.to_screen(rect, p);
        painter.rect_filled(rect, 0.0, theme.background);
        if self.prefs.show_grid {
            render::draw_grid(&painter, rect, &self.view, theme);
        }

        if self.layout.components.is_empty() && self.layout.instances.is_empty() && self.layout.lines.is_empty() && self.dragging_new.is_none() {
            ui_kit::empty_hint(
                &painter,
                rect,
                theme,
                "Empty layout",
                "Drag parts from the library, press Shift+A, or open an example",
            );
        }

        let hovered = self.mouse_world.filter(|_| self.editor.grab.is_none()).map(|w| self.hit(w)).unwrap_or(Hit::Empty);

        // Hovered cell and placement preview.
        if let Some(world) = self.mouse_world
            && self.editor.grab.is_none()
            && !hovered.is_port()
        {
            let cell = Rect::from_center_size(to_screen(model::snap(world)), Vec2::splat(GRID * zoom));
            painter.rect_filled(cell, 2.0, theme.hover);
            let preview = self.dragging_new.clone().or(match &self.tool {
                Some(Tool::Place(item)) => Some(item.clone()),
                _ => None,
            });
            match preview {
                Some(Placeable::Part(kind)) => render::draw_component(&painter, cell.center(), &kind, 0.45, zoom),
                Some(Placeable::Block(block)) => {
                    let g = box_geometry(model::cell_of(world), self.counts(&block));
                    let r = Rect::from_min_max(to_screen(g.rect.min), to_screen(g.rect.max));
                    painter.rect_stroke(r, 5.0, Stroke::new(1.5, theme.active.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                }
                Some(Placeable::Example(_)) => {
                    painter.rect_stroke(cell, 5.0, Stroke::new(1.5, theme.active.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                }
                None => {}
            }
        }

        let line_stroke = Stroke::new(2.0, Color32::from_rgb(214, 60, 60));
        for line in &self.layout.lines {
            painter.line_segment([to_screen(line.start), to_screen(line.end)], line_stroke);
        }
        if let (Drag::Line { start }, Some(world)) = (self.drag, self.mouse_world) {
            painter.line_segment(
                [to_screen(start), to_screen(model::snap(world))],
                Stroke::new(2.0, line_stroke.color.gamma_multiply(0.5)),
            );
        }

        let outcome = self.outcome().filter(|o| o.error.is_none()).cloned();

        // Boxes.
        for (b, inst) in self.layout.instances.iter().enumerate() {
            let g = self.geometry(inst);
            let r = Rect::from_min_max(to_screen(g.rect.min), to_screen(g.rect.max));
            if !rect.intersects(r.expand(20.0)) {
                continue;
            }
            let d = self.catalog.display(&inst.block);
            draw_box(&painter, r, inst, &d, outcome.as_ref(), &self.layout, theme, zoom, hovered == Hit::Box(b), self.prefs.show_port_names);
            if self.editor.boxes.contains(b) {
                let active = self.editor.boxes.active() == Some(b);
                painter.rect_stroke(
                    r.expand(3.0),
                    6.0,
                    Stroke::new(if active { 2.0 } else { 1.5 }, if active { theme.active } else { theme.selection }),
                    egui::StrokeKind::Outside,
                );
            }
        }

        // Colours for this frame's light: every wavelength simulated or emitted.
        let laser_wavelengths = self.layout.components.iter().filter_map(|c| match c.kind {
            OpticComponentKind::Laser { wavelength, .. } => Some(wavelength as f64),
            _ => None,
        });
        let palette = Palette::new(
            &self.prefs,
            outcome.iter().flat_map(|o| o.wavelengths.iter().copied()).chain(laser_wavelengths),
        );

        // Waveguides, one coloured strand per lit channel.
        for w in 0..self.layout.wires.len() {
            let Some(path) = self.waveguide_points(w) else { continue };
            let base = theme.major_line.gamma_multiply(1.6);
            let mut lit: Vec<(f64, Color32)> = outcome
                .as_ref()
                .and_then(|o| o.wire_fields.get(w).map(|fields| (o, fields)))
                .map(|(o, fields)| {
                    fields
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.norm_sqr() > 1e-9)
                        .map(|(k, f)| {
                            let color = palette.color(o.wavelengths[k], o.channel_colors.get(k).copied().flatten());
                            (f.norm_sqr(), color.gamma_multiply((f.norm() as f32).clamp(0.35, 1.0)))
                        })
                        .collect()
                })
                .unwrap_or_default();
            lit.sort_by(|a, b| b.0.total_cmp(&a.0));
            let selected = self.selected_wire == Some(WireRef::Waveguide(w));
            let width = (if selected || hovered == Hit::Waveguide(w) { 4.5 } else { 3.0 }) * zoom.clamp(0.5, 1.5);
            let points: Vec<Pos2> = path.into_iter().map(to_screen).collect();
            if selected {
                painter.add(Shape::line(points.clone(), Stroke::new(width + 3.0, theme.active.gamma_multiply(0.5))));
            }
            if self.prefs.channel_strands && lit.len() > 1 {
                let strand = (width * 0.8).max(1.5);
                let n = lit.len() as f32;
                for (k, (_, color)) in lit.iter().enumerate() {
                    let shift = (k as f32 - (n - 1.0) / 2.0) * strand;
                    painter.add(Shape::line(offset_path(&points, shift), Stroke::new(strand, *color)));
                }
            } else {
                let color = lit.first().map_or(base, |l| l.1);
                painter.add(Shape::line(points, Stroke::new(width, color)));
            }
        }

        // Beams and readings.
        let mut beam_labels = Vec::new();
        if let Some(o) = &outcome {
            beam_labels = render::draw_beams(
                &painter,
                rect,
                &self.view,
                &o.beams,
                pointer,
                &render::BeamStyle { palette: &palette, strands: self.prefs.channel_strands, arrows: self.prefs.beam_arrows },
            );
            for &(i, power) in o.detector_power.iter().filter(|_| self.prefs.show_readings) {
                let Some(c) = self.layout.components.get(i) else { continue };
                painter.text(
                    to_screen(c.pos) + Vec2::new(14.0, 10.0) * zoom,
                    Align2::LEFT_TOP,
                    crate::panels::units::format_power(power, self.prefs.power_unit),
                    FontId::monospace(10.0),
                    theme.text,
                );
            }
        }

        // Parts.
        for (i, c) in self.layout.components.iter().enumerate() {
            let at = to_screen(c.pos);
            if !rect.expand(GRID * zoom).contains(at) {
                continue;
            }
            let tint = match c.kind {
                OpticComponentKind::Laser { wavelength, color, .. } => Some(palette.color(wavelength as f64, color)),
                _ => None,
            };
            render::draw_component_tinted(&painter, at, &c.kind, 1.0, zoom, tint);
            if let OpticComponentKind::Laser { source: source @ 1.., .. } = c.kind
                && zoom >= 0.6
            {
                // Lasers on a shared source show its letter.
                painter.text(
                    at + Vec2::new(-14.0, -12.0) * zoom,
                    Align2::CENTER_CENTER,
                    source_letter(source),
                    FontId::proportional((10.0 * zoom).clamp(8.0, 14.0)),
                    theme.text,
                );
            }
            if let Some(v) = self.pin_value(i) {
                // Pins light up like a number input's bits.
                let lamp = at + Vec2::new(14.0, -12.0) * zoom;
                if v >= 0.5 {
                    painter.circle_filled(lamp, 7.0 * zoom, render::SIGNAL_HIGH.gamma_multiply(0.3));
                    painter.circle_filled(lamp, 4.5 * zoom, render::SIGNAL_HIGH);
                } else {
                    painter.circle(lamp, 4.5 * zoom, theme.background, Stroke::new(1.0, render::SIGNAL_LOW));
                }
            }
            if let Some(label) = self.layout.label(i) {
                painter.text(
                    at - Vec2::new(0.0, 15.0 * zoom),
                    Align2::CENTER_BOTTOM,
                    label,
                    FontId::proportional((11.0 * zoom).clamp(8.0, 16.0)),
                    theme.text,
                );
            }
            if self.editor.selection.contains(i) {
                let active = self.editor.selection.active() == Some(i);
                painter.rect_stroke(
                    Rect::from_center_size(at, Vec2::splat((GRID - 4.0) * zoom)),
                    4.0,
                    Stroke::new(if active { 2.0 } else { 1.5 }, if active { theme.active } else { theme.selection }),
                    egui::StrokeKind::Inside,
                );
            }
        }

        // Signal wires.
        for (w, path) in self.signal_routes().into_iter().enumerate() {
            let Some(path) = path else { continue };
            let value = self.signal_value(self.layout.signals[w].from, true);
            let selected = self.selected_wire == Some(WireRef::Signal(w));
            let width = if selected || hovered == Hit::Signal(w) { 2.5 } else { 1.5 };
            let points: Vec<Pos2> = path.into_iter().map(to_screen).collect();
            if selected {
                painter.add(Shape::line(points.clone(), Stroke::new(width + 3.0, theme.active.gamma_multiply(0.5))));
            }
            painter.add(Shape::line(points, Stroke::new(width, signal_color(value))));
        }

        // Ports and terminals.
        if zoom >= 0.35 {
            for (hit, at) in self.ports() {
                let screen = to_screen(at);
                if !rect.expand(10.0).contains(screen) {
                    continue;
                }
                let is_hovered = hovered == hit;
                match hit {
                    Hit::SignalOut(t) => {
                        render::draw_terminal(&painter, screen, true, self.signal_value(t, true), is_hovered, zoom, theme.background)
                    }
                    Hit::SignalIn(t) => {
                        render::draw_terminal(&painter, screen, false, self.signal_value(t, false), is_hovered, zoom, theme.background)
                    }
                    Hit::LightIn(p) | Hit::LightOut(p) => {
                        let output = matches!(hit, Hit::LightOut(_));
                        let connected = self.layout.wires.iter().any(|w| if output { w.from == p } else { w.to == p });
                        draw_light_port(&painter, screen, output || connected, is_hovered, zoom, theme);
                    }
                    _ => {}
                }
            }
        }

        // A wire being drawn.
        if let (Drag::Connect(from), Some(world)) = (self.drag, self.mouse_world) {
            let start = match from {
                Hit::SignalOut(t) => self.terminal_pos(t, true),
                Hit::SignalIn(t) => self.terminal_pos(t, false),
                Hit::LightOut(p) => self.light_port_pos(p, true),
                Hit::LightIn(p) => self.light_port_pos(p, false),
                _ => None,
            };
            if let Some(start) = start {
                let (path, color, width) = match from {
                    Hit::SignalOut(_) => (render::signal_route(start, world, 1.0), render::SIGNAL_HIGH, 1.5),
                    Hit::SignalIn(_) => (render::signal_route(world, start, 1.0), render::SIGNAL_HIGH, 1.5),
                    Hit::LightOut(_) => (waveguide_path(start, world), theme.active, 3.0),
                    _ => (waveguide_path(world, start), theme.active, 3.0),
                };
                // Over a port of the other kind: warn and say what fixes it.
                let hint = super::bridge::mismatch_hint(from, hovered);
                let color = if hint.is_some() { ui.visuals().warn_fg_color } else { color };
                painter.add(Shape::line(path.into_iter().map(to_screen).collect(), Stroke::new(width, color.gamma_multiply(0.7))));
                if let (Some(hint), Some(screen)) = (hint, pointer) {
                    let more = if super::bridge::needed(from, hovered).is_some() { " — drop to add one" } else { "" };
                    let text = format!("⚠ {hint}{more}");
                    let galley = painter.layout_no_wrap(text, FontId::proportional(12.5), ui.visuals().warn_fg_color);
                    let at = screen + Vec2::new(14.0, 14.0);
                    painter.rect_filled(Rect::from_min_size(at, galley.size()).expand(5.0), 4.0, theme.background);
                    painter.galley(at, galley, ui.visuals().warn_fg_color);
                }
            }
        }

        if let (Drag::Box { start, .. }, Some(screen)) = (self.drag, pointer) {
            let area = Rect::from_two_pos(to_screen(start), screen);
            painter.rect(
                area,
                0.0,
                Color32::from_rgba_unmultiplied(80, 130, 255, 40),
                Stroke::new(1.0, Color32::from_rgb(80, 130, 255)),
                egui::StrokeKind::Inside,
            );
        }

        // Cursor and tooltip.
        if self.editor.grab.is_none() && self.drag == Drag::None {
            if hovered.is_port() {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            } else if matches!(hovered, Hit::Part(_) | Hit::Box(_)) {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            let mut tips: Vec<String> = self.hover_text(hovered).into_iter().collect();
            if !matches!(hovered, Hit::Part(_) | Hit::Box(_)) {
                tips.extend(beam_labels);
            }
            if !tips.is_empty() && self.dragging_new.is_none() && self.menu.is_none() {
                response.on_hover_text_at_pointer(tips.join("\n\n"));
            }
        }
    }

    // ---- interaction ------------------------------------------------------

    fn select_hit(&mut self, hit: Hit, world: Pos2, extend: bool) {
        self.selected_wire = None;
        let catalog = &self.catalog;
        match hit {
            Hit::Waveguide(w) => {
                self.editor.clear_selection();
                self.selected_wire = Some(WireRef::Waveguide(w));
            }
            Hit::Signal(w) => {
                self.editor.clear_selection();
                self.selected_wire = Some(WireRef::Signal(w));
            }
            _ => {
                self.editor.click_select(&self.layout, world, extend, &|b| catalog.counts(b));
            }
        }
    }

    /// Finish drawing a wire from `from` by dropping it on `to`.
    fn connect(&mut self, from: Hit, to: Hit) {
        if let Some(bridge) = super::bridge::needed(from, to) {
            self.offer_bridge(bridge);
            return;
        }
        match (from, to) {
            (Hit::SignalOut(a), Hit::SignalIn(b)) | (Hit::SignalIn(b), Hit::SignalOut(a)) => {
                self.layout.connect_signal(a, b);
            }
            (Hit::LightOut(a), Hit::LightIn(b)) | (Hit::LightIn(b), Hit::LightOut(a)) => {
                self.layout.connect_optical(a, b);
            }
            (Hit::SignalOut(_) | Hit::SignalIn(_), Hit::LightIn(_) | Hit::LightOut(_))
            | (Hit::LightIn(_) | Hit::LightOut(_), Hit::SignalIn(_) | Hit::SignalOut(_)) => {
                self.notify(
                    "Light and signals don't connect: light → signal goes through a detector, signal → light through a laser wired to the signal",
                );
            }
            (Hit::SignalOut(_), Hit::SignalOut(_)) | (Hit::LightOut(_), Hit::LightOut(_)) => {
                self.notify("Connect an output to an input");
            }
            (Hit::SignalIn(_), Hit::SignalIn(_)) | (Hit::LightIn(_), Hit::LightIn(_)) => {
                self.notify("Connect an input to an output");
            }
            _ => {}
        }
    }

    /// Clicking a number input's bit toggles it.
    fn toggle_bit(&mut self, b: usize, world: Pos2) -> bool {
        let Some(inst) = self.layout.instances.get_mut(b) else { return false };
        let BlockRef::Builtin(Builtin::NumberInput { bits, value }) = &mut inst.block else { return false };
        let column = ((world.x / GRID).floor() as i32 - inst.cell.0) as u32;
        if column >= *bits {
            return false;
        }
        *value ^= 1 << (*bits - 1 - column);
        true
    }

    fn interact(&mut self, ui: &Ui, rect: Rect, response: &egui::Response, pointer: Option<Pos2>) {
        let shift = ui.input(|i| i.modifiers.shift);

        if let Some(g) = &self.editor.grab {
            let tweak = g.tweak;
            if let Some(world) = self.mouse_world {
                self.editor.update_grab(&mut self.layout, world);
            }
            let (pressed_primary, released_primary, pressed_secondary) = ui.input(|i| {
                (
                    i.pointer.button_pressed(egui::PointerButton::Primary),
                    i.pointer.button_released(egui::PointerButton::Primary),
                    i.pointer.button_pressed(egui::PointerButton::Secondary),
                )
            });
            if pressed_secondary {
                self.editor.cancel_grab(&mut self.layout);
                self.swallow_click = true;
            } else if (tweak && released_primary) || (!tweak && pressed_primary && pointer.is_some()) {
                self.confirm_grab();
                self.swallow_click = !tweak;
            }
            return;
        }
        if self.swallow_click {
            if !ui.input(|i| i.pointer.any_down()) {
                self.swallow_click = false;
            }
            return;
        }

        if self.dragging_new.is_some() && ui.input(|i| i.pointer.any_released()) {
            let item = self.dragging_new.take().expect("checked");
            if let Some(world) = self.mouse_world {
                self.place(item, world);
            }
        }

        // Finish drags wherever the button is released, even over another panel.
        if self.drag != Drag::None && !ui.input(|i| i.pointer.primary_down()) {
            let end = ui.input(|i| i.pointer.latest_pos()).map(|p| self.view.to_world(rect, p));
            match (std::mem::replace(&mut self.drag, Drag::None), end) {
                (Drag::Box { start, extend }, Some(end)) => {
                    let catalog = &self.catalog;
                    self.editor.box_select(&self.layout, Rect::from_two_pos(start, end), extend, &|b| catalog.counts(b));
                }
                (Drag::Line { start }, Some(end)) if rect.contains(self.view.to_screen(rect, end)) => {
                    let end = model::snap(end);
                    if end != start {
                        self.layout.lines.push(Line { start, end });
                    }
                }
                (Drag::Connect(from), Some(end)) if rect.contains(self.view.to_screen(rect, end)) => {
                    let to = self.hit(end);
                    self.connect(from, to);
                }
                _ => {}
            }
        }

        let Some(pointer) = pointer else { return };
        let world = self.view.to_world(rect, pointer);

        if response.drag_started_by(egui::PointerButton::Primary) {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
            let start = self.view.to_world(rect, origin);
            let hit = self.hit(start);
            if self.tool == Some(Tool::DrawLine) {
                self.drag = Drag::Line { start: model::snap(start) };
            } else if hit.is_port() {
                self.drag = Drag::Connect(hit);
            } else if let Hit::Part(i) = hit {
                if !self.editor.selection.contains(i) {
                    self.editor.clear_selection();
                    self.editor.selection.select_only(i);
                }
                self.selected_wire = None;
                self.editor.begin_grab(&self.layout, start, true);
                self.editor.update_grab(&mut self.layout, world);
            } else if let Hit::Box(b) = hit {
                if !self.editor.boxes.contains(b) {
                    self.editor.clear_selection();
                    self.editor.boxes.select_only(b);
                }
                self.selected_wire = None;
                self.editor.begin_grab(&self.layout, start, true);
                self.editor.update_grab(&mut self.layout, world);
            } else {
                self.drag = Drag::Box { start, extend: shift };
            }
        }

        if response.clicked_by(egui::PointerButton::Primary) {
            self.menu = None;
            let hit = self.hit(world);
            match hit {
                Hit::Part(i) => {
                    self.select_hit(hit, world, shift);
                    // A signal-in pin is a switch while the layout is tried on its own.
                    let pin = !shift && self.toggle_pin(i);
                    if response.double_clicked() && !pin {
                        self.editor.rotate_selected(&mut self.layout, 1);
                    }
                }
                Hit::Box(b) => {
                    self.select_hit(hit, world, shift);
                    if !shift {
                        self.toggle_bit(b, world);
                    }
                    if response.double_clicked()
                        && let BlockRef::Component(id) = self.layout.instances[b].block
                    {
                        if self.embedded {
                            self.requests.push(Request::Inspect(id));
                        } else {
                            self.notify("Components open in the workspace");
                        }
                    }
                }
                Hit::Empty => match self.tool.clone() {
                    Some(Tool::Place(item)) => self.place(item, world),
                    _ => self.select_hit(hit, world, shift),
                },
                Hit::Waveguide(_) | Hit::Signal(_) => self.select_hit(hit, world, shift),
                _ => {}
            }
        }

        if response.clicked_by(egui::PointerButton::Secondary) {
            let hit = self.hit(world);
            match hit {
                Hit::Part(i) if !self.editor.selection.contains(i) => {
                    self.editor.clear_selection();
                    self.editor.selection.select_only(i);
                }
                Hit::Box(b) if !self.editor.boxes.contains(b) => {
                    self.editor.clear_selection();
                    self.editor.boxes.select_only(b);
                }
                _ => {}
            }
            self.open_menu((hit != Hit::Empty).then_some(hit));
        }
    }
}

fn draw_light_port(painter: &egui::Painter, at: Pos2, filled: bool, hovered: bool, zoom: f32, theme: &CanvasTheme) {
    let radius = if hovered { 6.0 } else { 4.5 } * zoom.clamp(0.6, 1.4);
    let color = if hovered { theme.active } else { Color32::from_rgb(214, 60, 60) };
    if filled {
        painter.circle_filled(at, radius, color);
    } else {
        painter.circle(at, radius, theme.background, Stroke::new(1.5, color));
    }
}

/// Lamps in a row, one per column of the box.
fn draw_leds(painter: &egui::Painter, r: Rect, zoom: f32, lit: impl Iterator<Item = bool>, theme: &CanvasTheme) {
    let step = GRID * zoom;
    for (k, on) in lit.enumerate() {
        let at = Pos2::new(r.left() + (k as f32 + 0.5) * step, r.center().y);
        let radius = step * 0.22;
        if on {
            painter.circle_filled(at, radius * 1.5, render::SIGNAL_HIGH.gamma_multiply(0.25));
            painter.circle_filled(at, radius, render::SIGNAL_HIGH);
        } else {
            painter.circle(at, radius, theme.background, Stroke::new(1.0, theme.major_line.gamma_multiply(1.6)));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_box(
    painter: &egui::Painter,
    r: Rect,
    inst: &Instance,
    d: &BlockDisplay,
    outcome: Option<&Outcome>,
    layout: &model::Layout,
    theme: &CanvasTheme,
    zoom: f32,
    hovered: bool,
    port_names: bool,
) {
    let dark = theme.background.r() < 100;
    let body = if dark { Color32::from_rgb(50, 53, 60) } else { Color32::from_rgb(232, 235, 241) };
    let border = if hovered {
        theme.active
    } else if dark {
        Color32::from_rgb(110, 116, 128)
    } else {
        Color32::from_rgb(150, 156, 168)
    };
    let rounding = 5.0 * zoom.clamp(0.5, 1.5);
    let big = FontId::proportional((15.0 * zoom).clamp(5.0, 40.0));
    let small = FontId::proportional((9.5 * zoom).clamp(5.0, 16.0));
    if let BlockRef::Builtin(Builtin::InputPin { name } | Builtin::OutputPin { name }) = &inst.block {
        let fill = if matches!(inst.block, BlockRef::Builtin(Builtin::InputPin { .. })) {
            Color32::from_rgb(0, 137, 123)
        } else {
            Color32::from_rgb(123, 31, 162)
        };
        painter.rect_filled(r.shrink(4.0 * zoom), r.height() / 2.0, fill);
        painter.text(r.center(), Align2::CENTER_CENTER, name, big, Color32::WHITE);
        return;
    }
    painter.rect(r, rounding, body, Stroke::new(1.2, border), egui::StrokeKind::Inside);
    let value_label = |text: String| {
        let font = FontId::monospace((17.0 * zoom).clamp(9.0, 40.0));
        painter.text(r.right_center() + Vec2::new(8.0 * zoom, 0.0), Align2::LEFT_CENTER, text, font, theme.text);
    };
    match &inst.block {
        BlockRef::Builtin(Builtin::NumberInput { bits, value }) => {
            draw_leds(painter, r, zoom, (0..*bits).rev().map(|k| (value >> k) & 1 == 1), theme);
            value_label(value.to_string());
        }
        BlockRef::Builtin(Builtin::Display { bits }) => {
            let n = outcome.map(|o| input_number(layout, o, inst.id, *bits as usize));
            draw_leds(painter, r, zoom, (0..*bits).rev().map(|k| n.is_some_and(|n| (n >> k) & 1 == 1)), theme);
            value_label(n.map_or("–".into(), |n| format!("= {n}")));
        }
        BlockRef::Builtin(Builtin::Register { bits }) => {
            let n = outcome.map(|o| output_number(o, inst.id, *bits as usize));
            draw_leds(painter, r, zoom, (0..*bits).rev().map(|k| n.is_some_and(|n| (n >> k) & 1 == 1)), theme);
            painter.text(
                Pos2::new(r.right() - GRID * zoom * 0.5, r.center().y),
                Align2::CENTER_CENTER,
                "REG",
                small.clone(),
                theme.weak_text,
            );
            value_label(n.map_or("–".into(), |n| n.to_string()));
        }
        BlockRef::Builtin(Builtin::FunctionCell { name, .. }) => {
            painter.text(r.center(), Align2::CENTER_CENTER, format!("ƒ {name}"), big, theme.text);
        }
        _ => {
            let color = if d.component.is_some() {
                let accent = badge_fill(d);
                painter.rect_filled(
                    Rect::from_min_max(r.left_top(), Pos2::new(r.left() + 4.0 * zoom.clamp(0.5, 1.5), r.bottom())),
                    egui::CornerRadius { nw: rounding as u8, sw: rounding as u8, ne: 0, se: 0 },
                    accent,
                );
                accent
            } else {
                theme.text
            };
            painter.text(r.center(), Align2::CENTER_CENTER, &d.initials, big, color);
        }
    }
    // Port names, when there is room.
    if !port_names || zoom < 0.9 || super::shows_number(&inst.block) {
        return;
    }
    let g = box_geometry(inst.cell, d.counts());
    let to_screen = |p: Pos2| r.min + (p - g.rect.min) * zoom;
    let inset = 4.0 * zoom;
    for (names, points, align, offset) in [
        (&d.signal_inputs, &g.signal_in, Align2::CENTER_TOP, Vec2::new(0.0, inset)),
        (&d.signal_outputs, &g.signal_out, Align2::CENTER_BOTTOM, Vec2::new(0.0, -inset)),
        (&d.inputs, &g.optical_in, Align2::LEFT_CENTER, Vec2::new(inset + 3.0, 0.0)),
        (&d.outputs, &g.optical_out, Align2::RIGHT_CENTER, Vec2::new(-inset - 3.0, 0.0)),
    ] {
        for (name, &at) in names.iter().zip(points) {
            painter.text(to_screen(at) + offset, align, name, small.clone(), theme.weak_text);
        }
    }
}

/// Letter of a shared light source (1 → A).
pub fn source_letter(source: u8) -> String {
    char::from(b'A' + source.saturating_sub(1).min(25)).to_string()
}

/// A polyline shifted sideways by `shift` screen points.
fn offset_path(points: &[Pos2], shift: f32) -> Vec<Pos2> {
    (0..points.len())
        .map(|i| {
            let a = points[i.saturating_sub(1)];
            let b = points[(i + 1).min(points.len() - 1)];
            let d = (b - a).normalized();
            points[i] + Vec2::new(-d.y, d.x) * shift
        })
        .collect()
}
