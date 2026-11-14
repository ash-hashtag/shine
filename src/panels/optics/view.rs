//! 2D view transform: pan and zoom between world and screen space.

use egui::{Event, MouseWheelSource, MouseWheelUnit, Pos2, Rect, Response, Ui, Vec2};

use super::model::GRID;

/// Far enough out to see a design of millions of parts whole (drawn as a
/// summary below 10%, see the canvas).
pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Screen offset of the world origin from the canvas' top-left corner.
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            pan: Vec2::splat(GRID),
            zoom: 1.0,
        }
    }
}

impl View {
    pub fn to_screen(&self, canvas: Rect, world: Pos2) -> Pos2 {
        canvas.min + self.pan + world.to_vec2() * self.zoom
    }

    pub fn to_world(&self, canvas: Rect, screen: Pos2) -> Pos2 {
        ((screen - canvas.min - self.pan) / self.zoom).to_pos2()
    }

    /// World area visible in the canvas.
    pub fn visible(&self, canvas: Rect) -> Rect {
        Rect::from_min_max(
            self.to_world(canvas, canvas.min),
            self.to_world(canvas, canvas.max),
        )
    }

    pub fn pan_by(&mut self, screen_delta: Vec2) {
        self.pan += screen_delta;
    }

    /// Multiply the zoom by `factor`, keeping the world point under `anchor` fixed.
    pub fn zoom_at(&mut self, canvas: Rect, anchor: Pos2, factor: f32) {
        let world = self.to_world(canvas, anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = anchor - canvas.min - world.to_vec2() * self.zoom;
    }

    /// Fit `bounds` into the canvas with `margin` screen points around it.
    pub fn frame(&mut self, canvas: Rect, bounds: Rect, margin: f32) {
        // Do not zoom in further than a few cells fill the view.
        let bounds =
            Rect::from_center_size(bounds.center(), bounds.size().max(Vec2::splat(GRID * 5.0)));
        let available = (canvas.size() - Vec2::splat(2.0 * margin)).max(Vec2::splat(1.0));
        self.zoom = (available.x / bounds.width())
            .min(available.y / bounds.height())
            .clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = canvas.size() / 2.0 - bounds.center().to_vec2() * self.zoom;
    }
}

/// Blender-style navigation: wheel or pinch zooms at the cursor, trackpad
/// scrolling and middle-drag pan, Ctrl/Shift+wheel pan along one axis,
/// Ctrl+middle-drag zooms.
pub fn navigate(view: &mut View, ui: &Ui, rect: Rect, response: &Response, pointer: Option<Pos2>) {
    if let Some(anchor) = pointer {
        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::MouseWheel {
                    unit,
                    delta,
                    source,
                    modifiers,
                    ..
                } => {
                    let points = match unit {
                        MouseWheelUnit::Point => delta,
                        MouseWheelUnit::Line => delta * 40.0,
                        MouseWheelUnit::Page => delta * rect.height(),
                    };
                    let trackpad = source == MouseWheelSource::Trackpad
                        || (unit == MouseWheelUnit::Point && source != MouseWheelSource::Wheel);
                    if modifiers.command {
                        view.pan_by(Vec2::new(points.y + points.x, 0.0));
                    } else if modifiers.shift {
                        view.pan_by(Vec2::new(0.0, points.y + points.x));
                    } else if trackpad {
                        view.pan_by(points);
                    } else {
                        view.zoom_at(rect, anchor, (points.y * 0.0025).exp());
                    }
                }
                Event::Zoom(factor) => view.zoom_at(rect, anchor, factor),
                _ => {}
            }
        }
    }
    if response.dragged_by(egui::PointerButton::Middle) {
        let delta = response.drag_delta();
        if ui.input(|i| i.modifiers.command) {
            let anchor = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(rect.center());
            view.zoom_at(rect, anchor, (-delta.y * 0.01).exp());
        } else {
            view.pan_by(delta);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 50.0), Vec2::new(800.0, 600.0))
    }

    fn close(a: Pos2, b: Pos2) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn round_trips_between_spaces() {
        let view = View {
            pan: Vec2::new(13.0, -7.0),
            zoom: 2.5,
        };
        for p in [Pos2::ZERO, Pos2::new(123.0, -45.0)] {
            assert!(close(
                view.to_world(canvas(), view.to_screen(canvas(), p)),
                p
            ));
        }
        assert_eq!(view.to_screen(canvas(), Pos2::ZERO), Pos2::new(113.0, 43.0));
    }

    #[test]
    fn zoom_keeps_anchor_fixed_and_clamps() {
        let mut view = View::default();
        let anchor = Pos2::new(400.0, 300.0);
        let before = view.to_world(canvas(), anchor);
        view.zoom_at(canvas(), anchor, 1.7);
        assert!((view.zoom - 1.7).abs() < 1e-6);
        assert!(close(view.to_world(canvas(), anchor), before));
        view.zoom_at(canvas(), anchor, 1e6);
        assert_eq!(view.zoom, MAX_ZOOM);
        assert!(close(view.to_world(canvas(), anchor), before));
        view.zoom_at(canvas(), anchor, 1e-9);
        assert_eq!(view.zoom, MIN_ZOOM);
    }

    #[test]
    fn pan_moves_content() {
        let mut view = View::default();
        let p = view.to_screen(canvas(), Pos2::ZERO);
        view.pan_by(Vec2::new(10.0, -5.0));
        assert_eq!(
            view.to_screen(canvas(), Pos2::ZERO),
            p + Vec2::new(10.0, -5.0)
        );
    }

    #[test]
    fn frame_centres_and_fits() {
        let mut view = View::default();
        let bounds = Rect::from_min_max(Pos2::new(-400.0, 0.0), Pos2::new(400.0, 100.0));
        view.frame(canvas(), bounds, 20.0);
        assert!(close(
            view.to_screen(canvas(), bounds.center()),
            canvas().center()
        ));
        // Width-limited: 760 px available for 800 world units.
        assert!((view.zoom - 0.95).abs() < 1e-5);
        let visible = view.visible(canvas());
        assert!(visible.contains_rect(bounds));
    }

    #[test]
    fn frame_of_a_single_cell_does_not_zoom_absurdly() {
        let mut view = View::default();
        view.frame(
            canvas(),
            Rect::from_center_size(Pos2::ZERO, Vec2::splat(GRID)),
            20.0,
        );
        assert!(view.zoom <= 560.0 / (GRID * 5.0) + 1e-5);
        assert!(close(
            view.to_screen(canvas(), Pos2::ZERO),
            canvas().center()
        ));
    }
}
