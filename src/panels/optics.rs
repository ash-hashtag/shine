use egui::{Color32, Pos2, Stroke};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpticComponentKind {
    Laser,
    BeamSplitter,
    Mirror,
    Lens,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    PlaceComponent(OpticComponentKind),
    DrawLine,
}

pub struct PlacedComponent {
    pub kind: OpticComponentKind,
    pub pos: Pos2,
}

pub struct Line {
    pub start: Pos2,
    pub end: Pos2,
}

pub struct OpticsPanel {
    pub components: Vec<PlacedComponent>,
    pub lines: Vec<Line>,
    pub selected_tool: Option<Tool>,
    pub dragging_new: Option<OpticComponentKind>,
    pub active_line_start: Option<Pos2>,
    pub grid_size: f32,
    pub scroll_offset: egui::Vec2,
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
        let (mut color, label) = match kind {
            OpticComponentKind::Laser => (Color32::RED, "L"),
            OpticComponentKind::BeamSplitter => (Color32::BLUE, "BS"),
            OpticComponentKind::Mirror => (Color32::GREEN, "M"),
            OpticComponentKind::Lens => (Color32::from_rgb(100, 150, 255), "Lens"),
        };
        color = color.gamma_multiply(alpha as f32 / 255.0);

        painter.circle_filled(pos, 15.0, color);
        painter.text(
            pos,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            Color32::WHITE.gamma_multiply(alpha as f32 / 255.0),
        );
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
                            (OpticComponentKind::Laser, "🔦 Laser"),
                            (OpticComponentKind::BeamSplitter, "分 Beam Splitter"),
                            (OpticComponentKind::Mirror, "🪞 Mirror"),
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
                        if ui.button("🗑 Clear All").clicked() {
                            self.components.clear();
                            self.lines.clear();
                        }
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
                                        if let Some(Tool::PlaceComponent(kind)) = self.selected_tool
                                        {
                                            dropped_component =
                                                Some(PlacedComponent { kind, pos: s_pos });
                                        }
                                    }

                                    if response.secondary_clicked() {
                                        let click_pos = s_pos + self.scroll_offset;
                                        self.components.retain(|c| c.pos.distance(click_pos) > 5.0);
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
