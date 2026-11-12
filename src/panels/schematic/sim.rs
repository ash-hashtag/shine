//! Compiling a schematic (and the components it uses) into one circuit.

use std::collections::HashMap;

use super::geometry;
use super::model::{BlockRef, Builtin, InstanceId, Schematic, WireModel};
use crate::panels::project::DocId;
use crate::photonics::{C64, Circuit, OutPort, Ports, Waveguide};

/// Builds documents that a schematic uses as components.
pub trait Resolver {
    /// Add component `id` to `circuit`. `stack` holds the components being
    /// built around this one, to detect a component that contains itself.
    fn build_component(
        &self,
        id: DocId,
        circuit: &mut Circuit,
        stack: &mut Vec<DocId>,
    ) -> Result<Ports, String>;
}

/// A schematic added to a circuit.
pub struct BuiltSchematic {
    /// Boundary ports, from the pins in [`Schematic::pin_order`].
    pub ports: Ports,
    pub instance_ports: HashMap<InstanceId, Ports>,
    /// For each wire, the output port it starts at.
    pub wire_sources: Vec<OutPort>,
}

fn port_name(index: usize) -> String {
    format!("#{}", index + 1)
}

pub fn build_schematic(
    schematic: &Schematic,
    resolver: &dyn Resolver,
    circuit: &mut Circuit,
    stack: &mut Vec<DocId>,
) -> Result<BuiltSchematic, String> {
    let mut instance_ports = HashMap::new();
    for inst in &schematic.instances {
        let ports = match &inst.block {
            BlockRef::Builtin(b) => b.build(circuit).map_err(|e| e.to_string())?,
            BlockRef::Component(id) => resolver.build_component(*id, circuit, stack)?,
        };
        instance_ports.insert(inst.id, ports);
    }

    let shapes: HashMap<InstanceId, geometry::Shape> = schematic
        .instances
        .iter()
        .map(|inst| {
            let p = &instance_ports[&inst.id];
            (
                inst.id,
                geometry::shape(inst.pos, &inst.block, p.inputs.len(), p.outputs.len()),
            )
        })
        .collect();

    let settings = schematic.wire_settings;
    let mut wire_sources = Vec::with_capacity(schematic.wires.len());
    for (k, wire) in schematic.wires.iter().enumerate() {
        let missing = |what: &str, port: usize| {
            format!(
                "Wire {} {what} port {} no longer exists",
                k + 1,
                port_name(port)
            )
        };
        let from = instance_ports
            .get(&wire.from.instance)
            .and_then(|p| p.outputs.get(wire.from.port))
            .copied()
            .ok_or_else(|| missing("starts at", wire.from.port))?;
        let to = instance_ports
            .get(&wire.to.instance)
            .and_then(|p| p.inputs.get(wire.to.port))
            .copied()
            .ok_or_else(|| missing("ends at", wire.to.port))?;

        let drawn = || {
            let a = shapes[&wire.from.instance]
                .outputs
                .get(wire.from.port)
                .copied()?;
            // Pins hide their outer port, so map to the visible one.
            let b = shapes[&wire.to.instance]
                .inputs
                .get(wire.to.port)
                .copied()?;
            Some(geometry::wire_length(a, b) as f64)
        };
        let length = wire.length_um.or(match settings.model {
            WireModel::Ideal => None,
            WireModel::Geometric { um_per_unit } => drawn().map(|l| l * um_per_unit),
        });
        match length {
            Some(length) if length > 0.0 => {
                let wg = circuit
                    .add(Waveguide::new(length, settings.n_eff).with_loss(settings.loss_db_per_cm));
                circuit
                    .connect(from, wg.input(0))
                    .map_err(|e| e.to_string())?;
                circuit
                    .connect(wg.output(0), to)
                    .map_err(|e| e.to_string())?;
            }
            _ => circuit.connect(from, to).map_err(|e| e.to_string())?,
        }
        wire_sources.push(from);
    }

    let (inputs, outputs) = schematic.pin_order();
    let pin = |i: usize| &instance_ports[&schematic.instances[i].id];
    Ok(BuiltSchematic {
        ports: Ports {
            inputs: inputs.iter().map(|&i| pin(i).inputs[0]).collect(),
            outputs: outputs.iter().map(|&i| pin(i).outputs[0]).collect(),
        },
        instance_ports,
        wire_sources,
    })
}

/// Steady state of a schematic with its sources switched on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SchematicSim {
    pub wavelengths: Vec<f64>,
    /// Per wire, the field at its start for each wavelength.
    pub wire_fields: Vec<Vec<C64>>,
    /// Power leaving each output port, summed over wavelengths.
    pub port_power: HashMap<(InstanceId, usize), f64>,
    /// Detectors and output pins.
    pub readings: Vec<(InstanceId, f64)>,
    pub nodes: usize,
    pub error: Option<String>,
}

impl SchematicSim {
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Default::default()
        }
    }

    pub fn wire_power(&self, wire: usize) -> f64 {
        self.wire_fields.get(wire).map_or(0.0, |f| {
            f.iter().map(|v| v.norm_sqr()).fold(0.0, |a, b| a + b)
        })
    }

    pub fn reading(&self, instance: InstanceId) -> Option<f64> {
        self.readings.iter().find(|r| r.0 == instance).map(|r| r.1)
    }
}

pub fn simulate(
    schematic: &Schematic,
    resolver: &dyn Resolver,
    stack: &mut Vec<DocId>,
) -> SchematicSim {
    let mut circuit = Circuit::new();
    let built = match build_schematic(schematic, resolver, &mut circuit, stack) {
        Ok(b) => b,
        Err(e) => return SchematicSim::failed(e),
    };
    let solutions = match circuit.simulate_sources() {
        Ok(s) => s,
        Err(e) => return SchematicSim::failed(e.to_string()),
    };

    let mut sim = SchematicSim {
        wavelengths: solutions.iter().map(|s| s.wavelength_nm()).collect(),
        wire_fields: built
            .wire_sources
            .iter()
            .map(|&from| solutions.iter().map(|s| s.output_field(from)).collect())
            .collect(),
        nodes: circuit.len(),
        ..Default::default()
    };
    for inst in &schematic.instances {
        let ports = &built.instance_ports[&inst.id];
        for (k, &out) in ports.outputs.iter().enumerate() {
            let power = solutions
                .iter()
                .map(|s| s.output_field(out).norm_sqr())
                .fold(0.0, |a, b| a + b);
            sim.port_power.insert((inst.id, k), power);
        }
        let reading = match &inst.block {
            BlockRef::Builtin(Builtin::Detector) => Some(
                solutions
                    .iter()
                    .map(|s| s.detected_power(ports.inputs[0].node))
                    .fold(0.0, |a, b| a + b),
            ),
            BlockRef::Builtin(Builtin::OutputPin { .. }) => {
                sim.port_power.get(&(inst.id, 0)).copied()
            }
            _ => None,
        };
        if let Some(r) = reading {
            sim.readings.push((inst.id, r));
        }
    }
    sim
}

#[cfg(test)]
mod tests {
    use emath::Pos2;

    use super::*;
    use crate::panels::schematic::model::PortRef;

    /// Resolves nothing: schematics in these tests use only built-in blocks.
    struct NoComponents;

    impl Resolver for NoComponents {
        fn build_component(
            &self,
            id: DocId,
            _: &mut Circuit,
            _: &mut Vec<DocId>,
        ) -> Result<Ports, String> {
            Err(format!("unknown component {}", id.0))
        }
    }

    fn port(instance: InstanceId, port: usize) -> PortRef {
        PortRef { instance, port }
    }

    fn b(block: Builtin) -> BlockRef {
        BlockRef::Builtin(block)
    }

    /// Laser -> coupler -> (phase shifter | straight) -> coupler -> two detectors.
    fn mzi(phase: f64) -> (Schematic, InstanceId, InstanceId) {
        let mut s = Schematic::default();
        let laser = s.add(
            b(Builtin::Laser {
                amplitude: 1.0,
                phase: 0.0,
                wavelength_nm: 1550.0,
            }),
            Pos2::ZERO,
        );
        let c1 = s.add(b(Builtin::Coupler { coupling: 0.5 }), Pos2::new(100.0, 0.0));
        let ps = s.add(b(Builtin::PhaseShifter { phase }), Pos2::new(200.0, -40.0));
        let c2 = s.add(b(Builtin::Coupler { coupling: 0.5 }), Pos2::new(300.0, 0.0));
        let d0 = s.add(b(Builtin::Detector), Pos2::new(400.0, -40.0));
        let d1 = s.add(b(Builtin::Detector), Pos2::new(400.0, 40.0));
        s.connect(port(laser, 0), port(c1, 0));
        s.connect(port(c1, 0), port(ps, 0));
        s.connect(port(ps, 0), port(c2, 0));
        s.connect(port(c1, 1), port(c2, 1));
        s.connect(port(c2, 0), port(d0, 0));
        s.connect(port(c2, 1), port(d1, 0));
        (s, d0, d1)
    }

    #[test]
    fn mzi_switches_with_phase() {
        let (s, d0, d1) = mzi(0.0);
        let sim = simulate(&s, &NoComponents, &mut Vec::new());
        assert!(sim.error.is_none(), "{:?}", sim.error);
        assert_eq!(sim.wavelengths, vec![1550.0]);
        assert!(sim.reading(d0).unwrap() < 1e-12 && (sim.reading(d1).unwrap() - 1.0).abs() < 1e-12);
        let (s, d0, _) = mzi(std::f64::consts::PI);
        let sim = simulate(&s, &NoComponents, &mut Vec::new());
        assert!((sim.reading(d0).unwrap() - 1.0).abs() < 1e-12);
        assert_eq!(sim.reading(InstanceId(99)), None);
    }

    #[test]
    fn wire_fields_and_port_power() {
        let (s, ..) = mzi(0.0);
        let sim = simulate(&s, &NoComponents, &mut Vec::new());
        assert_eq!(sim.wire_fields.len(), s.wires.len());
        assert!(
            (sim.wire_power(0) - 1.0).abs() < 1e-12,
            "laser wire carries full power"
        );
        assert!((sim.wire_power(1) - 0.5).abs() < 1e-12);
        assert_eq!(sim.wire_power(99), 0.0);
        let c1 = s.instances[1].id;
        assert!((sim.port_power[&(c1, 1)] - 0.5).abs() < 1e-12);
        assert!(sim.nodes >= 6);
    }

    #[test]
    fn geometric_wires_add_phase() {
        let (mut s, d0, _) = mzi(0.0);
        // Ideal wires: balanced. A fixed half-wave delay on one arm flips the output.
        s.wires[2].length_um = Some(1550.0 / 1000.0 / 2.4 / 2.0);
        let sim = simulate(&s, &NoComponents, &mut Vec::new());
        assert!((sim.reading(d0).unwrap() - 1.0).abs() < 1e-9);

        // Geometric wires: the arms differ in drawn length, so the result changes.
        let (mut s, d0, _) = mzi(0.0);
        s.wire_settings.model = WireModel::Geometric {
            um_per_unit: 0.0137,
        };
        let geometric = simulate(&s, &NoComponents, &mut Vec::new())
            .reading(d0)
            .unwrap();
        assert!(geometric > 1e-6, "{geometric}");
    }

    #[test]
    fn output_pins_read_power_and_define_ports() {
        let mut s = Schematic::default();
        let input = s.add(b(Builtin::InputPin { name: "a".into() }), Pos2::ZERO);
        let split = s.add(b(Builtin::YSplitter), Pos2::new(100.0, 0.0));
        let top = s.add(
            b(Builtin::OutputPin { name: "top".into() }),
            Pos2::new(200.0, -30.0),
        );
        let bottom = s.add(
            b(Builtin::OutputPin {
                name: "bottom".into(),
            }),
            Pos2::new(200.0, 30.0),
        );
        s.connect(port(input, 0), port(split, 0));
        s.connect(port(split, 0), port(top, 0));
        s.connect(port(split, 1), port(bottom, 0));

        let mut c = Circuit::new();
        let built = build_schematic(&s, &NoComponents, &mut c, &mut Vec::new()).unwrap();
        assert_eq!(
            (built.ports.inputs.len(), built.ports.outputs.len()),
            (1, 2)
        );
        let sol = c
            .simulate(1550.0, &[(built.ports.inputs[0], C64::ONE)])
            .unwrap();
        for out in &built.ports.outputs {
            assert!((sol.output_field(*out).norm_sqr() - 0.5).abs() < 1e-12);
        }

        // Without a laser there is nothing to see, and that is not an error.
        let sim = simulate(&s, &NoComponents, &mut Vec::new());
        assert!(sim.error.is_none() && sim.wavelengths.is_empty());
        assert_eq!(sim.readings.len(), 2);
        for (_, power) in &sim.readings {
            assert!(power.is_sign_positive(), "dark readings print as 0, not -0");
        }
    }

    #[test]
    fn errors_are_reported() {
        let mut s = Schematic::default();
        s.add(BlockRef::Component(DocId(5)), Pos2::ZERO);
        assert_eq!(
            simulate(&s, &NoComponents, &mut Vec::new())
                .error
                .as_deref(),
            Some("unknown component 5")
        );

        let (mut s, ..) = mzi(0.0);
        s.wires[0].from.port = 7;
        let e = simulate(&s, &NoComponents, &mut Vec::new()).error.unwrap();
        assert!(e.contains("Wire 1 starts at port #8"), "{e}");
        let (mut s, ..) = mzi(0.0);
        s.wires[5].to.port = 3;
        assert!(
            simulate(&s, &NoComponents, &mut Vec::new())
                .error
                .unwrap()
                .contains("ends at")
        );
        assert_eq!(SchematicSim::failed("x").error.as_deref(), Some("x"));
    }
}
