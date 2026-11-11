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

/// Whether a focused control uses `key` itself: Tab moves focus on,
/// arrows change a slider, Space and Enter press a button.
fn used_by_focused_widget(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(key, Tab | ArrowLeft | ArrowRight | ArrowUp | ArrowDown | Space | Enter)
}

/// Key presses for shortcuts. While a control other than `canvas` has
/// keyboard focus, the keys it uses itself go to it instead, so the whole
/// interface works from the keyboard. Call before drawing any widget.
pub fn shortcut_keys(ctx: &egui::Context, canvas: Option<egui::Id>) -> Vec<(egui::Key, egui::Modifiers)> {
    if shortcuts_blocked(ctx) {
        return Vec::new();
    }
    let focused = ctx.memory(|m| m.focused());
    let widget_has_keys = focused.is_some() && focused != canvas;
    let keys: Vec<_> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                // Held arrow keys repeat, to keep moving or panning.
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat,
                    modifiers,
                    ..
                } if (!repeat || matches!(key, egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::ArrowUp | egui::Key::ArrowDown))
                    && !(widget_has_keys && used_by_focused_widget(*key)) =>
                {
                    Some((*key, *modifiers))
                }
                _ => None,
            })
            .collect()
    });
    keys
}

/// Cancel egui's Tab focus traversal for this frame, when Tab was used as a
/// shortcut instead.
pub fn keep_focus_on_tab(ctx: &egui::Context) {
    ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
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
    // Painted by hand, so describe it for screen readers (and UI tests).
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label));
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

/// Fonts for every symbol the UI uses, on every platform: egui's bundled
/// fonts (with its emoji fonts) plus its monospace font as a last resort for
/// arrows and shapes the proportional font lacks, such as →.
pub fn install_fonts(ctx: &egui::Context) {
    if let Some(hack) = egui::FontDefinitions::default().font_data.get("Hack") {
        ctx.add_font(egui::epaint::text::FontInsert::new(
            "Hack (symbols)",
            (**hack).clone(),
            vec![egui::epaint::text::InsertFontFamily {
                family: egui::FontFamily::Proportional,
                priority: egui::epaint::text::FontPriority::Lowest,
            }],
        ));
    }
}

/// Height of the menu bar and tab strip at the top of the window.
pub const TOP_BARS_HEIGHT: f32 = 50.0;

/// A floating window that opens in the middle of the screen and stays below
/// the menu bar and tabs, so it never covers the menus.
pub fn window<'open>(ctx: &egui::Context, title: &str) -> egui::Window<'open> {
    let screen = ctx.content_rect();
    let area = Rect::from_min_max(screen.min + Vec2::new(0.0, TOP_BARS_HEIGHT.min(screen.height() / 2.0)), screen.max);
    egui::Window::new(title.to_owned()).pivot(Align2::CENTER_CENTER).default_pos(area.center()).constrain_to(area)
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

/// A slider with its value box, for screen readers named by `label` (the id
/// of the label next to them): `egui::Slider`'s own value box can't be named.
/// The value box is named here; name the returned response (the slider's,
/// reporting a change from either) with `.labelled_by(label)`.
pub fn slider<N: egui::emath::Numeric>(
    ui: &mut Ui,
    label: egui::Id,
    value: &mut N,
    range: std::ops::RangeInclusive<N>,
    suffix: &str,
) -> Response {
    ui.horizontal(|ui| {
        let slider = ui.add(egui::Slider::new(value, range.clone()).show_value(false));
        let span = range.end().to_f64() - range.start().to_f64();
        let decimals = if N::INTEGRAL { 0 } else { 2 };
        let number = ui
            .add(
                egui::DragValue::new(value)
                    .range(range)
                    .speed(if N::INTEGRAL { 0.1 } else { span / 300.0 })
                    .min_decimals(decimals)
                    .max_decimals(decimals.max(4))
                    .suffix(suffix),
            )
            .labelled_by(label);
        slider | number
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character in the source's string literals (UI text, names,
    /// descriptions), outside ASCII.
    fn literal_chars() -> std::collections::BTreeMap<char, String> {
        let mut found = std::collections::BTreeMap::new();
        let mut dirs = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                for (n, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
                    if line.trim_start().starts_with("//") {
                        continue;
                    }
                    let (mut inside, mut escaped) = (false, false);
                    for c in line.chars() {
                        match c {
                            '\\' if inside && !escaped => escaped = true,
                            '"' if !escaped => inside = !inside,
                            c if inside && !c.is_ascii() => {
                                found.entry(c).or_insert_with(|| format!("{}:{}", path.display(), n + 1));
                            }
                            _ => {}
                        }
                        if c != '\\' {
                            escaped = false;
                        }
                    }
                }
            }
        }
        found
    }

    #[test]
    fn the_fonts_draw_every_character_in_the_ui() {
        // egui shows a missing glyph as an empty box on machines without a
        // system font that has it; ours must all be bundled.
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        let chars = literal_chars();
        assert!(chars.contains_key(&'µ'), "the scan finds literals");
        let missing: Vec<String> = ctx.fonts_mut(|f| {
            chars
                .iter()
                .filter(|(c, _)| !f.has_glyph(&FontId::proportional(14.0), **c) || !f.has_glyph(&FontId::monospace(14.0), **c))
                .map(|(c, at)| format!("{c} (U+{:04X}) at {at}", *c as u32))
                .collect()
        });
        ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
        assert!(missing.is_empty(), "no glyph for:\n{}", missing.join("\n"));
    }

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
