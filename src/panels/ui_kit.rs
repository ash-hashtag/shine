//! Small shared widgets and the canvas colour theme.

use egui::{Align2, Color32, FontId, Painter, Rect, Response, Sense, Stroke, Ui, Vec2};

/// Canvas colours derived from the egui theme, so light and dark modes both read well.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTheme {
    pub background: Color32,
    pub minor_line: Color32,
    pub major_line: Color32,
    pub text: Color32,
    pub weak_text: Color32,
    pub selection: Color32,
    pub active: Color32,
    pub hover: Color32,
}

impl CanvasTheme {
    pub fn light() -> Self {
        Self {
            background: Color32::from_rgb(250, 250, 248),
            minor_line: Color32::from_rgb(228, 230, 236),
            major_line: Color32::from_rgb(196, 201, 214),
            text: Color32::from_gray(40),
            weak_text: Color32::from_gray(130),
            selection: Color32::from_rgb(232, 140, 0),
            active: Color32::from_rgb(255, 196, 0),
            hover: Color32::from_rgba_unmultiplied(70, 120, 230, 28),
        }
    }

    pub fn dark() -> Self {
        Self {
            background: Color32::from_rgb(30, 31, 34),
            minor_line: Color32::from_rgb(44, 46, 51),
            major_line: Color32::from_rgb(64, 67, 75),
            text: Color32::from_gray(225),
            weak_text: Color32::from_gray(140),
            selection: Color32::from_rgb(240, 150, 30),
            active: Color32::from_rgb(255, 205, 70),
            hover: Color32::from_rgba_unmultiplied(110, 150, 255, 30),
        }
    }

    pub fn from_visuals(visuals: &egui::Visuals) -> Self {
        if visuals.dark_mode {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

/// Keyboard shortcuts are suspended only while the user types in a text
/// field. (Any focused widget counts as keyboard input for egui, which would
/// silently disable shortcuts after clicking a button.)
pub fn shortcuts_blocked(ctx: &egui::Context) -> bool {
    ctx.text_edit_focused()
}

/// Key presses mapped to shortcuts, with egui's Tab focus traversal cancelled
/// when Tab was used as a shortcut. Call before drawing any widget.
pub fn shortcut_keys(ctx: &egui::Context) -> Vec<(egui::Key, egui::Modifiers)> {
    if shortcuts_blocked(ctx) {
        return Vec::new();
    }
    let keys: Vec<_> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    if keys.iter().any(|(k, _)| *k == egui::Key::Tab) {
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
    }
    keys
}

/// Small uppercase section title with breathing room.
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(title.to_uppercase())
            .small()
            .strong()
            .weak(),
    );
    ui.add_space(2.0);
}

/// Full-width library row: an icon drawn by `icon`, then `label`.
/// Senses clicks and drags (drag it onto the canvas).
pub fn item_row(
    ui: &mut Ui,
    selected: bool,
    label: &str,
    icon: impl FnOnce(&Painter, Rect),
) -> Response {
    item_row_sized(ui, selected, label, 26.0, icon)
}

/// [`item_row`] at a chosen height (compact lists use about 22).
pub fn item_row_sized(
    ui: &mut Ui,
    selected: bool,
    label: &str,
    height: f32,
    icon: impl FnOnce(&Painter, Rect),
) -> Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        Sense::click_and_drag(),
    );
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() {
            ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
        }
        let icon_rect =
            Rect::from_center_size(rect.left_center() + Vec2::new(16.0, 0.0), Vec2::splat((height - 6.0).min(20.0)));
        icon(ui.painter(), icon_rect);
        ui.painter().text(
            rect.left_center() + Vec2::new(34.0, 0.0),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(if height < 24.0 { 13.0 } else { 13.5 }),
            visuals.text_color(),
        );
    }
    response
}

/// Rounded badge with initials, used for components in libraries and on canvases.
pub fn badge(painter: &Painter, rect: Rect, initials: &str, fill: Color32, text: Color32) {
    painter.rect(rect, 3.0, fill, Stroke::NONE, egui::StrokeKind::Inside);
    let size = (rect.height() * 0.45).clamp(6.0, 40.0)
        * if initials.chars().count() > 2 {
            0.85
        } else {
            1.0
        };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials,
        FontId::proportional(size),
        text,
    );
}

/// Centered hint drawn on an empty canvas.
pub fn empty_hint(painter: &Painter, canvas: Rect, theme: &CanvasTheme, title: &str, detail: &str) {
    painter.text(
        canvas.center() - Vec2::new(0.0, 10.0),
        Align2::CENTER_CENTER,
        title,
        FontId::proportional(17.0),
        theme.weak_text,
    );
    painter.text(
        canvas.center() + Vec2::new(0.0, 14.0),
        Align2::CENTER_CENTER,
        detail,
        FontId::proportional(13.0),
        theme.weak_text,
    );
}

/// Deterministic accent colour for a name, for component badges.
pub fn accent(name: &str) -> Color32 {
    const PALETTE: [Color32; 8] = [
        Color32::from_rgb(52, 101, 164),
        Color32::from_rgb(78, 154, 6),
        Color32::from_rgb(196, 98, 16),
        Color32::from_rgb(117, 80, 123),
        Color32::from_rgb(0, 128, 128),
        Color32::from_rgb(164, 0, 64),
        Color32::from_rgb(92, 92, 160),
        Color32::from_rgb(128, 102, 0),
    ];
    let hash = name
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    PALETTE[hash as usize % PALETTE.len()]
}

/// Up to three initials from a name: "Mach Zehnder switch" -> "MZS".
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    match words.as_slice() {
        [] => "?".to_owned(),
        [single] => single.chars().take(2).collect::<String>().to_uppercase(),
        many => many
            .iter()
            .take(3)
            .filter_map(|w| w.chars().next())
            .collect::<String>()
            .to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_from_names() {
        assert_eq!(initials("Mach Zehnder switch"), "MZS");
        assert_eq!(initials("mach-zehnder"), "MZ");
        assert_eq!(initials("ring"), "RI");
        assert_eq!(initials("a b c d"), "ABC");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("x"), "X");
    }

    #[test]
    fn accent_is_stable_and_varied() {
        assert_eq!(accent("MZI"), accent("MZI"));
        let colours: std::collections::HashSet<_> = ["a", "b", "c", "d", "e", "f"]
            .iter()
            .map(|n| accent(n))
            .collect();
        assert!(colours.len() > 2);
    }

    #[test]
    fn themes_follow_visuals() {
        assert_eq!(
            CanvasTheme::from_visuals(&egui::Visuals::dark()),
            CanvasTheme::dark()
        );
        assert_eq!(
            CanvasTheme::from_visuals(&egui::Visuals::light()),
            CanvasTheme::light()
        );
        assert_ne!(
            CanvasTheme::dark().background,
            CanvasTheme::light().background
        );
    }

    #[test]
    fn widgets_render() {
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |ui| {
            section(ui, "Parts");
            let r = item_row(ui, true, "Laser", |p, r| {
                badge(p, r, "L", Color32::RED, Color32::WHITE)
            });
            assert!(r.rect.height() > 20.0);
            item_row(ui, false, "Mirror", |_, _| {});
            let rect = ui.max_rect();
            empty_hint(
                ui.painter(),
                rect,
                &CanvasTheme::light(),
                "Empty",
                "Drag parts here",
            );
            badge(
                ui.painter(),
                Rect::from_min_size(rect.min, Vec2::splat(30.0)),
                "MZS",
                Color32::BLUE,
                Color32::WHITE,
            );
        })
        .textures_delta
        .clear();
    }
}
