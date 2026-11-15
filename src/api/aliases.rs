//! Aliases, how components simulate, and external programs, in the library.

use std::sync::Arc;

use super::design::Design;
use super::error::{Error, Result};
use super::part::Part;
use super::project::Project;
use crate::panels::circuit::alias;
use crate::panels::circuit::sim::Aliases;
use crate::panels::models::ModelSpec;
use crate::panels::optics::model::{BlockRef, Builtin, Item, SimulateAs};
use crate::panels::project::Alias;

/// The environment variable that lets the Python and TypeScript packages
/// run external programs: `1`, `true` or `yes`.
pub const ALLOW_PROCESSES_VARIABLE: &str = "SHINE_ALLOW_PROCESS_COMPONENTS";

/// Let external program parts run the programs they name, from now on in
/// this process. They run with your permissions: only for projects you
/// trust. Without this (or [`ALLOW_PROCESSES_VARIABLE`]), a design using
/// one fails to simulate.
pub fn allow_process_components() {
    #[cfg(not(target_arch = "wasm32"))]
    crate::panels::process::allow_all();
}

/// Whether the environment says programs may run, and if so, let them.
pub(crate) fn allow_processes_from_environment() {
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var(ALLOW_PROCESSES_VARIABLE).is_ok_and(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes")) {
        crate::panels::process::allow_all();
    }
}

/// What a component's alias is.
#[derive(Clone, Debug, PartialEq)]
pub struct AliasInfo {
    /// "truth table", "script" or "S-parameters".
    pub kind: String,
    /// Used when simulating components as set.
    pub on: bool,
    /// Made from the circuit rather than written.
    pub generated: bool,
    /// The largest difference from the circuit, when last checked.
    pub checked: Option<f64>,
}

impl Project {
    /// How components simulate: `"full"` (every one in full, aliases
    /// ignored) or `"set"` (the default: those whose alias is switched on, or
    /// whose part says so, by their alias).
    pub fn set_aliases(&self, mode: &str) -> Result<()> {
        let aliases = match mode {
            "full" => Aliases::Full,
            "set" => Aliases::AsSet,
            other => return Err(Error::Invalid(format!("aliases are \"full\" or \"set\", not \"{other}\""))),
        };
        super::project::lock(&self.inner).aliases = aliases;
        Ok(())
    }

    /// Simulate `parts` (component parts of one design) in full, and every
    /// other component with an alias by its alias: to watch them work among
    /// the rest.
    pub fn focus(&self, parts: &[Part]) -> Result<()> {
        let paths = parts
            .iter()
            .map(|p| match p.item {
                Item::Box(id) => Ok(vec![id]),
                Item::Part(_) => Err(Error::Invalid(format!("“{}” is a pin, not a component", p.name()))),
            })
            .collect::<Result<Vec<_>>>()?;
        super::project::lock(&self.inner).aliases = Aliases::Focus(paths);
        Ok(())
    }
}

impl Design {
    fn alias_mut<T>(&self, f: impl FnOnce(&mut Option<Alias>) -> T) -> Result<T> {
        self.with(|inner| {
            let doc = inner.project.doc_mut(self.id).ok_or_else(|| Error::Invalid("the design is gone".into()))?;
            let name = doc.name.clone();
            let meta = doc.component.as_mut().ok_or_else(|| Error::Invalid(format!("“{name}” is a design, not a component: only components have aliases")))?;
            let out = f(&mut meta.alias);
            doc.touch();
            Ok(out)
        })
    }

    /// Make this component's alias from its circuit: a truth table of its
    /// signal outputs for every combination of its signal inputs (logic: no
    /// light, no memory, up to 16 inputs). Exact for inputs of 0 and 1.
    /// Returns the largest difference from the circuit (0).
    pub fn make_alias(&self) -> Result<f64> {
        let made = self.with(|inner| alias::generate(&inner.project, self.id)).map_err(Error::Invalid)?;
        let checked = made.checked.unwrap_or(0.0);
        self.alias_mut(|a| {
            let on = a.as_ref().is_some_and(|a| a.on);
            *a = Some(Alias { on, ..made });
        })?;
        Ok(checked)
    }

    /// Use a JavaScript `code` (as for scripted components, with this
    /// component's ports) as its alias. Returns the largest difference from
    /// the circuit: keeping it right is up to you.
    pub fn set_alias_script(&self, code: &str) -> Result<f64> {
        let model = ModelSpec::from_script(code).map_err(Error::Invalid)?;
        let checked = self.with(|inner| alias::check(&inner.project, self.id, &model)).map_err(Error::Invalid)?;
        self.alias_mut(|a| {
            let on = a.as_ref().is_some_and(|a| a.on);
            *a = Some(Alias { model: Arc::new(model), on, generated: false, checked: Some(checked) });
        })?;
        Ok(checked)
    }

    /// Compare this component's alias with its circuit: the largest
    /// difference in any output.
    pub fn check_alias(&self) -> Result<f64> {
        let model = self.alias_mut(|a| a.as_ref().map(|a| a.model.clone()))?.ok_or_else(|| Error::Invalid(format!("“{}” has no alias", self.name())))?;
        let d = self.with(|inner| alias::check(&inner.project, self.id, &model)).map_err(Error::Invalid)?;
        self.alias_mut(|a| {
            if let Some(a) = a {
                a.checked = Some(d);
            }
        })?;
        Ok(d)
    }

    /// Switch this component's alias on (used when simulating as set) or off.
    pub fn use_alias(&self, on: bool) -> Result<()> {
        let had = self.alias_mut(|a| {
            if let Some(a) = a {
                a.on = on;
            }
            a.is_some()
        })?;
        if had { Ok(()) } else { Err(Error::Invalid(format!("“{}” has no alias: make one first", self.name()))) }
    }

    pub fn remove_alias(&self) -> Result<()> {
        self.alias_mut(|a| *a = None)
    }

    /// This component's alias, if it has one.
    pub fn alias(&self) -> Option<AliasInfo> {
        self.with(|inner| {
            let a = inner.project.doc(self.id)?.component.as_ref()?.alias.clone()?;
            let kind = match &a.model.source {
                crate::panels::models::ModelSource::TruthTable { .. } => "truth table",
                crate::panels::models::ModelSource::Script { .. } => "script",
                crate::panels::models::ModelSource::Table { .. } => "S-parameters",
            };
            Some(AliasInfo { kind: kind.into(), on: a.on, generated: a.generated, checked: a.checked })
        })
    }

    /// A part that asks a program on this computer for its outputs from its
    /// inputs (see the docs: Electronics, External programs). It runs only
    /// once allowed: [`allow_process_components`], or the environment
    /// variable `SHINE_ALLOW_PROCESS_COMPONENTS=1`. Light inputs give the
    /// program the power arriving (mW) after the signals; light outputs emit
    /// the amplitudes it answers after the signal outputs, at `wavelength_nm`.
    #[allow(clippy::too_many_arguments)]
    pub fn external_program(
        &self,
        name: &str,
        command: &str,
        args: Vec<String>,
        inputs: Vec<String>,
        outputs: Vec<String>,
        light_inputs: Vec<String>,
        light_outputs: Vec<String>,
        wavelength_nm: f64,
    ) -> Result<Part> {
        if command.trim().is_empty() {
            return Err(Error::Invalid("which program? (a command such as python3 or node)".into()));
        }
        if outputs.is_empty() && light_outputs.is_empty() {
            return Err(Error::Invalid("it needs at least one output".into()));
        }
        Ok(self.add(Builtin::Process { name: name.into(), command: command.into(), args, inputs, outputs, light_inputs, light_outputs, wavelength_nm }))
    }
}

impl Part {
    /// For a component part: `"set"` (as its component's alias switch says),
    /// `"full"` (its whole circuit) or `"alias"` (by its alias).
    pub fn set_simulate(&self, how: &str) -> Result<()> {
        let simulate = match how {
            "set" => SimulateAs::AsSet,
            "full" => SimulateAs::Full,
            "alias" => SimulateAs::Alias,
            other => return Err(Error::Invalid(format!("simulate is \"set\", \"full\" or \"alias\", not \"{other}\""))),
        };
        let Item::Box(id) = self.item else { return Err(Error::Invalid(format!("“{}” is a pin, not a component", self.name()))) };
        let mut inner = super::project::lock(&self.inner);
        let layout = inner.project.doc_mut(self.doc).ok_or_else(|| Error::Invalid("the design is gone".into()))?.layout_mut();
        let instance = layout.instances.iter_mut().find(|i| i.id == id).ok_or_else(|| Error::Invalid("the part is gone".into()))?;
        if !matches!(instance.block, BlockRef::Component(_)) {
            return Err(Error::Invalid("only component parts simulate by an alias".into()));
        }
        instance.simulate = simulate;
        Ok(())
    }
}
