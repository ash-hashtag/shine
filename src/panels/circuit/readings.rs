//! A design's readings: what its detectors, output ports, displays and
//! registers show. Shared by the editor's export and `shine simulate`.

use std::fmt::Write as _;

use super::sim::Outcome;
use super::sweep;
use crate::panels::optics::model::{BlockRef, Builtin, InstanceId, Layout};
use crate::panels::optics::{input_number, output_number};
use crate::panels::units::{self, PowerUnit};

/// One reading.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub name: String,
    pub value: f64,
    /// `mW` for light, empty for numbers.
    pub unit: &'static str,
    /// Light leaving a box (rather than a detector or output port).
    pub from_box: bool,
}

impl Reading {
    /// The value as people read it: "976 µW (-0.11 dBm)", or the number.
    pub fn shown(&self, unit: PowerUnit) -> String {
        if self.unit == "mW" { units::format_both(self.value, unit) } else { format!("{}", self.value) }
    }
}

/// Every reading of a simulated design, in the order the inspector lists them.
pub fn readings(layout: &Layout, outcome: &Outcome, name_of_box: &dyn Fn(InstanceId) -> String) -> Vec<Reading> {
    let mut out: Vec<Reading> = sweep::outputs(layout, outcome, name_of_box)
        .into_iter()
        .map(|(name, value, from_box)| Reading { name, value, unit: "mW", from_box })
        .collect();
    for inst in &layout.instances {
        let at = format!("{},{}", inst.cell.0, inst.cell.1);
        let (kind, n) = match &inst.block {
            BlockRef::Builtin(Builtin::Display { bits }) => ("Display", input_number(layout, outcome, inst.id, *bits as usize)),
            BlockRef::Builtin(Builtin::Register { bits }) => ("Register", output_number(outcome, inst.id, *bits as usize)),
            _ => continue,
        };
        out.push(Reading { name: format!("{kind} {at}"), value: n as f64, unit: "", from_box: false });
    }
    out
}

/// Readings as CSV: name, value, unit.
pub fn to_csv(readings: &[Reading]) -> String {
    let mut out = "\"reading\",\"value\",\"unit\"\n".to_owned();
    for r in readings {
        let _ = writeln!(out, "\"{}\",{},\"{}\"", r.name.replace('"', "\"\""), r.value, r.unit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panels::circuit::sim::{Machine, Options, simulate};
    use crate::panels::optics::examples;
    use crate::panels::project::Project;

    #[test]
    fn the_wdm_links_readings() {
        let mut p = Project::new();
        let id = examples::wdm_link(&mut p);
        let layout = p.doc(id).unwrap().layout();
        let o = simulate(layout, 10.0, &p, &Machine::default(), Some(id), &Options::default());
        let names = |id: InstanceId| format!("box {}", id.0);
        let r = readings(layout, &o, &names);
        let display = r.iter().find(|r| r.name.starts_with("Display")).unwrap();
        assert_eq!((display.value, display.unit, display.shown(PowerUnit::Dbm).as_str()), (11.0, "", "11"));
        let detector = r.iter().find(|r| r.name == "Detector 26,5").unwrap();
        assert!(detector.value > 0.9 && detector.shown(PowerUnit::MilliWatt).contains("µW"));
        assert!(r.iter().any(|r| r.from_box), "box outputs are there, marked");
        let csv = to_csv(&r);
        assert!(csv.starts_with("\"reading\",\"value\",\"unit\"\n"));
        assert!(csv.contains("\"Display 26,9\",11,\"\""));
        assert_eq!(csv.lines().count(), r.len() + 1);
    }
}
