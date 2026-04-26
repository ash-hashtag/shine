use egui::{Color32, Pos2, Stroke};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum OpticComponentKind {
    Laser {
        amplitude: f32,
        phase: f32,
        wavelength: f32,
        dir: egui::Vec2,
    },
    BeamSplitter {
        reflectivity: f32,
        transparency: f32,
        normal: egui::Vec2,
    },
    Mirror {
        normal: egui::Vec2,
    },
    Lens,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    PlaceComponent(OpticComponentKind),
    DrawLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedComponent {
    pub kind: OpticComponentKind,
    pub pos: Pos2,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub start: Pos2,
    pub end: Pos2,
}

#[derive(Serialize, Deserialize)]
pub struct LayoutData {
    pub components: Vec<PlacedComponent>,
    pub lines: Vec<Line>,
}

pub struct SimulatedBeam {
    pub start: Pos2,
    pub end: Pos2,
    pub amplitude: f32,
    pub phase: f32,
    pub wavelength: f32,
    pub dir: egui::Vec2,
}

pub struct OpticsPanel {
    pub components: Vec<PlacedComponent>,
    pub lines: Vec<Line>,
    pub selected_tool: Option<Tool>,
    pub dragging_new: Option<OpticComponentKind>,
    pub active_line_start: Option<Pos2>,
    pub grid_size: f32,
    pub scroll_offset: egui::Vec2,
    pub simulation_active: bool,
}

impl Default for OpticsPanel {
    fn default() -> Self {
        Self {
            components: Vec::new(),
            lines: Vec::new(),
            selected_tool: None,
            dragging_new: None,
            active_line_start: None,
            grid_size: 40.0,
            scroll_offset: egui::Vec2::ZERO,
            simulation_active: false,
        }
    }
}

impl OpticsPanel {
    fn draw_component_preview(
        painter: &egui::Painter,
        pos: Pos2, // This is now the center of the cell
        kind: OpticComponentKind,
        alpha: u8,
    ) {
        let color = match kind {
            OpticComponentKind::Laser { .. } => Color32::RED,
            OpticComponentKind::BeamSplitter { .. } => Color32::BLUE,
            OpticComponentKind::Mirror { .. } => Color32::GREEN,
            OpticComponentKind::Lens => Color32::from_rgb(100, 150, 255),
        }
        .gamma_multiply(alpha as f32 / 255.0);

        match kind {
            OpticComponentKind::Laser { dir, .. } => {
                let size = 15.0;
                let p1 = pos + dir * size;
                let p2 = pos + egui::vec2(-dir.y, dir.x) * (size * 0.5) - dir * (size * 0.5);
                let p3 = pos + egui::vec2(dir.y, -dir.x) * (size * 0.5) - dir * (size * 0.5);
                painter.add(egui::Shape::convex_polygon(
                    vec![p1, p2, p3],
                    color,
                    Stroke::NONE,
                ));
            }
            OpticComponentKind::BeamSplitter { normal, .. } => {
                let size = 15.0;
                let tangent = egui::vec2(-normal.y, normal.x);
                let thickness = 4.0;
                let p1 = pos + tangent * size + normal * thickness;
                let p2 = pos - tangent * size + normal * thickness;
                let p3 = pos - tangent * size - normal * thickness;
                let p4 = pos + tangent * size - normal * thickness;
                painter.add(egui::Shape::convex_polygon(
                    vec![p1, p2, p3, p4],
                    color,
                    Stroke::new(1.0, Color32::WHITE.gamma_multiply(alpha as f32 / 255.0)),
                ));
            }
            OpticComponentKind::Mirror { normal } => {
                let size = 15.0;
                let tangent = egui::vec2(-normal.y, normal.x);
                let thickness = 4.0;

                let f1 = pos + tangent * size;
                let f2 = pos - tangent * size;
                let b1 = pos + tangent * size - normal * thickness;
                let b2 = pos - tangent * size - normal * thickness;

                painter.add(egui::Shape::convex_polygon(
                    vec![f1, f2, b2, b1],
                    color,
                    Stroke::NONE,
                ));

                let black = Color32::BLACK.gamma_multiply(alpha as f32 / 255.0);
                painter.add(egui::Shape::convex_polygon(
                    vec![b1, b2, b2 - normal * thickness, b1 - normal * thickness],
                    black,
                    Stroke::NONE,
                ));
            }
            OpticComponentKind::Lens => {
                painter.circle_filled(pos, 15.0, color);
                painter.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    "Lens",
                    egui::FontId::proportional(12.0),
                    Color32::WHITE.gamma_multiply(alpha as f32 / 255.0),
                );
            }
        }
    }

    fn simulate_beams(&self) -> Vec<SimulatedBeam> {
        let mut simulated = Vec::new();
        let mut queue = Vec::new();

        // Find all lasers
        for comp in &self.components {
            if let OpticComponentKind::Laser {
                amplitude,
                phase,
                wavelength,
                dir,
            } = comp.kind
            {
                // To avoid immediate self-intersection, push from slightly outside
                queue.push((
                    comp.pos + dir.normalized() * 16.0,
                    dir.normalized(),
                    amplitude,
                    phase,
                    wavelength,
                    0,
                ));
            }
        }

        let max_depth = 10;
        let component_radius = 15.0;

        while let Some((mut ray_start, ray_dir, amplitude, phase, wavelength, depth)) = queue.pop()
        {
            if depth > max_depth || amplitude < 0.01 {
                continue;
            }

            // Advance start slightly to avoid self-intersection with the component it just bounced from
            ray_start = ray_start + ray_dir * 1.0;

            let mut closest_t = f32::MAX;
            let mut hit_comp: Option<&PlacedComponent> = None;

            for comp in &self.components {
                let v_center = ray_start - comp.pos;
                let b = 2.0 * (v_center.x * ray_dir.x + v_center.y * ray_dir.y);
                let c = (v_center.x * v_center.x + v_center.y * v_center.y)
                    - component_radius * component_radius;

                let discriminant = b * b - 4.0 * c;
                if discriminant > 0.0 {
                    let t1 = (-b - discriminant.sqrt()) / 2.0;
                    let t2 = (-b + discriminant.sqrt()) / 2.0;

                    let mut min_t = f32::MAX;
                    if t1 > 0.1 && t1 < min_t {
                        min_t = t1;
                    }
                    if t2 > 0.1 && t2 < min_t {
                        min_t = t2;
                    }

                    if min_t < closest_t {
                        closest_t = min_t;
                        hit_comp = Some(comp);
                    }
                }
            }

            if let Some(comp) = hit_comp {
                let hit_pos = comp.pos;
                simulated.push(SimulatedBeam {
                    start: ray_start,
                    end: hit_pos,
                    amplitude,
                    phase,
                    wavelength,
                    dir: ray_dir,
                });

                match comp.kind {
                    OpticComponentKind::Mirror { normal } => {
                        let dot = ray_dir.x * normal.x + ray_dir.y * normal.y;

                        // If dot > 0, it means the ray is hitting the back side (normal and ray are in same direction)
                        if dot > 0.0 {
                            // Block/Absorb light
                        } else {
                            let mut new_dir = ray_dir - normal * 2.0 * dot;
                            new_dir = new_dir.normalized();
                            let new_phase = phase + std::f32::consts::PI;
                            queue.push((
                                hit_pos + new_dir * 16.0,
                                new_dir,
                                amplitude,
                                new_phase,
                                wavelength,
                                depth + 1,
                            ));
                        }
                    }
                    OpticComponentKind::BeamSplitter {
                        reflectivity,
                        transparency,
                        normal,
                    } => {
                        let dot = ray_dir.x * normal.x + ray_dir.y * normal.y;
                        let mut ref_dir = ray_dir - normal * 2.0 * dot;
                        ref_dir = ref_dir.normalized();
                        let ref_phase = phase + std::f32::consts::PI;
                        queue.push((
                            hit_pos + ref_dir * 16.0,
                            ref_dir,
                            amplitude * reflectivity,
                            ref_phase,
                            wavelength,
                            depth + 1,
                        ));
                        queue.push((
                            hit_pos + ray_dir * 16.0,
                            ray_dir,
                            amplitude * transparency,
                            phase,
                            wavelength,
                            depth + 1,
                        ));
                    }
                    OpticComponentKind::Lens => {
                        queue.push((
                            hit_pos + ray_dir * 16.0,
                            ray_dir,
                            amplitude,
                            phase,
                            wavelength,
                            depth + 1,
                        ));
                    }
                    OpticComponentKind::Laser { .. } => {
                        // Absorb
                    }
                }
            } else {
                let end_pos = ray_start + ray_dir * (self.grid_size * 50.0);
                simulated.push(SimulatedBeam {
                    start: ray_start,
                    end: end_pos,
                    amplitude,
                    phase,
                    wavelength,
                    dir: ray_dir,
                });
            }
        }

        let result = simulated;
        tracing::debug!("Simulated {} beam segments", result.len());
        result
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let mut dropped_component = None;

        ui.allocate_ui(ui.available_size(), |ui| {
            ui.with_layout(
                egui::Layout::left_to_right(egui::Align::Min).with_cross_justify(true),
                |ui| {
                    // Side Menu
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        ui.heading("Components");
                        ui.label(egui::RichText::new("Drag onto grid").small().weak());
                        ui.separator();

                        let component_tools = [
                            (
                                OpticComponentKind::Laser {
                                    amplitude: 1.0,
                                    phase: 0.0,
                                    wavelength: 650.0,
                                    dir: egui::vec2(1.0, 0.0), // pointing right
                                },
                                "🔦 Laser",
                            ),
                            (
                                OpticComponentKind::BeamSplitter {
                                    reflectivity: 0.5,
                                    transparency: 0.5,
                                    normal: egui::vec2(-1.0, -1.0).normalized(), // 45 degrees
                                },
                                "分 Beam Splitter",
                            ),
                            (
                                OpticComponentKind::Mirror {
                                    normal: egui::vec2(-1.0, -1.0).normalized(), // 45 degrees
                                },
                                "🪞 Mirror",
                            ),
                            (OpticComponentKind::Lens, "🔍 Lens"),
                        ];

                        for (kind, label) in component_tools {
                            let tool = Tool::PlaceComponent(kind);
                            let response = ui.add(egui::Button::new(label).fill(
                                if self.selected_tool == Some(tool) {
                                    Color32::from_gray(80)
                                } else {
                                    Color32::TRANSPARENT
                                },
                            ));

                            if response.drag_started() {
                                self.dragging_new = Some(kind);
                            }

                            if response.clicked() {
                                self.selected_tool = if self.selected_tool == Some(tool) {
                                    None
                                } else {
                                    Some(tool)
                                };
                            }
                        }

                        ui.separator();
                        let line_tool = Tool::DrawLine;
                        if ui
                            .add(egui::Button::new("📏 Line").fill(
                                if self.selected_tool == Some(line_tool) {
                                    Color32::from_gray(80)
                                } else {
                                    Color32::TRANSPARENT
                                },
                            ))
                            .clicked()
                        {
                            self.selected_tool = if self.selected_tool == Some(line_tool) {
                                None
                            } else {
                                Some(line_tool)
                            };
                        }

                        ui.add_space(20.0);
                        if ui
                            .button(if self.simulation_active {
                                "⏹ Stop Simulation"
                            } else {
                                "▶ Simulate"
                            })
                            .clicked()
                        {
                            self.simulation_active = !self.simulation_active;
                        }

                        ui.add_space(20.0);
                        if ui.button("🗑 Clear All").clicked() {
                            tracing::info!("Clearing all components and lines");
                            self.components.clear();
                            self.lines.clear();
                            self.simulation_active = false;
                        }

                        ui.horizontal(|ui| {
                            if ui.button("💾 Save").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Shine Layout", &["json"])
                                    .save_file()
                                {
                                    let data = LayoutData {
                                        components: self.components.clone(),
                                        lines: self.lines.clone(),
                                    };
                                    match serde_json::to_string_pretty(&data) {
                                        Ok(json) => {
                                            if let Err(e) = std::fs::write(&path, json) {
                                                tracing::error!("Failed to save layout: {}", e);
                                            } else {
                                                tracing::info!("Saved layout to {:?}", path);
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to serialize layout: {}", e)
                                        }
                                    }
                                }
                            }

                            if ui.button("📂 Load").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Shine Layout", &["json"])
                                    .pick_file()
                                {
                                    match std::fs::read_to_string(&path) {
                                        Ok(json) => {
                                            match serde_json::from_str::<LayoutData>(&json) {
                                                Ok(data) => {
                                                    self.components = data.components;
                                                    self.lines = data.lines;
                                                    tracing::info!("Loaded layout from {:?}", path);
                                                }
                                                Err(e) => {
                                                    tracing::error!("Failed to parse layout: {}", e)
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to read layout file: {}", e)
                                        }
                                    }
                                }
                            }
                        });

                        ui.separator();
                        ui.label("Drag & Drop to place");
                        ui.label("Right-click: Remove");
                    });

                    ui.separator();

                    // Grid Area - Fill remaining space
                    egui::Frame::canvas(ui.style())
                        .fill(Color32::WHITE)
                        .show(ui, |ui| {
                            let (rect, response) =
                                ui.allocate_exact_size(ui.available_size(), egui::Sense::all());
                            let painter = ui.painter_at(rect);
                            let grid_size = 40.0;

                            // Panning logic
                            if response.dragged_by(egui::PointerButton::Middle) {
                                self.scroll_offset += response.drag_delta();
                            }

                            // Handle interactions and calculate hover state
                            let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                            let mut snapped_pos = None;
                            let mut hovered_cell = None;

                            if let Some(m_pos) = mouse_pos {
                                if rect.contains(m_pos) {
                                    let world_pos = m_pos - self.scroll_offset;
                                    let cell_x = ((world_pos.x - rect.left()) / grid_size).floor();
                                    let cell_y = ((world_pos.y - rect.top()) / grid_size).floor();

                                    // Snap to center of cell (world space)
                                    let s_pos = Pos2::new(
                                        rect.left() + cell_x * grid_size + grid_size / 2.0,
                                        rect.top() + cell_y * grid_size + grid_size / 2.0,
                                    );
                                    snapped_pos = Some(s_pos + self.scroll_offset);

                                    hovered_cell = Some(egui::Rect::from_min_size(
                                        Pos2::new(
                                            rect.left() + cell_x * grid_size + self.scroll_offset.x,
                                            rect.top() + cell_y * grid_size + self.scroll_offset.y,
                                        ),
                                        egui::vec2(grid_size, grid_size),
                                    ));

                                    if ui.input(|i| i.pointer.any_released()) {
                                        if let Some(kind) = self.dragging_new {
                                            dropped_component =
                                                Some(PlacedComponent { kind, pos: s_pos });
                                            self.dragging_new = None;
                                        }

                                        if let Some(start_pos) = self.active_line_start {
                                            if start_pos != s_pos {
                                                self.lines.push(Line {
                                                    start: start_pos,
                                                    end: s_pos,
                                                });
                                            }
                                            self.active_line_start = None;
                                        }
                                    }

                                    if response.drag_started()
                                        && self.selected_tool == Some(Tool::DrawLine)
                                    {
                                        self.active_line_start = Some(s_pos);
                                    }

                                    if response.clicked() && !response.secondary_clicked() {
                                        let click_pos = s_pos;
                                        let mut rotated = false;
                                        for comp in &mut self.components {
                                            if comp.pos.distance(click_pos) < 5.0 {
                                                match &mut comp.kind {
                                                    OpticComponentKind::Laser { dir, .. } => {
                                                        let current_angle = dir.y.atan2(dir.x);
                                                        let new_angle = current_angle
                                                            + std::f32::consts::PI / 2.0;
                                                        *dir = egui::vec2(
                                                            new_angle.cos(),
                                                            new_angle.sin(),
                                                        );
                                                    }
                                                    OpticComponentKind::BeamSplitter {
                                                        normal,
                                                        ..
                                                    } => {
                                                        let current_angle =
                                                            normal.y.atan2(normal.x);
                                                        let new_angle = current_angle
                                                            + std::f32::consts::PI / 4.0;
                                                        *normal = egui::vec2(
                                                            new_angle.cos(),
                                                            new_angle.sin(),
                                                        );
                                                    }
                                                    OpticComponentKind::Mirror { normal } => {
                                                        let current_angle =
                                                            normal.y.atan2(normal.x);
                                                        let new_angle = current_angle
                                                            + std::f32::consts::PI / 4.0;
                                                        *normal = egui::vec2(
                                                            new_angle.cos(),
                                                            new_angle.sin(),
                                                        );
                                                    }
                                                    _ => {}
                                                }
                                                rotated = true;
                                                self.simulation_active = false;
                                                break;
                                            }
                                        }
                                        if !rotated {
                                            if let Some(Tool::PlaceComponent(kind)) =
                                                self.selected_tool
                                            {
                                                dropped_component =
                                                    Some(PlacedComponent { kind, pos: s_pos });
                                            }
                                        }
                                    }

                                    if response.secondary_clicked() {
                                        let click_pos = s_pos; // Using snapped position (local)
                                        let before_count = self.components.len();
                                        self.components.retain(|c| c.pos.distance(click_pos) > 5.0);
                                        if self.components.len() < before_count {
                                            tracing::info!("Removed component at {:?}", click_pos);
                                        }
                                        // Also remove lines near the click point if any
                                        self.lines.retain(|l| {
                                            l.start.distance(s_pos) > 5.0
                                                && l.end.distance(s_pos) > 5.0
                                        });
                                    }
                                }
                            }

                            // Draw highlight
                            if let Some(cell_rect) = hovered_cell {
                                painter.rect(
                                    cell_rect,
                                    0.0,
                                    Color32::from_rgb(200, 220, 255).gamma_multiply(0.3),
                                    Stroke::new(1.0, Color32::from_rgb(100, 150, 255)),
                                    egui::StrokeKind::Inside,
                                );
                            }

                            // Draw grid lines
                            let grid_stroke = Stroke::new(1.0, Color32::from_rgb(0, 0, 139));
                            let start_x = rect.left() + (self.scroll_offset.x % grid_size);
                            let start_y = rect.top() + (self.scroll_offset.y % grid_size);

                            // Vertical lines (loop over width)
                            for i in 0..=(rect.width() / grid_size) as i32 + 1 {
                                let offset = i as f32 * grid_size;
                                painter.line_segment(
                                    [
                                        Pos2::new(start_x + offset, rect.top()),
                                        Pos2::new(start_x + offset, rect.bottom()),
                                    ],
                                    grid_stroke,
                                );
                            }
                            // Horizontal lines (loop over height)
                            for i in 0..=(rect.height() / grid_size) as i32 + 1 {
                                let offset = i as f32 * grid_size;
                                painter.line_segment(
                                    [
                                        Pos2::new(rect.left(), start_y + offset),
                                        Pos2::new(rect.right(), start_y + offset),
                                    ],
                                    grid_stroke,
                                );
                            }

                            // Draw lines
                            let line_stroke = Stroke::new(2.0, Color32::RED);
                            for line in &self.lines {
                                painter.line_segment(
                                    [
                                        line.start + self.scroll_offset,
                                        line.end + self.scroll_offset,
                                    ],
                                    line_stroke,
                                );
                            }

                            // Draw active line preview
                            if let (Some(start_pos), Some(s_pos)) =
                                (self.active_line_start, snapped_pos)
                            {
                                painter.line_segment(
                                    [start_pos + self.scroll_offset, s_pos],
                                    Stroke::new(
                                        line_stroke.width,
                                        line_stroke.color.gamma_multiply(0.5),
                                    ),
                                );
                            }

                            // Simulate and draw beams
                            if self.simulation_active {
                                let simulated_beams = self.simulate_beams();
                                for beam in simulated_beams {
                                    let alpha =
                                        ((beam.amplitude * 255.0) as u32).clamp(0, 255) as u8;
                                    let color = Color32::RED.gamma_multiply(alpha as f32 / 255.0);
                                    let stroke = Stroke::new(2.0, color);

                                    let start_world = beam.start + self.scroll_offset;
                                    let end_world = beam.end + self.scroll_offset;

                                    painter.line_segment([start_world, end_world], stroke);

                                    if let Some(m_pos) = mouse_pos {
                                        let a = start_world;
                                        let b = end_world;
                                        let v = b - a;
                                        let w = m_pos - a;
                                        let v_sq = v.x * v.x + v.y * v.y;
                                        if v_sq > 0.0 {
                                            let t = (w.x * v.x + w.y * v.y) / v_sq;
                                            let t = t.clamp(0.0, 1.0);
                                            let closest_point = a + v * t;
                                            if m_pos.distance(closest_point) < 5.0 {
                                                egui::show_tooltip_text(
                                                    ui.ctx(),
                                                    egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("beam_tooltip")),
                                                    egui::Id::new("beam_tooltip"),
                                                    format!(
                                                        "Amplitude: {:.2}\nPhase: {:.2} π\nWavelength: {:.1} nm",
                                                        beam.amplitude, beam.phase / std::f32::consts::PI, beam.wavelength
                                                    ),
                                                );
                                            }
                                        }
                                    }

                                    let length = beam.start.distance(beam.end);
                                    let num_triangles = (length / 40.0).floor() as usize;
                                    let num_triangles = num_triangles.max(1);

                                    for i in 1..=num_triangles {
                                        let t = if num_triangles == 1 {
                                            0.5
                                        } else {
                                            i as f32 / (num_triangles + 1) as f32
                                        };
                                        let arrow_pos = start_world + beam.dir * (length * t);

                                        let size = 6.0;
                                        let p1 = arrow_pos + beam.dir * size;
                                        let p2 = arrow_pos
                                            + egui::vec2(-beam.dir.y, beam.dir.x) * (size * 0.5)
                                            - beam.dir * (size * 0.5);
                                        let p3 = arrow_pos
                                            + egui::vec2(beam.dir.y, -beam.dir.x) * (size * 0.5)
                                            - beam.dir * (size * 0.5);

                                        painter.add(egui::Shape::convex_polygon(
                                            vec![p1, p2, p3],
                                            color,
                                            Stroke::NONE,
                                        ));
                                    }
                                }
                            }

                            // Draw placed components
                            for comp in &self.components {
                                let draw_pos = comp.pos + self.scroll_offset;
                                if rect.contains(draw_pos) {
                                    Self::draw_component_preview(
                                        &painter, draw_pos, comp.kind, 255,
                                    );
                                }
                            }

                            // Draw drag preview
                            if let Some(kind) = self.dragging_new {
                                if let Some(m_pos) = mouse_pos {
                                    let preview_pos = snapped_pos.unwrap_or(m_pos);
                                    if rect.contains(preview_pos) || self.dragging_new.is_some() {
                                        Self::draw_component_preview(
                                            &painter,
                                            preview_pos,
                                            kind,
                                            128,
                                        );
                                    }
                                }
                            }
                        });
                },
            );
        });
        if let Some(new_comp) = dropped_component {
            if !self
                .components
                .iter()
                .any(|c| c.pos.distance(new_comp.pos) < 5.0)
            {
                tracing::info!(
                    "Placed component: {:?} at {:?}",
                    new_comp.kind,
                    new_comp.pos
                );
                self.components.push(new_comp);
            }
        }

        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selected_tool = None;
            self.dragging_new = None;
            self.active_line_start = None;
        }

        if ui.input(|i| i.pointer.any_released()) {
            self.dragging_new = None;
            self.active_line_start = None;
        }
    }
}
