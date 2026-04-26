use egui::{Color32, Pos2, Stroke};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OpticComponentKind {
    Laser,
    BeamSplitter,
    Mirror,
    Lens,
}

pub struct PlacedComponent {
    pub kind: OpticComponentKind,
    pub pos: Pos2,
}

pub struct OpticsPanel {
    pub components: Vec<PlacedComponent>,
    pub selected_tool: Option<OpticComponentKind>,
    pub dragging_new: Option<OpticComponentKind>,
    pub grid_size: f32,
}

impl Default for OpticsPanel {
    fn default() -> Self {
        Self {
            components: Vec::new(),
            selected_tool: None,
            dragging_new: None,
            grid_size: 40.0,
        }
    }
}

impl OpticsPanel {
    fn draw_component_preview(
        painter: &egui::Painter,
        pos: Pos2,
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

        ui.horizontal(|ui| {
            // Side Menu
            ui.vertical(|ui| {
                ui.set_min_width(150.0);
                ui.set_max_width(150.0); // Prevent separator from taking up full width
                ui.heading("Components");
                ui.label(egui::RichText::new("Drag onto grid").small().weak());
                ui.separator();

                let tools = [
                    (OpticComponentKind::Laser, "🔦 Laser"),
                    (OpticComponentKind::BeamSplitter, "分 Beam Splitter"),
                    (OpticComponentKind::Mirror, "🪞 Mirror"),
                    (OpticComponentKind::Lens, "🔍 Lens"),
                ];

                for (kind, label) in tools {
                    let response = ui.add(egui::Button::new(label).fill(
                        if self.selected_tool == Some(kind) {
                            Color32::from_gray(80)
                        } else {
                            Color32::TRANSPARENT
                        },
                    ));

                    if response.drag_started() {
                        self.dragging_new = Some(kind);
                    }

                    if response.clicked() {
                        self.selected_tool = if self.selected_tool == Some(kind) {
                            None
                        } else {
                            Some(kind)
                        };
                    }
                }

                ui.add_space(20.0);
                if ui.button("🗑 Clear All").clicked() {
                    self.components.clear();
                }

                ui.separator();
                ui.label("Drag & Drop to place");
                ui.label("Right-click: Remove");
            });

            ui.separator();

            // Grid Area - 400x400 encapsulated canvas
            ui.vertical(|ui| {
                ui.add_space(10.0);
                egui::Frame::canvas(ui.style())
                    .fill(Color32::WHITE)
                    .show(ui, |ui| {
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(400.0, 400.0), egui::Sense::all());
                        let painter = ui.painter_at(rect);

                        // Handle interactions and calculate hover state
                        let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                        let mut snapped_pos = None;
                        let mut hovered_cell = None;

                        if let Some(m_pos) = mouse_pos {
                            if rect.contains(m_pos) {
                                let grid_x = ((m_pos.x - rect.left()) / self.grid_size).round();
                                let grid_y = ((m_pos.y - rect.top()) / self.grid_size).round();
                                let s_pos = Pos2::new(
                                    rect.left() + grid_x * self.grid_size,
                                    rect.top() + grid_y * self.grid_size,
                                );
                                snapped_pos = Some(s_pos);

                                let cell_x = ((m_pos.x - rect.left()) / self.grid_size).floor();
                                let cell_y = ((m_pos.y - rect.top()) / self.grid_size).floor();
                                hovered_cell = Some(egui::Rect::from_min_size(
                                    Pos2::new(
                                        rect.left() + cell_x * self.grid_size,
                                        rect.top() + cell_y * self.grid_size,
                                    ),
                                    egui::vec2(self.grid_size, self.grid_size),
                                ));

                                if ui.input(|i| i.pointer.any_released()) {
                                    if let Some(kind) = self.dragging_new {
                                        dropped_component =
                                            Some(PlacedComponent { kind, pos: s_pos });
                                        self.dragging_new = None;
                                    }
                                }

                                if response.clicked() && !response.secondary_clicked() {
                                    if let Some(kind) = self.selected_tool {
                                        dropped_component =
                                            Some(PlacedComponent { kind, pos: s_pos });
                                    }
                                }

                                if response.secondary_clicked() {
                                    self.components.retain(|c| c.pos.distance(s_pos) > 5.0);
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

                        // Draw grid lines (Tileable 400x400)
                        let grid_stroke = Stroke::new(1.0, Color32::from_rgb(0, 0, 139));
                        for i in 0..=(400.0 / self.grid_size) as i32 {
                            let offset = i as f32 * self.grid_size;
                            // Vertical
                            painter.line_segment(
                                [
                                    Pos2::new(rect.left() + offset, rect.top()),
                                    Pos2::new(rect.left() + offset, rect.bottom()),
                                ],
                                grid_stroke,
                            );
                            // Horizontal
                            painter.line_segment(
                                [
                                    Pos2::new(rect.left(), rect.top() + offset),
                                    Pos2::new(rect.right(), rect.top() + offset),
                                ],
                                grid_stroke,
                            );
                        }

                        // Draw placed components (only if they are within this grid)
                        for comp in &self.components {
                            if rect.contains(comp.pos) {
                                Self::draw_component_preview(&painter, comp.pos, comp.kind, 255);
                            }
                        }

                        // Draw drag preview
                        if let Some(kind) = self.dragging_new {
                            if let Some(m_pos) = mouse_pos {
                                let preview_pos = snapped_pos.unwrap_or(m_pos);
                                if rect.contains(preview_pos) || self.dragging_new.is_some() {
                                    Self::draw_component_preview(&painter, preview_pos, kind, 128);
                                }
                            }
                        }
                    });
            });
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
        }

        if ui.input(|i| i.pointer.any_released()) {
            self.dragging_new = None;
        }
    }
}
