//! User preferences: how things look and behave, saved between sessions
//! (a settings file on desktop, browser storage on the web).

use egui::Color32;
use serde::{Deserialize, Serialize};

pub use super::units::PowerUnit;

/// Key the preferences are stored under.
pub const STORAGE_KEY: &str = "shine.preferences";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// How light is coloured on the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightColors {
    /// Visible wavelengths in their real colour; others (infrared,
    /// ultraviolet) each get a distinct false colour.
    #[default]
    Spectral,
    /// Every wavelength gets a distinct colour, ordered by wavelength.
    Distinct,
    /// All light in one colour.
    Single,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub theme: Theme,
    pub light_colors: LightColors,
    /// Draw a laser's light in the colour chosen for it, when it has one.
    pub laser_colors: bool,
    /// Colour for [`LightColors::Single`].
    pub single_color: [u8; 3],
    /// Draw each wavelength on a waveguide or beam as its own strand.
    pub channel_strands: bool,
    pub beam_arrows: bool,
    pub show_readings: bool,
    /// Unit for powers on the canvas and in the inspector.
    pub power_unit: PowerUnit,
    pub show_port_names: bool,
    pub show_grid: bool,
    /// Frames per second, frame and simulation time in the status bar.
    pub show_frame_time: bool,
    /// The performance graph window.
    pub show_performance_graph: bool,
    /// Wavelength of newly placed lasers.
    pub default_wavelength_nm: f32,
    /// Seconds between clock ticks while running.
    pub tick_seconds: f64,
    /// Live simulation on for new documents.
    pub live_simulation: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            light_colors: LightColors::Spectral,
            laser_colors: true,
            single_color: [220, 60, 60],
            channel_strands: true,
            beam_arrows: true,
            show_readings: true,
            power_unit: PowerUnit::MilliWatt,
            show_port_names: true,
            show_grid: true,
            show_frame_time: false,
            show_performance_graph: false,
            default_wavelength_nm: 650.0,
            tick_seconds: 0.5,
            live_simulation: true,
        }
    }
}

/// Distinct colours for wavelengths without a real colour, in order.
pub const FALSE_COLORS: [Color32; 10] = [
    Color32::from_rgb(0, 166, 166),
    Color32::from_rgb(242, 142, 43),
    Color32::from_rgb(156, 106, 222),
    Color32::from_rgb(89, 161, 79),
    Color32::from_rgb(227, 119, 194),
    Color32::from_rgb(78, 121, 167),
    Color32::from_rgb(188, 189, 34),
    Color32::from_rgb(156, 117, 95),
    Color32::from_rgb(23, 190, 207),
    Color32::from_rgb(225, 87, 89),
];

/// Visible range in nm.
pub fn is_visible(nm: f64) -> bool {
    (380.0..=780.0).contains(&nm)
}

/// Colours for the light in one simulation: assigns false colours by rank
/// among the wavelengths present, so neighbouring channels look different.
#[derive(Clone, Debug)]
pub struct Palette {
    mode: LightColors,
    laser_colors: bool,
    single: Color32,
    /// Wavelengths that get false colours, sorted.
    ranked: Vec<f64>,
}

impl Palette {
    pub fn new(prefs: &Preferences, wavelengths: impl IntoIterator<Item = f64>) -> Self {
        let mut ranked: Vec<f64> = wavelengths
            .into_iter()
            .filter(|&w| prefs.light_colors == LightColors::Distinct || !is_visible(w))
            .collect();
        ranked.sort_by(f64::total_cmp);
        ranked.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        Self {
            mode: prefs.light_colors,
            laser_colors: prefs.laser_colors,
            single: Color32::from_rgb(prefs.single_color[0], prefs.single_color[1], prefs.single_color[2]),
            ranked,
        }
    }

    /// Colour of light at `nm`, from a source with an optional chosen colour.
    pub fn color(&self, nm: f64, chosen: Option<[u8; 3]>) -> Color32 {
        if let Some([r, g, b]) = chosen.filter(|_| self.laser_colors) {
            return Color32::from_rgb(r, g, b);
        }
        match self.mode {
            LightColors::Single => self.single,
            LightColors::Spectral if is_visible(nm) => crate::panels::optics::render::wavelength_color(nm),
            LightColors::Spectral | LightColors::Distinct => {
                let rank = self.ranked.iter().position(|&w| (w - nm).abs() < 1e-6).unwrap_or(0);
                FALSE_COLORS[rank % FALSE_COLORS.len()]
            }
        }
    }
}

/// Wavelength presets offered for lasers: (nm, what it is).
pub const WAVELENGTH_PRESETS: &[(f32, &str)] = &[
    (405.0, "violet (Blu-ray)"),
    (450.0, "blue"),
    (532.0, "green (DPSS)"),
    (633.0, "red (HeNe)"),
    (650.0, "red (diode)"),
    (780.0, "near-IR (CD)"),
    (850.0, "near-IR (multimode links)"),
    (1310.0, "O-band (telecom)"),
    (1550.0, "C-band (telecom)"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spectral_colours_real_and_false() {
        let prefs = Preferences::default();
        let p = Palette::new(&prefs, [532.0, 1530.0, 1540.0, 1530.0]);
        assert_eq!(p.color(532.0, None), crate::panels::optics::render::wavelength_color(532.0));
        assert_eq!(p.color(1530.0, None), FALSE_COLORS[0]);
        assert_eq!(p.color(1540.0, None), FALSE_COLORS[1], "neighbouring IR channels differ");
        assert_eq!(p.color(1530.0, Some([1, 2, 3])), Color32::from_rgb(1, 2, 3), "a laser's own colour wins");
        let p = Palette::new(&Preferences { laser_colors: false, ..prefs.clone() }, [1530.0]);
        assert_eq!(p.color(1530.0, Some([1, 2, 3])), FALSE_COLORS[0]);
    }

    #[test]
    fn distinct_and_single_modes() {
        let prefs = Preferences { light_colors: LightColors::Distinct, ..Preferences::default() };
        let p = Palette::new(&prefs, [650.0, 532.0]);
        assert_eq!((p.color(532.0, None), p.color(650.0, None)), (FALSE_COLORS[0], FALSE_COLORS[1]));
        let prefs = Preferences { light_colors: LightColors::Single, single_color: [9, 9, 9], ..Preferences::default() };
        assert_eq!(Palette::new(&prefs, [1.0]).color(1550.0, None), Color32::from_rgb(9, 9, 9));
        assert!(is_visible(550.0) && !is_visible(1550.0));
    }

    #[test]
    fn preferences_survive_missing_fields() {
        // Settings saved by an older version lack newer fields: they default.
        let p: Preferences = serde_json::from_str(r#"{"show_grid": false}"#).unwrap();
        assert!(!p.show_grid && p.show_readings);
    }
}
