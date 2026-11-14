//! Shine's library for Python and TypeScript (`shine-photonics`): design
//! photonic circuits in code, simulate, sweep, analyse and export them, and
//! open them in the Shine app. This crate exports `shine::api` with BoltFFI;
//! the doc comments here are the packages' docstrings.

use boltffi::{data, error, export};
use shine::api;

boltffi::scaffolding!();

// ---- errors -----------------------------------------------------------------------

/// What went wrong, in words that say what would have worked: “Ring
/// resonator” has no port “drop”; it has: in, through.
#[error]
pub struct ShineError {
    pub message: String,
}

impl From<api::Error> for ShineError {
    fn from(e: api::Error) -> Self {
        Self { message: e.to_string() }
    }
}

// ---- records ----------------------------------------------------------------------

/// Whether a port carries light (a waveguide) or a signal (a wire).
#[data]
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PortKind {
    Light,
    Signal,
}

/// Whether light or a signal goes into a port or comes out of it.
#[data]
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Direction {
    In,
    Out,
}

/// A port of a part: its name, what it carries and which way.
#[data]
#[derive(Clone)]
pub struct PortInfo {
    pub name: String,
    pub kind: PortKind,
    pub direction: Direction,
}

impl From<api::PortInfo> for PortInfo {
    fn from(p: api::PortInfo) -> Self {
        Self {
            name: p.name,
            kind: match p.kind {
                api::PortKind::Light => PortKind::Light,
                api::PortKind::Signal => PortKind::Signal,
            },
            direction: match p.direction {
                api::Direction::In => Direction::In,
                api::Direction::Out => Direction::Out,
            },
        }
    }
}

/// A part's parameter: name, unit, current value and the range it typically
/// takes (values outside it are allowed).
#[data]
#[derive(Clone)]
pub struct Param {
    pub name: String,
    pub unit: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
}

/// A complex number: a field as its real and imaginary parts (|z|² is its
/// power in mW).
#[data]
#[derive(Clone, Copy)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

/// A matrix of complex numbers, row by row: element (row, col) is
/// `values[row * cols + col]`. For a transmission, rows are outputs and
/// columns inputs.
#[data]
#[derive(Clone)]
pub struct Matrix {
    pub rows: u32,
    pub cols: u32,
    pub values: Vec<Complex>,
}

impl From<api::Matrix> for Matrix {
    fn from(m: api::Matrix) -> Self {
        Self { rows: m.rows, cols: m.cols, values: m.values.into_iter().map(|z| Complex { re: z.re, im: z.im }).collect() }
    }
}

impl From<Matrix> for api::Matrix {
    fn from(m: Matrix) -> Self {
        Self { rows: m.rows, cols: m.cols, values: m.values.into_iter().map(|z| api::Complex::new(z.re, z.im)).collect() }
    }
}

/// The power at a detector or light output, or leaving a part's light
/// output ("Ring resonator.through"), in mW.
#[data]
#[derive(Clone)]
pub struct Reading {
    pub name: String,
    pub power_mw: f64,
}

/// One output's power at every value of a sweep, in mW.
#[data]
#[derive(Clone)]
pub struct Series {
    pub name: String,
    pub power_mw: Vec<f64>,
}

/// What a wavelength sweep says about an output's resonances.
#[data]
#[derive(Clone)]
pub struct Readouts {
    /// Whether the resonances are dips (else peaks).
    pub dips: bool,
    /// Where each resonance is, in nm.
    pub resonances_nm: Vec<f64>,
    /// Free spectral range: the mean spacing of neighbouring resonances.
    pub fsr_nm: Option<f64>,
    /// Full width at half depth of the strongest resonance.
    pub fwhm_nm: Option<f64>,
    /// Quality factor: wavelength over width.
    pub q: Option<f64>,
    /// Largest over smallest power, in dB.
    pub extinction_db: f64,
}

/// An output's mean, standard deviation and extremes over Monte Carlo runs, in mW.
#[data]
#[derive(Clone, Copy)]
pub struct Stats {
    pub mean: f64,
    pub std: f64,
    pub min: f64,
    pub max: f64,
}

/// How much each kind of parameter varies in a Monte Carlo run (standard
/// deviations): effective index (absolute), coupling (relative: 0.03 is 3%),
/// phase (radians) and loss (relative). The defaults are typical of silicon
/// photonics.
#[data]
#[derive(Clone, Copy)]
pub struct Variation {
    #[boltffi::default(0.002)]
    pub n_eff: f64,
    #[boltffi::default(0.03)]
    pub coupling: f64,
    #[boltffi::default(0.05)]
    pub phase: f64,
    #[boltffi::default(0.1)]
    pub loss: f64,
}

/// A parameter an optimisation may adjust: a part (by name), its parameter,
/// and the range to stay within.
#[data]
#[derive(Clone)]
pub struct Variable {
    pub part: String,
    pub param: String,
    pub min: f64,
    pub max: f64,
}

/// What an optimisation aims for.
#[data]
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Goal {
    Maximise,
    Minimise,
    /// Reach `target_mw`.
    Target,
}

/// One output an optimisation aims for, among several: its goal (and
/// `target_mw` for Target), met as the design is or, with `band_points` of 2
/// or more, on average across `band_from_nm`…`band_to_nm` (the design's first
/// laser tuned to each), and its `weight` against the others.
#[data]
#[derive(Clone)]
pub struct Aim {
    pub output: String,
    pub goal: Goal,
    pub target_mw: f64,
    pub band_from_nm: f64,
    pub band_to_nm: f64,
    pub band_points: u32,
    pub weight: f64,
}

/// What an optimisation of several aims found: the best values, each aim's
/// power before and after (mW; a band's average), and the simulations.
#[data]
#[derive(Clone)]
pub struct Aimed {
    pub values: Vec<f64>,
    pub before_mw: Vec<f64>,
    pub after_mw: Vec<f64>,
    pub simulations: u32,
}

/// What an optimisation found: the best values (one per variable), the
/// output's power before and after, and how many simulations it took.
#[data]
#[derive(Clone)]
pub struct Optimised {
    pub values: Vec<f64>,
    pub before_mw: f64,
    pub after_mw: f64,
    pub simulations: u32,
}

/// A component's transmission written in code: return the matrix (rows the
/// outputs, columns the inputs) at a wavelength in nm.
#[export]
pub trait TransferFunction: Send + Sync {
    fn matrix(&self, wavelength_nm: f64) -> Matrix;
}

// ---- Project ----------------------------------------------------------------------

/// A Shine project: designs and the components they use, as one `.shine`
/// file. Save it and open it in the Shine app, or open one made there.
#[export]
pub struct Project(api::Project);

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

#[export]
impl Project {
    /// An empty project.
    pub fn new() -> Self {
        Self(api::Project::new())
    }

    /// A `.shine` file.
    pub fn open(path: String) -> Result<Self, ShineError> {
        Ok(Self(api::Project::open(&path)?))
    }

    /// A project from a `.shine` file's contents.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self, ShineError> {
        Ok(Self(api::Project::from_bytes(&data)?))
    }

    /// Save as a `.shine` file, to open in the app. Parts placed without a
    /// position are laid out left to right as light flows.
    pub fn save(&self, path: String) -> Result<(), ShineError> {
        Ok(self.0.save(&path)?)
    }

    /// The project as a `.shine` file's contents.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.to_bytes()
    }

    /// A new, empty design (a name in use gets a number after it).
    pub fn new_design(&self, name: String) -> Design {
        Design(self.0.new_design(&name))
    }

    /// A new, empty component: a design others place as one part. Its pins
    /// are its ports; the parameters it exposes are what each copy sets.
    pub fn new_component(&self, name: String) -> Design {
        Design(self.0.new_component(&name))
    }

    /// A design or component by name.
    pub fn design(&self, name: String) -> Result<Design, ShineError> {
        Ok(Design(self.0.design(&name)?))
    }

    /// The names of every design and component, in order.
    pub fn designs(&self) -> Vec<String> {
        self.0.designs()
    }

    /// A component known by its S-parameters, from a Touchstone file's
    /// contents (`.s2p`, `.s4p`…, versions 1 and 2). Not in the browser.
    pub fn new_model_from_touchstone(&self, name: String, data: Vec<u8>) -> Result<Design, ShineError> {
        Ok(Design(self.0.new_model_from_touchstone(&name, &data)?))
    }

    /// A component from a Lumerical INTERCONNECT file's contents (`.dat` or
    /// `.sparam`, as foundry kits publish them).
    pub fn new_model_from_lumerical(&self, name: String, text: String) -> Result<Design, ShineError> {
        Ok(Design(self.0.new_model_from_lumerical(&name, &text)?))
    }

    /// A component from its transmission at a set of wavelengths: one matrix
    /// per wavelength, rows the outputs and columns the inputs, interpolated
    /// in between.
    pub fn new_model_from_table(
        &self,
        name: String,
        inputs: Vec<String>,
        outputs: Vec<String>,
        wavelengths_nm: Vec<f64>,
        matrices: Vec<Matrix>,
    ) -> Result<Design, ShineError> {
        let matrices = matrices.into_iter().map(Into::into).collect();
        Ok(Design(self.0.new_model_from_table(&name, inputs, outputs, wavelengths_nm, matrices)?))
    }

    /// A component whose transmission `model.matrix(wavelength_nm)` computes:
    /// sampled at `wavelengths_nm` and kept as a table, so it saves with the
    /// project and opens in the app.
    pub fn new_model_from_function(
        &self,
        name: String,
        inputs: Vec<String>,
        outputs: Vec<String>,
        wavelengths_nm: Vec<f64>,
        model: Box<dyn TransferFunction>,
    ) -> Result<Design, ShineError> {
        let f = |wl: f64| -> api::Matrix { model.matrix(wl).into() };
        Ok(Design(self.0.new_model_from_function(&name, inputs, outputs, wavelengths_nm, &f)?))
    }

    /// A weight matrix as light: a component with inputs in1…in{cols} and
    /// outputs out1…out{rows} computing W x / σmax (σmax, W's largest
    /// singular value: light can't be amplified), as two meshes of MZIs
    /// with an attenuator per mode between them. Any shape, real or complex.
    pub fn new_weight_layer(&self, name: String, weights: Matrix) -> Result<Design, ShineError> {
        Ok(Design(self.0.new_weight_layer(&name, &weights.into())?))
    }

    /// A unitary matrix as one mesh of MZIs computing U x.
    pub fn new_unitary_mesh(&self, name: String, unitary: Matrix) -> Result<Design, ShineError> {
        Ok(Design(self.0.new_unitary_mesh(&name, &unitary.into())?))
    }

    /// A component whose light and signals a JavaScript program computes, as
    /// written in the app. Not in the browser.
    pub fn new_scripted_component(&self, code: String) -> Result<Design, ShineError> {
        Ok(Design(self.0.new_scripted_component(&code)?))
    }
}

// ---- Design -----------------------------------------------------------------------

/// A design: parts wired together. A component is a design others place as
/// one part, its pins its ports.
#[export]
pub struct Design(api::Design);

fn part(p: Result<api::Part, api::Error>) -> Result<Part, ShineError> {
    Ok(Part(p?))
}

#[export]
impl Design {
    /// Its name.
    pub fn name(&self) -> String {
        self.0.name()
    }

    /// Whether it is a component (others can place it).
    pub fn is_component(&self) -> bool {
        self.0.is_component()
    }

    /// What a component says it is (empty for a design): for a weight
    /// layer, what it computes, with W's largest singular value.
    pub fn description(&self) -> String {
        self.0.description()
    }

    /// A laser: one wavelength, coherent; its light leaves from `out`.
    pub fn laser(&self, #[boltffi::default(1550.0)] wavelength_nm: f64, #[boltffi::default(1.0)] power_mw: f64) -> Result<Part, ShineError> {
        part(self.0.laser(wavelength_nm, power_mw))
    }

    /// A photodetector: reads the power arriving at `in`.
    pub fn detector(&self) -> Part {
        Part(self.0.detector())
    }

    /// A directional coupler: `coupling` (0 to 1) of the power crosses from
    /// `in1` to `out2` and `in2` to `out1`, a quarter turn ahead; the rest
    /// goes straight through.
    pub fn coupler(&self, #[boltffi::default(0.5)] coupling: f64) -> Result<Part, ShineError> {
        part(self.0.coupler(coupling))
    }

    /// A phase shifter (a heater): adds `phase` radians; a signal on its
    /// `phase` port adds that many times π more.
    pub fn phase_shifter(&self, #[boltffi::default(0.0)] phase: f64) -> Part {
        Part(self.0.phase_shifter(phase))
    }

    /// An attenuator: `loss_db` at every wavelength; a signal on its
    /// `transmission` port scales what passes (0 blocks it).
    pub fn attenuator(&self, loss_db: f64) -> Result<Part, ShineError> {
        part(self.0.attenuator(loss_db))
    }

    /// A length of waveguide (the app's Delay line), `in` → `out`: silicon
    /// strip at 1550 nm unless set otherwise.
    pub fn waveguide(
        &self,
        length_um: f64,
        #[boltffi::default(2.4)] n_eff: f64,
        #[boltffi::default(4.2)] n_g: f64,
        #[boltffi::default(0.0)] loss_db_per_cm: f64,
    ) -> Result<Part, ShineError> {
        let p = self.0.waveguide(length_um)?;
        p.set("n_eff", n_eff)?.set("n_g", n_g)?.set("loss", loss_db_per_cm)?;
        Ok(Part(p))
    }

    /// A Mach–Zehnder interferometer: θ sets the split (π: `in1` → `out1`,
    /// the bar state; 0: crossed; π/2: half each way), φ the phase of `in1`.
    pub fn mzi(&self, #[boltffi::default(1.5707963267948966)] theta: f64, #[boltffi::default(0.0)] phi: f64) -> Part {
        Part(self.0.mzi(theta, phi))
    }

    /// An all-pass ring resonator on a bus, `in` → `through`, with dips at
    /// its resonances (spaced λ²/(n_g·length)).
    pub fn ring(
        &self,
        coupling: f64,
        length_um: f64,
        #[boltffi::default(2.4)] n_eff: f64,
        #[boltffi::default(4.2)] n_g: f64,
        #[boltffi::default(10.0)] loss_db_per_cm: f64,
    ) -> Result<Part, ShineError> {
        let p = self.0.ring(coupling, length_um)?;
        p.set("n_eff", n_eff)?.set("n_g", n_g)?.set("loss", loss_db_per_cm)?;
        Ok(Part(p))
    }

    /// An add–drop ring: `in` → `through`, and at its resonances to `drop`;
    /// `add` joins the drop path.
    pub fn add_drop_ring(
        &self,
        coupling: f64,
        length_um: f64,
        #[boltffi::default(2.4)] n_eff: f64,
        #[boltffi::default(4.2)] n_g: f64,
        #[boltffi::default(0.0)] loss_db_per_cm: f64,
    ) -> Result<Part, ShineError> {
        let p = self.0.add_drop_ring(coupling, length_um)?;
        p.set("n_eff", n_eff)?.set("n_g", n_g)?.set("loss", loss_db_per_cm)?;
        Ok(Part(p))
    }

    /// A Y-junction splitting `in` equally between `out1` and `out2`.
    pub fn y_splitter(&self) -> Part {
        Part(self.0.y_splitter())
    }

    /// A Y-junction joining `in1` and `in2`: only their in-phase part is guided.
    pub fn y_combiner(&self) -> Part {
        Part(self.0.y_combiner())
    }

    /// Two waveguides crossing: `in1` → `out2`, `in2` → `out1`.
    pub fn crossing(&self) -> Part {
        Part(self.0.crossing())
    }

    /// A tree of Y-splitters sharing `in` equally between `outputs` (a power of two).
    pub fn splitter_tree(&self, outputs: u32) -> Result<Part, ShineError> {
        part(self.0.splitter_tree(outputs))
    }

    /// A tree of MZI switches sending `in` to output `address` (from 0) of `outputs`.
    pub fn switch_tree(&self, outputs: u32, #[boltffi::default(0)] address: u32) -> Result<Part, ShineError> {
        part(self.0.switch_tree(outputs, address))
    }

    /// An optical discrete Fourier transform over `modes` waveguides.
    pub fn dft(&self, modes: u32) -> Result<Part, ShineError> {
        part(self.0.dft(modes))
    }

    /// An input value (usually 0 to 1) out on `v`: drives a heater, a
    /// modulator or an attenuator.
    pub fn value_input(&self, #[boltffi::default(0.0)] value: f64) -> Part {
        Part(self.0.value_input(value))
    }

    /// A number out on `bits` signals, most significant first (`b7` … `b0`).
    pub fn number_input(&self, bits: u32, #[boltffi::default(0)] value: u32) -> Result<Part, ShineError> {
        part(self.0.number_input(bits, value as u64))
    }

    /// Shows the number on its `bits` signal inputs.
    pub fn display(&self, bits: u32) -> Result<Part, ShineError> {
        part(self.0.display(bits))
    }

    /// Clocked memory: stores its `d` inputs on each clock tick.
    pub fn register(&self, bits: u32) -> Result<Part, ShineError> {
        part(self.0.register(bits))
    }

    /// Electronics as expressions: each of `outputs` is an expression over
    /// the signal inputs `in1`… and the light powers `p1`… arriving at its
    /// light inputs, as in the app's function cells.
    pub fn function_cell(&self, name: String, inputs: u32, light_inputs: u32, outputs: Vec<String>) -> Result<Part, ShineError> {
        part(self.0.function_cell(&name, inputs, light_inputs, outputs))
    }

    /// A copy of a component, as one part.
    pub fn place(&self, component: &Design) -> Result<Part, ShineError> {
        part(self.0.place(&component.0))
    }

    /// A light input of this component: light from outside leaves it on `out`.
    pub fn light_input(&self, name: String) -> Part {
        Part(self.0.light_input(&name))
    }

    /// A light output of this component: what arrives on its `in` leaves it.
    pub fn light_output(&self, name: String) -> Part {
        Part(self.0.light_output(&name))
    }

    /// A signal input of this component: the value from outside, on `out`.
    pub fn signal_input(&self, name: String) -> Part {
        Part(self.0.signal_input(&name))
    }

    /// A signal output of this component: what arrives on its `in`.
    pub fn signal_output(&self, name: String) -> Part {
        Part(self.0.signal_output(&name))
    }

    /// Let each placed copy of this component set `part`'s parameter
    /// `param`, under the name `name`.
    pub fn expose(&self, name: String, part: &Part, param: String) -> Result<(), ShineError> {
        Ok(self.0.expose(&name, &part.0, &param)?)
    }

    /// Connect a light output to a light input (a waveguide), or a signal
    /// output to a signal input (a wire). A light port takes one waveguide,
    /// so one already there is replaced.
    pub fn connect(&self, source: &Port, target: &Port) -> Result<(), ShineError> {
        Ok(self.0.connect(&source.0, &target.0)?)
    }

    /// Remove whatever is connected to a port.
    pub fn disconnect(&self, port: &Port) -> Result<(), ShineError> {
        Ok(self.0.disconnect(&port.0)?)
    }

    /// Remove a part and everything connected to it.
    pub fn remove(&self, part: &Part) -> Result<(), ShineError> {
        Ok(self.0.remove(&part.0)?)
    }

    /// The names of its parts, in the order added.
    pub fn parts(&self) -> Vec<String> {
        self.0.parts().iter().map(|p| p.name()).collect()
    }

    /// A part by name ("Ring resonator", "Directional coupler 2", a pin's name…).
    pub fn find_part(&self, name: String) -> Result<Part, ShineError> {
        part(self.0.find_part(&name))
    }

    /// The design as text: its parts, their ports and how they connect.
    pub fn describe(&self) -> String {
        self.0.describe()
    }

    /// Simulate it: lasers on, electronics settled.
    pub fn simulate(&self) -> Result<Simulation, ShineError> {
        Ok(Simulation(self.0.simulate()?))
    }

    /// How light passes from each of its light inputs to each light output
    /// (its pins), at each wavelength, with its own lasers off.
    pub fn transmission(&self, wavelengths_nm: Vec<f64>) -> Result<Transmission, ShineError> {
        Ok(Transmission(self.0.transmission(&wavelengths_nm)?))
    }

    /// Sweep one parameter of one part from `start` to `stop` in `points`
    /// steps, simulating at each.
    pub fn sweep(&self, part: &Part, param: String, start: f64, stop: f64, #[boltffi::default(101)] points: u32) -> Result<Sweep, ShineError> {
        Ok(Sweep(self.0.sweep(&part.0, &param, start, stop, points)?))
    }

    /// Run it `runs` times with its parameters varied as fabrication would,
    /// from `seed` so it repeats.
    pub fn monte_carlo(
        &self,
        runs: u32,
        #[boltffi::default(1)] seed: u32,
        #[boltffi::default(None)] variation: Option<Variation>,
    ) -> Result<MonteCarlo, ShineError> {
        let v = variation.map_or_else(api::Variation::default, |v| api::Variation { n_eff: v.n_eff, coupling: v.coupling, phase: v.phase, loss: v.loss });
        Ok(MonteCarlo(self.0.monte_carlo(runs, seed as u64, &v)?))
    }

    /// Adjust the variables to bring `output`'s power (as readings name it)
    /// towards the goal: the most, the least, or `target_mw`. Nelder–Mead
    /// from the current values. Not in the browser.
    pub fn optimise(
        &self,
        variables: Vec<Variable>,
        output: String,
        goal: Goal,
        #[boltffi::default(0.0)] target_mw: f64,
        #[boltffi::default(300)] steps: u32,
    ) -> Result<Optimised, ShineError> {
        let vars = self.variables(&variables)?;
        let goal = match goal {
            Goal::Maximise => api::Goal::Maximise,
            Goal::Minimise => api::Goal::Minimise,
            Goal::Target => api::Goal::Target(target_mw),
        };
        let r = self.0.optimise(&vars, &output, goal, steps)?;
        Ok(Optimised { values: r.values, before_mw: r.before_mw, after_mw: r.after_mw, simulations: r.simulations })
    }

    /// Adjust the variables to bring several outputs towards their aims at
    /// once (weighted and summed), each as the design is or across a band of
    /// wavelengths: a filter's shape, or one output up and another down.
    /// Not in the browser.
    pub fn optimise_aims(&self, variables: Vec<Variable>, aims: Vec<Aim>, #[boltffi::default(300)] steps: u32) -> Result<Aimed, ShineError> {
        let vars = self.variables(&variables)?;
        let aims: Vec<api::Aim> = aims
            .iter()
            .map(|a| api::Aim {
                output: a.output.clone(),
                goal: match a.goal {
                    Goal::Maximise => api::Goal::Maximise,
                    Goal::Minimise => api::Goal::Minimise,
                    Goal::Target => api::Goal::Target(a.target_mw),
                },
                band: (a.band_points >= 2).then_some((a.band_from_nm, a.band_to_nm, a.band_points)),
                weight: a.weight,
            })
            .collect();
        let r = self.0.optimise_aims(&vars, &aims, steps)?;
        Ok(Aimed { values: r.values, before_mw: r.before_mw, after_mw: r.after_mw, simulations: r.simulations })
    }

    /// Set the variables to values (what an optimisation found).
    pub fn apply(&self, variables: Vec<Variable>, values: Vec<f64>) -> Result<(), ShineError> {
        let vars = self.variables(&variables)?;
        Ok(self.0.apply(&vars, &values)?)
    }

    /// Its S-parameters between its light pins, as a Touchstone file.
    pub fn to_touchstone(&self, wavelengths_nm: Vec<f64>) -> Result<String, ShineError> {
        Ok(self.0.to_touchstone(&wavelengths_nm)?)
    }

    /// Its circuit as a SAX netlist with models (Python).
    pub fn to_sax(&self, wavelengths_nm: Vec<f64>) -> Result<String, ShineError> {
        Ok(self.0.to_sax(&wavelengths_nm)?)
    }

    /// Its layout for gdsfactory (YAML), which draws the mask.
    pub fn to_gdsfactory(&self, #[boltffi::default(40.0)] spacing_um: f64) -> Result<String, ShineError> {
        Ok(self.0.to_gdsfactory(spacing_um)?)
    }

    /// Open the Shine desktop app on the project, showing this design (the
    /// app is `shine`, or the program in SHINE_APP). Not in the browser.
    pub fn show(&self) -> Result<(), ShineError> {
        Ok(self.0.show()?)
    }
}

impl Design {
    fn variables(&self, variables: &[Variable]) -> Result<Vec<(api::Part, String, f64, f64)>, ShineError> {
        variables.iter().map(|v| Ok((self.0.find_part(&v.part)?, v.param.clone(), v.min, v.max))).collect()
    }
}

// ---- Part and Port ----------------------------------------------------------------

/// One block placed in a design: built in (a coupler, a ring…) or a copy of
/// a component.
#[export]
pub struct Part(api::Part);

#[export]
impl Part {
    /// Its name in its design: "Ring resonator", "Ring resonator 2"…, or a
    /// pin's or component's name.
    pub fn name(&self) -> String {
        self.0.name()
    }

    /// What it is: the block's name ("Ring resonator") or the component's.
    pub fn kind(&self) -> String {
        self.0.kind()
    }

    /// Its ports: name, light or signal, in or out.
    pub fn ports(&self) -> Vec<PortInfo> {
        self.0.ports().into_iter().map(Into::into).collect()
    }

    /// A port by name (`in1`, `through`, `phase`…).
    pub fn port(&self, name: String) -> Result<Port, ShineError> {
        Ok(Port(self.0.port(&name)?))
    }

    /// Connect this part's first free light output to `next`'s first free
    /// light input, and return `next`: `laser.to(ring).to(detector)`.
    pub fn to(&self, next: &Part) -> Result<Part, ShineError> {
        part(self.0.to(&next.0))
    }

    /// Its parameters: name, unit, value and typical range.
    pub fn params(&self) -> Vec<Param> {
        self.0.params().into_iter().map(|p| Param { name: p.name, unit: p.unit, value: p.value, min: p.min, max: p.max }).collect()
    }

    /// A parameter's value.
    pub fn get(&self, param: String) -> Result<f64, ShineError> {
        Ok(self.0.get(&param)?)
    }

    /// Set a parameter; returns the part, so settings chain.
    pub fn set(&self, param: String, value: f64) -> Result<Part, ShineError> {
        part(self.0.set(&param, value))
    }

    /// Fix its position in the app (its top-left cell); parts left unplaced
    /// are laid out when the project is saved.
    pub fn place_at(&self, column: i32, row: i32) -> Part {
        Part(self.0.place_at(column, row))
    }
}

/// One of a part's ports, where a waveguide or a signal wire connects.
#[export]
pub struct Port(api::Port);

#[export]
impl Port {
    /// The port's name on its part.
    pub fn name(&self) -> String {
        self.0.name()
    }

    /// "Ring resonator.through".
    pub fn label(&self) -> String {
        self.0.label()
    }

    /// The part it belongs to.
    pub fn part(&self) -> Part {
        Part(self.0.part())
    }

    pub fn kind(&self) -> PortKind {
        match self.0.kind() {
            api::PortKind::Light => PortKind::Light,
            api::PortKind::Signal => PortKind::Signal,
        }
    }

    pub fn direction(&self) -> Direction {
        match self.0.direction() {
            api::Direction::In => Direction::In,
            api::Direction::Out => Direction::Out,
        }
    }
}

// ---- results ----------------------------------------------------------------------

/// One simulation of a design: the light everywhere, and the signals.
#[export]
pub struct Simulation(api::Simulation);

#[export]
impl Simulation {
    /// Whether the light and the electronics settled on one state.
    pub fn converged(&self) -> bool {
        self.0.converged()
    }

    /// The power arriving at a part (a detector, a light output…), in mW.
    pub fn power_mw(&self, part: &Part) -> Result<f64, ShineError> {
        Ok(self.0.power_mw(&part.0)?)
    }

    /// The power leaving a light output, in mW.
    pub fn output_power_mw(&self, port: &Port) -> Result<f64, ShineError> {
        Ok(self.0.output_power_mw(&port.0)?)
    }

    /// A signal output's value, or what arrives at a signal input.
    pub fn signal(&self, port: &Port) -> Result<f64, ShineError> {
        Ok(self.0.signal(&port.0)?)
    }

    /// The number a display shows.
    pub fn display(&self, part: &Part) -> Result<u32, ShineError> {
        Ok(self.0.display(&part.0)? as u32)
    }

    /// Every detector's and light output's power, and every part's light
    /// outputs ("Ring resonator.through"), in mW.
    pub fn readings(&self) -> Vec<Reading> {
        self.0.readings().into_iter().map(|r| Reading { name: r.name, power_mw: r.power_mw }).collect()
    }
}

/// How light passes between a design's light inputs and outputs at each wavelength.
#[export]
pub struct Transmission(api::Transmission);

fn complexes(v: Vec<api::Complex>) -> Vec<Complex> {
    v.into_iter().map(|z| Complex { re: z.re, im: z.im }).collect()
}

#[export]
impl Transmission {
    pub fn wavelengths_nm(&self) -> Vec<f64> {
        self.0.wavelengths_nm.clone()
    }

    /// The design's light inputs, in order.
    pub fn inputs(&self) -> Vec<String> {
        self.0.inputs.clone()
    }

    /// The design's light outputs, in order.
    pub fn outputs(&self) -> Vec<String> {
        self.0.outputs.clone()
    }

    /// The field leaving `output` per unit field entering `input`, at each wavelength.
    pub fn field(&self, output: String, input: String) -> Result<Vec<Complex>, ShineError> {
        Ok(complexes(self.0.field(&output, &input)?))
    }

    /// The fraction of power from `input` reaching `output`, at each wavelength.
    pub fn power(&self, output: String, input: String) -> Result<Vec<f64>, ShineError> {
        Ok(self.0.power(&output, &input)?)
    }

    /// The phase from `input` to `output`, unwrapped, in radians.
    pub fn phase(&self, output: String, input: String) -> Result<Vec<f64>, ShineError> {
        Ok(self.0.phase(&output, &input)?)
    }

    /// The group delay from `input` to `output`, in ps.
    pub fn group_delay_ps(&self, output: String, input: String) -> Result<Vec<f64>, ShineError> {
        Ok(self.0.group_delay_ps(&output, &input)?)
    }

    /// The whole matrix (rows the outputs, columns the inputs) at one of the wavelengths.
    pub fn matrix(&self, wavelength_index: u32) -> Result<Matrix, ShineError> {
        Ok(self.0.matrix(wavelength_index)?.into())
    }
}

/// A sweep: every reading at each value of one parameter.
#[export]
pub struct Sweep(api::Sweep);

#[export]
impl Sweep {
    /// The swept values.
    pub fn x(&self) -> Vec<f64> {
        self.0.x.clone()
    }

    /// What was swept, with its unit: "wavelength (nm)".
    pub fn x_label(&self) -> String {
        self.0.x_label.clone()
    }

    /// The names of everything measured.
    pub fn outputs(&self) -> Vec<String> {
        self.0.outputs()
    }

    /// One output's power at every swept value.
    pub fn series(&self, output: String) -> Result<Series, ShineError> {
        let s = self.0.series(&output)?;
        Ok(Series { name: s.name, power_mw: s.power_mw })
    }

    /// An output's resonances: where they are, their spacing (FSR), width
    /// and Q. For sweeps of a laser's wavelength.
    pub fn readouts(&self, output: String) -> Result<Readouts, ShineError> {
        let r = self.0.readouts(&output)?;
        Ok(Readouts { dips: r.dips, resonances_nm: r.resonances_nm, fsr_nm: r.fsr_nm, fwhm_nm: r.fwhm_nm, q: r.q, extinction_db: r.extinction_db })
    }

    /// The sweep as CSV: the swept value, then one column per output.
    pub fn to_csv(&self) -> String {
        self.0.to_csv()
    }
}

/// A design under fabrication variation: every reading over many runs.
#[export]
pub struct MonteCarlo(api::MonteCarlo);

#[export]
impl MonteCarlo {
    pub fn runs(&self) -> u32 {
        self.0.runs
    }

    pub fn outputs(&self) -> Vec<String> {
        self.0.outputs()
    }

    /// An output's power in every run, in mW.
    pub fn values(&self, output: String) -> Result<Vec<f64>, ShineError> {
        Ok(self.0.values(&output)?)
    }

    /// An output's mean, standard deviation and extremes.
    pub fn stats(&self, output: String) -> Result<Stats, ShineError> {
        let s = self.0.stats(&output)?;
        Ok(Stats { mean: s.mean, std: s.std, min: s.min, max: s.max })
    }

    /// The fraction of runs whose output is between `min_mw` and `max_mw`.
    pub fn yield_of(&self, output: String, min_mw: f64, max_mw: f64) -> Result<f64, ShineError> {
        Ok(self.0.yield_of(&output, min_mw, max_mw)?)
    }

    pub fn to_csv(&self) -> String {
        self.0.to_csv()
    }
}

/// W's largest singular value σmax: what a weight layer divides by.
#[export]
pub fn layer_scale(weights: Matrix) -> Result<f64, ShineError> {
    Ok(api::layer_scale(&weights.into())?)
}

/// Shine's version.
#[export]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
