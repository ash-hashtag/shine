//! Browser for chip-scale circuits built from [`photonics::blocks`]: each
//! example has parameters, runs the circuit solver and plots the result.

use egui_plot::{Bar, BarChart, Legend, Line, Plot};

use crate::photonics::blocks::{
    AddDropRing, AllPassRing, Dft, SplitterTree, SwitchTree, UnbalancedMzi, WdmDemux,
    output_powers, resonant_length_um,
};
use crate::photonics::{Block, C64, Circuit, InPort, Matrix, OutPort, SvdMesh};

pub struct Series {
    pub name: String,
    pub points: Vec<[f64; 2]>,
}

pub struct BarGroup {
    pub name: String,
    pub values: Vec<f64>,
}

pub enum Chart {
    Lines {
        x_label: &'static str,
        y_label: &'static str,
        series: Vec<Series>,
    },
    Bars {
        x_label: &'static str,
        y_label: &'static str,
        groups: Vec<BarGroup>,
    },
}

pub struct ChipResult {
    pub chart: Chart,
    pub notes: Vec<String>,
}

pub trait ChipExample: Send {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// Parameter widgets; returns whether anything changed.
    fn params_ui(&mut self, ui: &mut egui::Ui) -> bool;
    fn compute(&self) -> Result<ChipResult, String>;
}

fn linspace(start: f64, end: f64, n: usize) -> Vec<f64> {
    (0..n)
        .map(|k| start + (end - start) * k as f64 / (n - 1).max(1) as f64)
        .collect()
}

/// Power on each named output over a wavelength sweep (wavelengths solved in parallel).
fn spectra(
    circuit: &Circuit,
    input: InPort,
    outputs: &[(String, OutPort)],
    wavelengths: &[f64],
) -> Result<Vec<Series>, String> {
    let sols = circuit
        .sweep(wavelengths, &[(input, C64::ONE)])
        .map_err(|e| e.to_string())?;
    Ok(outputs
        .iter()
        .map(|(name, port)| Series {
            name: name.clone(),
            points: sols
                .iter()
                .map(|s| [s.wavelength_nm(), s.output_field(*port).norm_sqr()])
                .collect(),
        })
        .collect())
}

fn build(block: &impl Block) -> Result<(Circuit, crate::photonics::Ports), String> {
    let mut c = Circuit::new();
    let ports = c.add_block(block).map_err(|e| e.to_string())?;
    Ok((c, ports))
}

fn slider<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    text: &str,
) -> bool {
    ui.add(egui::Slider::new(value, range).text(text)).changed()
}

pub struct RingNotch {
    pub coupling: f64,
    pub length_um: f64,
    pub loss_db_per_cm: f64,
}

impl ChipExample for RingNotch {
    fn name(&self) -> &'static str {
        "Ring resonator (notch filter)"
    }
    fn description(&self) -> &'static str {
        "A ring next to a bus waveguide removes wavelengths that fit a whole number of times around it. When the coupling equals the round-trip loss (critical coupling) the notch goes to zero — used for filters, modulators and sensors."
    }
    fn params_ui(&mut self, ui: &mut egui::Ui) -> bool {
        slider(ui, &mut self.coupling, 0.001..=0.5, "coupling")
            | slider(ui, &mut self.length_um, 10.0..=500.0, "length µm")
            | slider(ui, &mut self.loss_db_per_cm, 0.0..=100.0, "loss dB/cm")
    }
    fn compute(&self) -> Result<ChipResult, String> {
        let ring = AllPassRing {
            coupling: self.coupling,
            length_um: self.length_um,
            n_eff: 2.4,
            loss_db_per_cm: self.loss_db_per_cm,
        };
        let (c, p) = build(&ring)?;
        let fsr = ring.fsr_nm(1550.0);
        let wls = linspace(1550.0 - fsr, 1550.0 + fsr, 801);
        let series = spectra(&c, p.inputs[0], &[("through".into(), p.outputs[0])], &wls)?;
        let min = series[0]
            .points
            .iter()
            .map(|p| p[1])
            .fold(f64::INFINITY, f64::min);
        let max_err = wls
            .iter()
            .zip(&series[0].points)
            .map(|(&wl, pt)| (ring.through(wl).norm_sqr() - pt[1]).abs())
            .fold(0.0, f64::max);
        Ok(ChipResult {
            chart: Chart::Lines {
                x_label: "wavelength (nm)",
                y_label: "power",
                series,
            },
            notes: vec![
                format!("Free spectral range: {fsr:.3} nm"),
                format!("Deepest notch: {:.1} dB", 10.0 * min.max(1e-12).log10()),
                format!("Max deviation from closed form: {max_err:.1e}"),
            ],
        })
    }
}

pub struct AddDrop {
    pub coupling: f64,
    pub order: u32,
}

impl ChipExample for AddDrop {
    fn name(&self) -> &'static str {
        "Add–drop ring filter"
    }
    fn description(&self) -> &'static str {
        "A ring between two waveguides moves its resonant wavelength from the input bus to the drop port and lets everything else pass — one channel of a wavelength router."
    }
    fn params_ui(&mut self, ui: &mut egui::Ui) -> bool {
        slider(ui, &mut self.coupling, 0.005..=0.5, "coupling")
            | slider(ui, &mut self.order, 5..=200, "ring order")
    }
    fn compute(&self) -> Result<ChipResult, String> {
        let ring = AddDropRing::resonant(1550.0, 2.4, self.order, self.coupling);
        let (c, p) = build(&ring)?;
        let fsr = 1550.0 / self.order as f64;
        let wls = linspace(1550.0 - 0.6 * fsr, 1550.0 + 0.6 * fsr, 801);
        let series = spectra(
            &c,
            p.inputs[0],
            &[
                ("through".into(), p.outputs[0]),
                ("drop".into(), p.outputs[1]),
            ],
            &wls,
        )?;
        Ok(ChipResult {
            chart: Chart::Lines {
                x_label: "wavelength (nm)",
                y_label: "power",
                series,
            },
            notes: vec![
                format!(
                    "Ring length: {:.2} µm",
                    resonant_length_um(1550.0, 2.4, self.order)
                ),
                format!("FSR ≈ {fsr:.2} nm"),
            ],
        })
    }
}

pub struct Interleaver {
    pub delay_um: f64,
}

impl ChipExample for Interleaver {
    fn name(&self) -> &'static str {
        "Interleaver (unbalanced MZI)"
    }
    fn description(&self) -> &'static str {
        "An extra length in one arm makes the interferometer wavelength-periodic: alternate channels leave on alternate outputs. Cascades of these build lattice filters."
    }
    fn params_ui(&mut self, ui: &mut egui::Ui) -> bool {
        slider(ui, &mut self.delay_um, 5.0..=500.0, "delay µm")
    }
    fn compute(&self) -> Result<ChipResult, String> {
        let mzi = UnbalancedMzi {
            delay_um: self.delay_um,
            n_eff: 2.4,
        };
        let (c, p) = build(&mzi)?;
        let wls = linspace(1540.0, 1560.0, 801);
        let series = spectra(
            &c,
            p.inputs[0],
            &[("bar".into(), p.outputs[0]), ("cross".into(), p.outputs[1])],
            &wls,
        )?;
        Ok(ChipResult {
            chart: Chart::Lines {
                x_label: "wavelength (nm)",
                y_label: "power",
                series,
            },
            notes: vec![format!(
                "Channel period (FSR): {:.3} nm",
                mzi.fsr_nm(1550.0)
            )],
        })
    }
}

pub struct Demux {
    pub channels: usize,
    pub spacing_nm: f64,
    pub coupling: f64,
}

impl Demux {
    fn block(&self) -> WdmDemux {
        let mid = (self.channels as f64 - 1.0) / 2.0;
        WdmDemux {
            channels_nm: (0..self.channels)
                .map(|k| 1550.0 + (k as f64 - mid) * self.spacing_nm)
                .collect(),
            n_eff: 2.4,
            order: 20,
            coupling: self.coupling,
        }
    }
}

impl ChipExample for Demux {
    fn name(&self) -> &'static str {
        "WDM demultiplexer"
    }
    fn description(&self) -> &'static str {
        "A row of add–drop rings, each tuned to one channel, splits a multi-wavelength signal into separate outputs — how one waveguide carries many data streams."
    }
    fn params_ui(&mut self, ui: &mut egui::Ui) -> bool {
        slider(ui, &mut self.channels, 1..=8, "channels")
            | slider(ui, &mut self.spacing_nm, 0.5..=9.0, "spacing nm")
            | slider(ui, &mut self.coupling, 0.005..=0.3, "coupling")
    }
    fn compute(&self) -> Result<ChipResult, String> {
        let demux = self.block();
        let (c, p) = build(&demux)?;
        let span = self.spacing_nm * (self.channels as f64 + 1.0) / 2.0;
        let wls = linspace(1550.0 - span, 1550.0 + span, 1001);
        let mut outputs: Vec<(String, OutPort)> = demux
            .channels_nm
            .iter()
            .zip(&p.outputs)
            .map(|(wl, &o)| (format!("drop {wl:.1} nm"), o))
            .collect();
        outputs.push(("through".into(), *p.outputs.last().unwrap()));
        let series = spectra(&c, p.inputs[0], &outputs, &wls)?;
        Ok(ChipResult {
            chart: Chart::Lines {
                x_label: "wavelength (nm)",
                y_label: "power",
                series,
            },
            notes: vec![format!(
                "{} rings, {} circuit nodes",
                self.channels,
                c.len()
            )],
        })
    }
}

pub struct Router {
    pub levels: u32,
    pub address: usize,
}

impl ChipExample for Router {
    fn name(&self) -> &'static str {
        "MZI switch tree (router)"
    }
    fn description(&self) -> &'static str {
        "Binary tree of Mach–Zehnder switches. The address bits set each switch to bar or cross, steering the input to exactly one output — an optical address decoder."
    }
    fn params_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = slider(ui, &mut self.levels, 1..=5, "levels");
        self.address = self.address.min((1 << self.levels) - 1);
        changed |= slider(ui, &mut self.address, 0..=(1 << self.levels) - 1, "address");
        changed
    }
    fn compute(&self) -> Result<ChipResult, String> {
        let tree = SwitchTree {
            levels: self.levels,
            address: self.address,
        };
        let (c, p) = build(&tree)?;
        let powers =
            output_powers(&c, 1550.0, p.inputs[0], &p.outputs).map_err(|e| e.to_string())?;
        Ok(ChipResult {
            chart: Chart::Bars {
                x_label: "output",
                y_label: "power",
                groups: vec![BarGroup {
                    name: "power".into(),
                    values: powers,
                }],
            },
            notes: vec![format!(
                "Address {} = 0b{:0width$b}; {} MZIs",
                self.address,
                self.address,
                (1 << self.levels) - 1,
                width = self.levels as usize
            )],
        })
    }
}

pub struct Fourier {
    pub n: usize,
    pub tone: usize,
}

impl ChipExample for Fourier {
    fn name(&self) -> &'static str {
        "Optical Fourier transform"
    }
    fn description(&self) -> &'static str {
        "An MZI mesh programmed with the DFT matrix. Inputs carry a sampled tone e^{2πi·k·j/n}; the transform focuses all of its power into output k — computation at the speed of light."
    }
    fn params_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = slider(ui, &mut self.n, 2..=16, "modes");
        self.tone = self.tone.min(self.n - 1);
        changed |= slider(ui, &mut self.tone, 0..=self.n - 1, "input tone k");
        changed
    }
    fn compute(&self) -> Result<ChipResult, String> {
        let dft = Dft { n: self.n };
        let (c, p) = build(&dft)?;
        let drives: Vec<_> = p
            .inputs
            .iter()
            .enumerate()
            .map(|(j, &i)| {
                let phase = 2.0 * std::f64::consts::PI * (self.tone * j) as f64 / self.n as f64;
                (i, C64::from_polar(1.0, phase))
            })
            .collect();
        let sol = c.simulate(1550.0, &drives).map_err(|e| e.to_string())?;
        Ok(ChipResult {
            chart: Chart::Bars {
                x_label: "output mode",
                y_label: "power",
                groups: vec![BarGroup {
                    name: "power".into(),
                    values: p
                        .outputs
                        .iter()
                        .map(|&o| sol.output_field(o).norm_sqr())
                        .collect(),
                }],
            },
            notes: vec![format!(
                "{} MZIs in a triangular mesh",
                self.n * (self.n - 1) / 2
            )],
        })
    }
}

/// Deterministic pseudo-random numbers in [-1, 1).
fn pseudo_random(seed: u64, count: usize) -> Vec<f64> {
    let mut state = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
        })
        .collect()
}

pub struct MatMul {
    pub n: usize,
    pub seed: u64,
}

impl ChipExample for MatMul {
    fn name(&self) -> &'static str {
        "Matrix–vector multiplier"
    }
    fn description(&self) -> &'static str {
        "Any matrix W = U·Σ·V† is realised with two MZI meshes and a column of attenuators. Light encoding x comes out as W·x — the core of optical neural-network accelerators."
    }
    fn params_ui(&mut self, ui: &mut egui::Ui) -> bool {
        slider(ui, &mut self.n, 2..=12, "size") | slider(ui, &mut self.seed, 0..=99, "random seed")
    }
    fn compute(&self) -> Result<ChipResult, String> {
        let n = self.n;
        let w_vals = pseudo_random(self.seed, n * n);
        let x_vals = pseudo_random(self.seed + 1000, n);
        let w = Matrix::from_fn(n, n, |i, j| C64::new(w_vals[i * n + j], 0.0));
        let mesh = SvdMesh::decompose(&w).map_err(|e| e.to_string())?;
        let (c, p) = build(&mesh)?;
        let drives: Vec<_> = p
            .inputs
            .iter()
            .zip(&x_vals)
            .map(|(&i, &x)| (i, C64::new(x, 0.0)))
            .collect();
        let sol = c.simulate(1550.0, &drives).map_err(|e| e.to_string())?;
        let optical: Vec<f64> = p
            .outputs
            .iter()
            .map(|&o| (sol.output_field(o) * mesh.scale).re)
            .collect();
        let exact: Vec<f64> = (0..n)
            .map(|i| (0..n).map(|j| w_vals[i * n + j] * x_vals[j]).sum())
            .collect();
        let err = optical
            .iter()
            .zip(&exact)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        Ok(ChipResult {
            chart: Chart::Bars {
                x_label: "output i",
                y_label: "(W·x)ᵢ",
                groups: vec![
                    BarGroup {
                        name: "exact".into(),
                        values: exact,
                    },
                    BarGroup {
                        name: "optical".into(),
                        values: optical,
                    },
                ],
            },
            notes: vec![
                format!(
                    "{} MZIs, {} attenuators, {} circuit nodes",
                    n * (n - 1),
                    n,
                    c.len()
                ),
                format!("Max error: {err:.1e}"),
            ],
        })
    }
}

pub struct FanOut {
    pub levels: u32,
}

impl ChipExample for FanOut {
    fn name(&self) -> &'static str {
        "1×N splitter tree"
    }
    fn description(&self) -> &'static str {
        "Cascaded Y-junctions divide one input equally among 2^levels outputs: the fan-out that broadcasts signals across a chip."
    }
    fn params_ui(&mut self, ui: &mut egui::Ui) -> bool {
        slider(ui, &mut self.levels, 0..=6, "levels")
    }
    fn compute(&self) -> Result<ChipResult, String> {
        let (c, p) = build(&SplitterTree {
            levels: self.levels,
        })?;
        let powers =
            output_powers(&c, 1550.0, p.inputs[0], &p.outputs).map_err(|e| e.to_string())?;
        Ok(ChipResult {
            chart: Chart::Bars {
                x_label: "output",
                y_label: "power",
                groups: vec![BarGroup {
                    name: "power".into(),
                    values: powers,
                }],
            },
            notes: vec![format!("{} outputs", 1usize << self.levels)],
        })
    }
}

pub fn chip_examples() -> Vec<Box<dyn ChipExample>> {
    vec![
        Box::new(RingNotch {
            coupling: 0.05,
            length_um: 100.0,
            loss_db_per_cm: 20.0,
        }),
        Box::new(AddDrop {
            coupling: 0.05,
            order: 20,
        }),
        Box::new(Interleaver { delay_um: 50.0 }),
        Box::new(Demux {
            channels: 4,
            spacing_nm: 4.0,
            coupling: 0.03,
        }),
        Box::new(Router {
            levels: 3,
            address: 5,
        }),
        Box::new(Fourier { n: 8, tone: 3 }),
        Box::new(MatMul { n: 6, seed: 7 }),
        Box::new(FanOut { levels: 3 }),
    ]
}

pub struct ChipBrowser {
    pub examples: Vec<Box<dyn ChipExample>>,
    pub selected: usize,
    result: Option<Result<ChipResult, String>>,
}

impl Default for ChipBrowser {
    fn default() -> Self {
        Self {
            examples: chip_examples(),
            selected: 0,
            result: None,
        }
    }
}

impl ChipBrowser {
    pub fn select(&mut self, index: usize) {
        if index != self.selected && index < self.examples.len() {
            self.selected = index;
            self.result = None;
        }
    }

    /// Result for the selected example, computed on demand.
    pub fn result(&mut self) -> &Result<ChipResult, String> {
        let example = &self.examples[self.selected];
        self.result.get_or_insert_with(|| {
            let start = web_time::Instant::now();
            let r = example.compute();
            tracing::debug!("Computed '{}' in {:?}", example.name(), start.elapsed());
            if let Err(e) = &r {
                tracing::warn!("Example '{}' failed: {e}", example.name());
            }
            r
        })
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(210.0);
                for i in 0..self.examples.len() {
                    let name = self.examples[i].name();
                    if ui.selectable_label(i == self.selected, name).clicked() {
                        self.select(i);
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                let example = &mut self.examples[self.selected];
                ui.heading(example.name());
                ui.label(example.description());
                ui.separator();
                if example.params_ui(ui) {
                    self.result = None;
                }
                ui.separator();
                let id = self.selected;
                match self.result() {
                    Ok(result) => {
                        for note in &result.notes {
                            ui.label(note);
                        }
                        draw_chart(ui, id, &result.chart);
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::RED, e);
                    }
                }
            });
        });
    }
}

fn draw_chart(ui: &mut egui::Ui, id: usize, chart: &Chart) {
    let plot = |x: &str, y: &str| {
        Plot::new(("chip_plot", id))
            .height(300.0)
            .legend(Legend::default())
            .x_axis_label(x.to_owned())
            .y_axis_label(y.to_owned())
            .include_y(0.0)
    };
    match chart {
        Chart::Lines {
            x_label,
            y_label,
            series,
        } => {
            plot(x_label, y_label).show(ui, |p| {
                for s in series {
                    p.line(Line::new(s.name.clone(), s.points.clone()));
                }
            });
        }
        Chart::Bars {
            x_label,
            y_label,
            groups,
        } => {
            let width = 0.8 / groups.len().max(1) as f64;
            plot(x_label, y_label).show(ui, |p| {
                for (g, group) in groups.iter().enumerate() {
                    let offset = (g as f64 - (groups.len() as f64 - 1.0) / 2.0) * width;
                    let bars = group
                        .values
                        .iter()
                        .enumerate()
                        .map(|(i, &v)| Bar::new(i as f64 + offset, v).width(width))
                        .collect();
                    p.bar_chart(BarChart::new(group.name.clone(), bars));
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(r: &ChipResult) -> &[Series] {
        match &r.chart {
            Chart::Lines { series, .. } => series,
            Chart::Bars { .. } => panic!("expected lines"),
        }
    }

    fn bars(r: &ChipResult) -> &[BarGroup] {
        match &r.chart {
            Chart::Bars { groups, .. } => groups,
            Chart::Lines { .. } => panic!("expected bars"),
        }
    }

    #[test]
    fn helpers() {
        assert_eq!(linspace(0.0, 1.0, 3), vec![0.0, 0.5, 1.0]);
        assert_eq!(linspace(2.0, 3.0, 1), vec![2.0]);
        let r = pseudo_random(1, 100);
        assert!(r.iter().all(|v| (-1.0..1.0).contains(v)));
        assert_eq!(r, pseudo_random(1, 100));
        assert_ne!(r, pseudo_random(2, 100));
    }

    #[test]
    fn every_example_computes() {
        for ex in chip_examples() {
            let r = ex
                .compute()
                .unwrap_or_else(|e| panic!("{}: {e}", ex.name()));
            assert!(!ex.description().is_empty());
            assert!(!r.notes.is_empty(), "{}", ex.name());
        }
    }

    #[test]
    fn ring_notch_matches_closed_form() {
        let r = RingNotch {
            coupling: 0.05,
            length_um: 100.0,
            loss_db_per_cm: 20.0,
        }
        .compute()
        .unwrap();
        let s = &lines(&r)[0];
        assert_eq!(s.points.len(), 801);
        assert!(r.notes[2].contains("e-1"), "{}", r.notes[2]);
        let min = s.points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
        let max = s.points.iter().map(|p| p[1]).fold(0.0, f64::max);
        assert!(min < 0.5 && max > 0.9);
    }

    #[test]
    fn add_drop_peaks_at_centre() {
        let r = AddDrop {
            coupling: 0.05,
            order: 20,
        }
        .compute()
        .unwrap();
        let drop = &lines(&r)[1];
        let peak = drop
            .points
            .iter()
            .max_by(|a, b| a[1].total_cmp(&b[1]))
            .unwrap();
        assert!((peak[0] - 1550.0).abs() < 0.2 && peak[1] > 0.99, "{peak:?}");
    }

    #[test]
    fn interleaver_conserves_power() {
        let r = Interleaver { delay_um: 50.0 }.compute().unwrap();
        let [bar, cross] = lines(&r) else { panic!() };
        for (a, b) in bar.points.iter().zip(&cross.points) {
            assert!((a[1] + b[1] - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn demux_has_a_series_per_port() {
        let d = Demux {
            channels: 3,
            spacing_nm: 5.0,
            coupling: 0.03,
        };
        let r = d.compute().unwrap();
        let s = lines(&r);
        assert_eq!(s.len(), 4);
        assert_eq!(s[3].name, "through");
        // Each drop port peaks at its own channel.
        for (k, wl) in d.block().channels_nm.iter().enumerate() {
            let peak = s[k]
                .points
                .iter()
                .max_by(|a, b| a[1].total_cmp(&b[1]))
                .unwrap();
            assert!(
                (peak[0] - wl).abs() < 0.3 && peak[1] > 0.9,
                "channel {k}: {peak:?}"
            );
        }
    }

    #[test]
    fn router_selects_address() {
        let r = Router {
            levels: 3,
            address: 6,
        }
        .compute()
        .unwrap();
        let v = &bars(&r)[0].values;
        assert_eq!(v.len(), 8);
        for (k, p) in v.iter().enumerate() {
            assert!((p - if k == 6 { 1.0 } else { 0.0 }).abs() < 1e-9);
        }
        assert!(r.notes[0].contains("0b110"));
    }

    #[test]
    fn fourier_focuses_tone() {
        let r = Fourier { n: 8, tone: 5 }.compute().unwrap();
        let v = &bars(&r)[0].values;
        for (k, p) in v.iter().enumerate() {
            assert!((p - if k == 5 { 8.0 } else { 0.0 }).abs() < 1e-8, "{v:?}");
        }
    }

    #[test]
    fn matmul_is_accurate() {
        let r = MatMul { n: 5, seed: 3 }.compute().unwrap();
        let [exact, optical] = bars(&r) else { panic!() };
        for (a, b) in exact.values.iter().zip(&optical.values) {
            assert!((a - b).abs() < 1e-9);
        }
    }

    #[test]
    fn fan_out_is_even() {
        let r = FanOut { levels: 2 }.compute().unwrap();
        let v = &bars(&r)[0].values;
        assert_eq!(v.len(), 4);
        for p in v {
            assert!((p - 0.25).abs() < 1e-12);
        }
    }

    #[test]
    fn browser_caches_and_switches() {
        let mut b = ChipBrowser::default();
        assert!(b.result().is_ok());
        assert!(b.result.is_some());
        b.select(0);
        assert!(b.result.is_some(), "re-selecting keeps the cache");
        b.select(4);
        assert!(b.result.is_none());
        b.select(999);
        assert_eq!(b.selected, 4);
        assert!(b.result().is_ok());
    }

    #[test]
    fn browser_renders_every_example() {
        let ctx = egui::Context::default();
        let mut b = ChipBrowser::default();
        for i in 0..b.examples.len() {
            b.select(i);
            ctx.run_ui(Default::default(), |ui| b.ui(ui))
                .textures_delta
                .clear();
        }
    }
}
