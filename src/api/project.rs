use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard};

use super::design::Design;
use super::error::{Error, Result};
use super::records::Matrix;
use crate::panels::models::{self, ModelSource, ModelSpec};
use crate::panels::optics::model::{Item, Layout};
use crate::panels::project::{DocId, Project as CoreProject};

/// What every handle (project, design, part, port) shares: the project, and
/// which parts were added without a position, to be laid out on saving.
pub(crate) struct Inner {
    pub project: CoreProject,
    pub unplaced: HashSet<(DocId, Item)>,
}

pub(crate) type Shared = Arc<Mutex<Inner>>;

pub(crate) fn lock(shared: &Shared) -> MutexGuard<'_, Inner> {
    // A panic in another thread mid-edit leaves the data as it was; carry on.
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// A Shine project: designs and the components they use, as one `.shine`
/// file. Open it in the app to look at it, or open a file made in the app.
///
/// Every `Project`, [`Design`], [`super::Part`] and [`super::Port`] is a
/// handle: cloning one is cheap and all clones see the same project.
#[derive(Clone)]
pub struct Project {
    pub(crate) inner: Shared,
}

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

/// A component's initials and description for a model made from data.
fn model_component(project: &mut CoreProject, model: ModelSpec) -> DocId {
    let name = project.unique_name(&model.name);
    let description = model.summary();
    let initials = crate::panels::names::initials(&name);
    project.add_component(&name, &initials, &description, models::component_layout(model), 10.0)
}

impl Project {
    /// An empty project.
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(Inner { project: CoreProject::new(), unplaced: HashSet::new() })) }
    }

    /// A project from a `.shine` file's contents.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let project = CoreProject::from_bytes(data).map_err(Error::File)?;
        Ok(Self { inner: Arc::new(Mutex::new(Inner { project, unplaced: HashSet::new() })) })
    }

    /// A `.shine` file.
    pub fn open(path: &str) -> Result<Self> {
        let data = std::fs::read(path).map_err(|e| Error::File(format!("can't read {path}: {e}")))?;
        Self::from_bytes(&data)
    }

    /// The project as a `.shine` file's contents. Parts added without a
    /// position are laid out first, left to right as light flows.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut inner = lock(&self.inner);
        super::arrange::arrange_all(&mut inner);
        inner.project.to_bytes()
    }

    /// Save as a `.shine` file, to open in the app.
    pub fn save(&self, path: &str) -> Result<()> {
        std::fs::write(path, self.to_bytes()).map_err(|e| Error::File(format!("can't write {path}: {e}")))
    }

    fn add(&self, name: &str, component: bool) -> Design {
        let mut inner = lock(&self.inner);
        let p = &mut inner.project;
        let name = p.unique_name(name);
        let id = if component {
            let initials = crate::panels::names::initials(&name);
            p.add_component(&name, &initials, "", Layout::default(), 10.0)
        } else {
            p.add_layout(&name, Layout::default(), 10.0)
        };
        Design { inner: self.inner.clone(), id }
    }

    /// A new, empty design (a name in use gets a number after it).
    pub fn new_design(&self, name: &str) -> Design {
        self.add(name, false)
    }

    /// A new, empty component: a design others can place as one part. Its
    /// light and signal pins are its ports, and the parameters it exposes
    /// are what each placed copy sets.
    pub fn new_component(&self, name: &str) -> Design {
        self.add(name, true)
    }

    /// A design or component by name.
    pub fn design(&self, name: &str) -> Result<Design> {
        let inner = lock(&self.inner);
        let p = &inner.project;
        match p.find(name) {
            Some(id) => Ok(Design { inner: self.inner.clone(), id }),
            None => Err(Error::NoSuchDesign { name: name.to_owned(), available: p.documents.iter().map(|d| d.name.clone()).collect() }),
        }
    }

    /// A weight matrix as light: a component with inputs in1…in{cols} and
    /// outputs out1…out{rows} that computes W x / σmax, where σmax is W's
    /// largest singular value (light can't be amplified): W = U Σ V†, the
    /// two unitaries as meshes of MZIs (the rectangular, Clements,
    /// arrangement) and Σ/σmax as an attenuator per mode. Any shape, real or
    /// complex. The meshes are components of their own, named after it; its
    /// description gives σmax ([`Design::description`]); `layer_scale` too.
    pub fn new_weight_layer(&self, name: &str, weights: &Matrix) -> Result<Design> {
        let m = checked(weights)?;
        let mut inner = lock(&self.inner);
        let layer = crate::panels::optics::layers::weight_layer(&mut inner.project, name, &m).map_err(Error::Invalid)?;
        Ok(Design { inner: self.inner.clone(), id: layer.id })
    }

    /// A unitary matrix as one mesh of MZIs: a component with inputs in1…
    /// and outputs out1… that computes U x.
    pub fn new_unitary_mesh(&self, name: &str, unitary: &Matrix) -> Result<Design> {
        let m = checked(unitary)?;
        let mut inner = lock(&self.inner);
        let id = crate::panels::optics::layers::unitary_mesh(&mut inner.project, name, &m).map_err(Error::Invalid)?;
        Ok(Design { inner: self.inner.clone(), id })
    }

    /// The names of every design and component, in order.
    pub fn designs(&self) -> Vec<String> {
        lock(&self.inner).project.documents.iter().map(|d| d.name.clone()).collect()
    }

    fn model(&self, model: std::result::Result<ModelSpec, String>) -> Result<Design> {
        let model = model.map_err(Error::Invalid)?;
        let id = model_component(&mut lock(&self.inner).project, model);
        Ok(Design { inner: self.inner.clone(), id })
    }

    /// A component known by its S-parameters, from a Touchstone file's
    /// contents (`.s2p`, `.s4p`…, versions 1 and 2).
    pub fn new_model_from_touchstone(&self, name: &str, data: &[u8]) -> Result<Design> {
        if cfg!(target_arch = "wasm32") {
            return Err(Error::Unsupported("Touchstone files are read in the desktop library".into()));
        }
        let file = if models::is_touchstone(name) { name.to_owned() } else { format!("{name}.s{}p", touchstone_ports(data)) };
        self.model(ModelSpec::from_touchstone(&file, data))
    }

    /// A component from a Lumerical INTERCONNECT S-parameter file's
    /// contents (`.dat` or `.sparam`, as foundry kits publish them).
    pub fn new_model_from_lumerical(&self, name: &str, text: &str) -> Result<Design> {
        self.model(ModelSpec::from_lumerical(name, text).map(|mut m| {
            m.name = name.to_owned();
            m
        }))
    }

    /// A component from its transmission at a set of wavelengths: one
    /// matrix per wavelength, rows the outputs and columns the inputs.
    /// Between wavelengths it is interpolated, as for imported files.
    pub fn new_model_from_table(
        &self,
        name: &str,
        inputs: Vec<String>,
        outputs: Vec<String>,
        wavelengths_nm: Vec<f64>,
        matrices: Vec<Matrix>,
    ) -> Result<Design> {
        if wavelengths_nm.is_empty() || wavelengths_nm.len() != matrices.len() {
            return Err(Error::Invalid(format!("{} wavelengths but {} matrices: one matrix per wavelength", wavelengths_nm.len(), matrices.len())));
        }
        if inputs.is_empty() || outputs.is_empty() {
            return Err(Error::Invalid("a model needs at least one input and one output".into()));
        }
        for (k, m) in matrices.iter().enumerate() {
            if m.rows as usize != outputs.len() || m.cols as usize != inputs.len() || m.values.len() != (m.rows * m.cols) as usize {
                return Err(Error::Invalid(format!(
                    "matrix {k} is {}×{}; it should be {}×{} (outputs × inputs)",
                    m.rows,
                    m.cols,
                    outputs.len(),
                    inputs.len()
                )));
            }
        }
        // Ascending wavelengths, as the model's interpolation needs.
        let mut points: Vec<(f64, &Matrix)> = wavelengths_nm.iter().copied().zip(&matrices).collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let source = ModelSource::Table {
            origin: "Shine library".into(),
            wavelengths_nm: points.iter().map(|p| p.0).collect(),
            values: points.iter().map(|p| p.1.values.iter().map(|z| [z.re, z.im]).collect()).collect(),
        };
        self.model(Ok(ModelSpec::new(name, inputs, outputs, source)))
    }

    /// A component whose transmission `model` computes: it is sampled at
    /// `wavelengths_nm` and kept as a table, so the component saves with the
    /// project and opens in the app like any other.
    pub fn new_model_from_function(
        &self,
        name: &str,
        inputs: Vec<String>,
        outputs: Vec<String>,
        wavelengths_nm: Vec<f64>,
        model: &dyn TransferFunction,
    ) -> Result<Design> {
        let matrices = wavelengths_nm.iter().map(|&wl| model.matrix(wl)).collect();
        self.new_model_from_table(name, inputs, outputs, wavelengths_nm, matrices)
    }

    /// A component whose light and signals a JavaScript program computes,
    /// as written in the app (Component → New scripted component).
    pub fn new_scripted_component(&self, code: &str) -> Result<Design> {
        if cfg!(target_arch = "wasm32") {
            return Err(Error::Unsupported("scripted components run in the desktop library".into()));
        }
        self.model(ModelSpec::from_script(code))
    }
}

/// The number of ports a Touchstone file's data describes, from its first
/// data line's column count (for files named without their `.sNp`).
fn touchstone_ports(data: &[u8]) -> usize {
    let text = String::from_utf8_lossy(data);
    let numbers = text
        .lines()
        .map(|l| l.split('!').next().unwrap_or("").trim())
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('['))
        .map_or(0, |l| l.split_whitespace().count());
    // A row is a frequency then N² pairs (for N ≤ 2 on one line).
    (1..=4).find(|n| 1 + 2 * n * n == numbers).unwrap_or(2)
}

/// A component's transmission written in code: the matrix (rows the
/// outputs, columns the inputs) at a wavelength in nm.
pub trait TransferFunction {
    fn matrix(&self, wavelength_nm: f64) -> Matrix;
}

impl<F: Fn(f64) -> Matrix> TransferFunction for F {
    fn matrix(&self, wavelength_nm: f64) -> Matrix {
        self(wavelength_nm)
    }
}

/// W's largest singular value σmax: what a weight layer divides by.
pub fn layer_scale(weights: &Matrix) -> Result<f64> {
    Ok(crate::panels::optics::layers::svd(&checked(weights)?).map_err(Error::Invalid)?.scale)
}

/// A matrix whose values fill its rows and columns, as the core's.
fn checked(m: &Matrix) -> Result<crate::photonics::Matrix> {
    if m.rows == 0 || m.cols == 0 || m.values.len() != (m.rows * m.cols) as usize {
        return Err(Error::Invalid(format!("a {}×{} matrix needs {} values; it has {}", m.rows, m.cols, m.rows * m.cols, m.values.len())));
    }
    Ok(m.to_core())
}

impl std::fmt::Debug for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Project({})", self.designs().join(", "))
    }
}
