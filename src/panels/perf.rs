//! Frame rate and frame timings, for the status bar and the performance
//! graph: they show whether a stutter is real and where the time goes.

use std::collections::VecDeque;

/// Frames kept for the graph.
const HISTORY: usize = 600;
/// Seconds of history the graph shows.
const GRAPH_SECONDS: f64 = 10.0;
/// A pause longer than this means the app was idle (it only draws when
/// something changes), not that a frame was slow.
const IDLE_GAP_S: f64 = 0.25;

/// One drawn frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// When it started (seconds, the app's clock).
    pub at: f64,
    /// Time since the frame before, in ms (`None` after an idle pause).
    pub interval_ms: Option<f64>,
    /// CPU time it took, drawing included where known.
    pub cpu_ms: f64,
    /// Of which simulating.
    pub simulate_ms: f64,
}

/// Frame timings over recent frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameStats {
    /// CPU time of the last frame, drawing included (from eframe; `None` in tests).
    pub frame_cpu_ms: Option<f64>,
    /// Time in the editor's own code per frame (smoothed).
    pub ui_ms: f64,
    /// Time simulating per frame (smoothed).
    pub simulate_ms: f64,
    /// Simulation time so far this frame.
    pub(crate) simulating: f64,
    /// How long the last simulation took, and when it ran (seconds).
    pub last_simulation_ms: f64,
    pub(crate) last_simulation_at: f64,
    /// When a slow frame was last logged.
    logged_at: f64,
    /// Recent frames, oldest first.
    pub frames: VecDeque<Frame>,
}

impl FrameStats {
    /// Fold one frame's timings in; `now` is when it started.
    pub fn record(&mut self, ui_ms: f64, now: f64) {
        let smooth = |old: f64, new: f64| if old == 0.0 { new } else { old * 0.8 + new * 0.2 };
        self.ui_ms = smooth(self.ui_ms, ui_ms);
        self.simulate_ms = smooth(self.simulate_ms, self.simulating);
        let cpu_ms = self.frame_cpu_ms.unwrap_or(ui_ms).max(ui_ms);
        if cpu_ms > 50.0 && now - self.logged_at > 1.0 {
            self.logged_at = now;
            tracing::info!("Slow frame: {cpu_ms:.1} ms, {:.1} ms of it simulating", self.simulating);
        }
        let interval_ms = self
            .frames
            .back()
            .map(|f| now - f.at)
            .filter(|&gap| gap > 0.0 && gap < IDLE_GAP_S)
            .map(|gap| gap * 1e3);
        if self.frames.len() == HISTORY {
            self.frames.pop_front();
        }
        self.frames.push_back(Frame { at: now, interval_ms, cpu_ms, simulate_ms: self.simulating });
        self.simulating = 0.0;
    }

    /// Frames per second over the last half second of drawing, or `None`
    /// when the app has been idle (it draws only when something changes).
    pub fn fps(&self) -> Option<f64> {
        let last = self.frames.back()?;
        let recent: Vec<f64> = self
            .frames
            .iter()
            .rev()
            .take_while(|f| last.at - f.at < 0.5)
            .filter_map(|f| f.interval_ms)
            .collect();
        if recent.is_empty() {
            return None;
        }
        Some(1000.0 * recent.len() as f64 / recent.iter().sum::<f64>())
    }

    /// The slowest gap between frames in the last second, in ms.
    pub fn worst_interval_ms(&self) -> Option<f64> {
        let last = self.frames.back()?;
        self.frames.iter().rev().take_while(|f| last.at - f.at < 1.0).filter_map(|f| f.interval_ms).reduce(f64::max)
    }

    /// "60 fps · frame 2.1 ms · simulation 0.4 ms".
    pub fn summary(&self) -> String {
        let frame = self.frame_cpu_ms.unwrap_or(self.ui_ms);
        let fps = self.fps().map_or_else(|| "idle".to_owned(), |f| format!("{f:.0} fps"));
        format!("{fps} · frame {frame:.1} ms · simulation {:.1} ms", self.simulate_ms)
    }

    /// The graph: time between frames (what you see as smoothness) and CPU
    /// time per frame, over the last few seconds, against 60 and 30 fps.
    pub fn graph_ui(&self, ui: &mut egui::Ui) {
        use egui_plot::{HLine, Legend, Line, Plot, PlotPoints};
        let now = self.frames.back().map_or(0.0, |f| f.at);
        // The last few seconds only, so an old spike doesn't set the scale.
        let points = |value: fn(&Frame) -> Option<f64>| -> PlotPoints<'static> {
            self.frames
                .iter()
                .filter(|f| now - f.at <= GRAPH_SECONDS)
                .filter_map(|f| Some([f.at - now, value(f)?]))
                .collect::<Vec<_>>()
                .into()
        };
        ui.horizontal(|ui| {
            ui.monospace(self.summary());
            if let Some(worst) = self.worst_interval_ms() {
                ui.label(egui::RichText::new(format!("worst gap {worst:.0} ms")).small().weak())
                    .on_hover_text("The longest wait between two frames in the last second");
            }
        });
        Plot::new("performance_graph")
            .height(170.0)
            .legend(Legend::default().position(egui_plot::Corner::LeftTop))
            .include_y(0.0)
            .include_y(40.0)
            .include_x(-GRAPH_SECONDS)
            .include_x(0.0)
            .allow_drag(false)
            .allow_zoom(false)
            .allow_scroll(false)
            .x_axis_label("seconds ago")
            .y_axis_label("ms")
            .show(ui, |p| {
                p.hline(HLine::new("60 fps", 1000.0 / 60.0).style(egui_plot::LineStyle::dashed_loose()));
                p.hline(HLine::new("30 fps", 1000.0 / 30.0).style(egui_plot::LineStyle::dashed_loose()));
                p.line(Line::new("between frames", points(|f| f.interval_ms)));
                p.line(Line::new("CPU per frame", points(|f| Some(f.cpu_ms))));
                p.line(Line::new("simulation", points(|f| Some(f.simulate_ms))));
            });
        ui.label(
            egui::RichText::new(
                "Shine draws only when something changes, so it's idle (no frames) when nothing moves. While you drag, \
                 the time between frames should stay near the 60 fps line; spikes above it are dropped frames.",
            )
            .small()
            .weak(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_from_frame_times_and_idle_pauses() {
        let mut s = FrameStats::default();
        assert_eq!(s.fps(), None);
        for k in 0..60 {
            s.record(1.0, k as f64 / 60.0);
        }
        assert!((s.fps().unwrap() - 60.0).abs() < 0.5, "{:?}", s.fps());
        assert!(s.summary().starts_with("60 fps"), "{}", s.summary());
        // A stutter shows as the worst gap.
        s.record(1.0, 1.0 + 0.1);
        assert!((s.worst_interval_ms().unwrap() - (0.1 + 1.0 / 60.0) * 1e3).abs() < 1.0);
        // After a long pause the app was idle, not slow.
        s.record(1.0, 5.0);
        assert_eq!(s.frames.back().unwrap().interval_ms, None);
        assert_eq!(s.fps(), None);
        assert!(s.summary().starts_with("idle"));
    }

    #[test]
    fn history_is_bounded_and_records_simulation() {
        let mut s = FrameStats::default();
        for k in 0..(HISTORY + 50) {
            s.simulating = 2.0;
            s.record(3.0, k as f64 * 0.01);
        }
        assert_eq!(s.frames.len(), HISTORY);
        let f = s.frames.back().unwrap();
        assert_eq!((f.cpu_ms, f.simulate_ms), (3.0, 2.0));
        assert_eq!(s.simulating, 0.0, "reset each frame");
    }

    #[test]
    fn graph_draws() {
        let mut s = FrameStats::default();
        for k in 0..30 {
            s.record(1.0, k as f64 / 60.0);
        }
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(Default::default(), |ui| s.graph_ui(ui));
        out.textures_delta.clear();
    }
}
