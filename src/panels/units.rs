//! Real units. Optical power is in milliwatts: a field of amplitude `a`
//! carries `a²` mW, so a laser of amplitude 1 emits 1 mW (0 dBm). Ratios
//! (losses, transmissions, extinction) are in dB.

use serde::{Deserialize, Serialize};

/// How powers are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerUnit {
    #[default]
    MilliWatt,
    Dbm,
}

/// Lowest level shown in dB: anything weaker reads as this.
pub const DB_FLOOR: f64 = -100.0;

/// A power ratio in dB (or a power in mW as dBm), floored at [`DB_FLOOR`].
pub fn db(ratio: f64) -> f64 {
    (10.0 * ratio.max(1e-10).log10()).max(DB_FLOOR)
}

/// A level in dB (or dBm) back to a ratio (or mW).
pub fn from_db(db: f64) -> f64 {
    10f64.powf(db / 10.0)
}

/// Three significant figures, without needless exponent notation.
fn sig3(v: f64) -> String {
    match v.abs() {
        a if a >= 100.0 => format!("{v:.0}"),
        a if a >= 10.0 => format!("{v:.1}"),
        _ => format!("{v:.2}"),
    }
}

/// `v` rounded to three significant figures.
fn round3(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    let scale = 10f64.powi(2 - v.abs().log10().floor() as i32);
    (v * scale).round() / scale
}

/// A power with three significant figures: "1.20 W", "1.00 mW", "0.250 mW",
/// "0.00123 mW", "500 nW". Below a milliwatt it stays in mW down to a
/// microwatt (µ beside W is easily read as m). Rounding comes first, so
/// 0.99999 mW reads "1.00 mW".
pub fn format_mw(mw: f64) -> String {
    let mw = round3(mw);
    match mw.abs() {
        a if a < 1e-9 => "0 mW".into(),
        a if a >= 1e3 => format!("{} W", sig3(mw / 1e3)),
        a if a >= 1.0 => format!("{} mW", sig3(mw)),
        a if a >= 1e-3 => {
            let decimals = (2 - a.log10().floor() as i32).max(0) as usize;
            format!("{mw:.decimals$} mW")
        }
        _ => format!("{} nW", sig3(mw * 1e6)),
    }
}

/// A power in dBm: "-3.01 dBm", or "-∞ dBm" for no light.
pub fn format_dbm(mw: f64) -> String {
    if mw <= 1e-10 { "-∞ dBm".into() } else { format!("{:.2} dBm", db(mw)) }
}

pub fn format_power(mw: f64, unit: PowerUnit) -> String {
    match unit {
        PowerUnit::MilliWatt => format_mw(mw),
        PowerUnit::Dbm => format_dbm(mw),
    }
}

/// Both units, the chosen one first: "0.500 mW (-3.01 dBm)".
pub fn format_both(mw: f64, unit: PowerUnit) -> String {
    match unit {
        PowerUnit::MilliWatt => format!("{} ({})", format_mw(mw), format_dbm(mw)),
        PowerUnit::Dbm => format!("{} ({})", format_dbm(mw), format_mw(mw)),
    }
}

/// Loss in dB of a power transmission (0.5 → 3.01 dB).
pub fn loss_db(transmission: f64) -> f64 {
    -db(transmission)
}

/// Edits a laser's field amplitude as its power, in the chosen unit, with
/// the other unit beside it. Returns whether it changed.
#[cfg(feature = "gui")]
pub fn power_editor(ui: &mut egui::Ui, amplitude: &mut f64, unit: PowerUnit) -> egui::Response {
    let mw = *amplitude * *amplitude;
    ui.horizontal(|ui| {
        let r = match unit {
            PowerUnit::MilliWatt => {
                let mut v = mw;
                let r = ui.add(egui::DragValue::new(&mut v).range(0.0..=1000.0).speed(0.01).max_decimals(4).suffix(" mW"));
                if r.changed() {
                    *amplitude = v.max(0.0).sqrt();
                }
                r
            }
            PowerUnit::Dbm => {
                let mut v = db(mw).max(-60.0);
                let r = ui.add(egui::DragValue::new(&mut v).range(-60.0..=30.0).speed(0.05).max_decimals(2).suffix(" dBm"));
                if r.changed() {
                    *amplitude = from_db(v).sqrt();
                }
                r
            }
        };
        let other = match unit {
            PowerUnit::MilliWatt => format_dbm(mw),
            PowerUnit::Dbm => format_mw(mw),
        };
        ui.label(egui::RichText::new(other).small().weak());
        r
    })
    .inner
    .on_hover_text("Optical power. A field of amplitude a carries a² mW: amplitude 1 is 1 mW, 0 dBm.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels() {
        assert_eq!(db(1.0), 0.0);
        assert_eq!(db(0.0), DB_FLOOR);
        assert!((db(0.5) + 3.0103).abs() < 1e-4);
        assert!((loss_db(0.5) - 3.0103).abs() < 1e-4);
        assert!((from_db(-3.0103) - 0.5).abs() < 1e-5);
        assert!((from_db(db(0.123)) - 0.123).abs() < 1e-12);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_mw(1.0), "1.00 mW");
        assert_eq!(format_mw(0.25), "0.250 mW");
        assert_eq!(format_mw(1234.0), "1.23 W");
        assert_eq!(format_mw(0.0123), "0.0123 mW");
        assert_eq!(format_mw(0.501_187), "0.501 mW", "3 dB off 1 mW");
        assert_eq!(format_mw(0.000_5), "500 nW");
        assert_eq!(format_mw(2e-6), "2.00 nW");
        assert_eq!(format_mw(0.0), "0 mW");
        // Rounding decides the unit: no "1000 nW" or "1000 mW".
        assert_eq!(format_mw(0.999_999), "1.00 mW");
        assert_eq!(format_mw(999.9), "1.00 W");
        assert_eq!(format_mw(0.000_999_96), "0.00100 mW");
        assert_eq!(format_mw(0.5), "0.500 mW");
        assert_eq!(format_dbm(1.0), "0.00 dBm");
        assert_eq!(format_dbm(0.5), "-3.01 dBm");
        assert_eq!(format_dbm(0.0), "-∞ dBm");
        assert_eq!(format_power(10.0, PowerUnit::Dbm), "10.00 dBm");
        assert_eq!(format_both(0.5, PowerUnit::MilliWatt), "0.500 mW (-3.01 dBm)");
        assert_eq!(format_both(0.5, PowerUnit::Dbm), "-3.01 dBm (0.500 mW)");
    }

    #[cfg(feature = "gui")]
    #[test]
    fn power_editor_keeps_amplitude() {
        // Drawing the editor without input leaves the amplitude alone.
        let ctx = egui::Context::default();
        let mut a = 0.7;
        let mut out = ctx.run_ui(Default::default(), |ui| {
            for unit in [PowerUnit::MilliWatt, PowerUnit::Dbm] {
                assert!(!power_editor(ui, &mut a, unit).changed());
            }
        });
        out.textures_delta.clear();
        assert_eq!(a, 0.7);
    }
}
