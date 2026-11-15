//! A component's alias, in the right panel when the component is open:
//! generate one, write one as a script, check it, switch it on
//! (design/aliases.md).

use std::sync::Arc;

use egui::{RichText, Ui};

use super::Workspace;
use crate::panels::circuit::alias;
use crate::panels::models::ModelSpec;
use crate::panels::project::{Alias, DocId, Interface};
use crate::panels::ui_kit;

/// A script with the component's ports, to start an alias from.
pub fn alias_template(name: &str, iface: &Interface) -> String {
    let list = |names: &[String]| names.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
    let mut s = format!(
        "// An alias for “{name}”: a compact model the simulator can use instead of\n\
         // its circuit. It must have the component's ports, in the same order.\n\
         const component = {{\n  name: {:?},\n  light: {{ inputs: [{}], outputs: [{}] }},\n  signals: {{ inputs: [{}], outputs: [{}] }},\n",
        format!("{name} (alias)"),
        list(&iface.inputs),
        list(&iface.outputs),
        list(&iface.signal_inputs),
        list(&iface.signal_outputs),
    );
    if !iface.inputs.is_empty() && !iface.outputs.is_empty() {
        let row = format!("[{}]", vec!["0"; iface.inputs.len()].join(", "));
        let rows = vec![row; iface.outputs.len()].join(", ");
        s += &format!(
            "\n  // The field from each light input to each output at a wavelength in nm:\n  \
             // one row per output, each entry a number or [re, im].\n  \
             transmission(wavelength, signals) {{\n    return [{rows}];\n  }},\n"
        );
    }
    if !iface.signal_outputs.is_empty() {
        let outs = iface.signal_outputs.iter().map(|n| format!("{n:?}: 0")).collect::<Vec<_>>().join(", ");
        s += &format!("\n  // The signal outputs, by name.\n  outputs(signals) {{\n    return {{ {outs} }};\n  }},\n");
    }
    s + "};\n"
}

impl Workspace {
    fn set_alias(&mut self, id: DocId, f: impl FnOnce(&mut Option<Alias>)) {
        if let Some(doc) = self.project.doc_mut(id)
            && let Some(meta) = doc.component.as_mut()
        {
            f(&mut meta.alias);
            doc.touch();
        }
    }

    /// Make an alias of component `id` from its circuit (a truth table).
    pub fn generate_alias(&mut self, id: DocId) {
        match alias::generate(&self.project, id) {
            Ok(mut made) => {
                let rows = made.model.summary();
                self.set_alias(id, |a| {
                    made.on = a.as_ref().is_some_and(|a| a.on);
                    *a = Some(made);
                });
                self.notify(format!("Alias made: {rows}, the same as the circuit"));
            }
            Err(e) => self.notify(format!("Can't make an alias: {e}")),
        }
    }

    /// Compare component `id`'s alias with its circuit.
    pub fn check_alias(&mut self, id: DocId) {
        let Some(model) = self.project.doc(id).and_then(|d| d.component.as_ref()?.alias.as_ref()).map(|a| a.model.clone()) else { return };
        match alias::check(&self.project, id, &model) {
            Ok(d) => {
                self.set_alias(id, |a| {
                    if let Some(a) = a {
                        a.checked = Some(d);
                    }
                });
                self.notify(if d < 1e-9 { "The alias gives what the circuit does".to_owned() } else { format!("The alias differs from the circuit by up to {d:.3}") });
            }
            Err(e) => self.notify(format!("Can't check the alias: {e}")),
        }
    }

    /// Use `code` as component `id`'s alias, checked against its circuit.
    fn apply_alias_script(&mut self, id: DocId, code: &str) -> Result<(), String> {
        let model = ModelSpec::from_script(code)?;
        let checked = alias::check(&self.project, id, &model)?;
        self.set_alias(id, |a| {
            let on = a.as_ref().is_some_and(|a| a.on);
            *a = Some(Alias { model: Arc::new(model), on, generated: false, checked: Some(checked) });
        });
        self.notify(if checked < 1e-9 { "Alias applied: it gives what the circuit does".to_owned() } else { format!("Alias applied; it differs from the circuit by up to {checked:.3}") });
        Ok(())
    }

    /// The Alias section, for component `id`.
    pub(super) fn alias_ui(&mut self, ui: &mut Ui, id: DocId) {
        let Some(doc) = self.project.doc(id) else { return };
        let (name, iface) = (doc.name.clone(), doc.interface());
        let current = doc.component.as_ref().and_then(|m| m.alias.clone());
        ui_kit::section(ui, "Alias");
        match &current {
            None => {
                ui.label(
                    RichText::new(
                        "A shortcut for simulating: a compact model with this component's ports that a design can simulate instead of its circuit. \
                         Make one from the circuit (logic), or write one.",
                    )
                    .small()
                    .weak(),
                );
            }
            Some(a) => {
                let made = if a.generated { "Made from the circuit" } else { "Written by hand" };
                ui.label(format!("{made}: {}", a.model.summary()));
                let checked = match a.checked {
                    Some(d) if d < 1e-9 => "Checked: gives what the circuit does".to_owned(),
                    Some(d) => format!("Checked: differs from the circuit by up to {d:.3}"),
                    None => "Not checked against the circuit".to_owned(),
                };
                ui.label(RichText::new(checked).small().weak());
                let mut on = a.on;
                if ui
                    .checkbox(&mut on, "Use it when simulating")
                    .on_hover_text("Designs simulating components as set use the alias for this one; Focus and each box can choose too")
                    .changed()
                {
                    self.set_alias(id, |a| {
                        if let Some(a) = a {
                            a.on = on;
                        }
                    });
                }
            }
        }
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .small_button("Make from the circuit")
                .on_hover_text("A truth table of its signal outputs for every combination of its signal inputs: for logic (no light, no memory)")
                .clicked()
            {
                action = Some("generate");
            }
            if ui.small_button("Write a script…").on_hover_text("An alias in JavaScript: anything, light and signals (desktop app)").clicked() {
                action = Some("script");
            }
            if current.is_some() {
                if ui.small_button("Check").on_hover_text("Compare it with the circuit: the largest difference in any output").clicked() {
                    action = Some("check");
                }
                if ui.small_button("Remove").clicked() {
                    action = Some("remove");
                }
            }
        });
        match action {
            Some("generate") => self.generate_alias(id),
            Some("check") => self.check_alias(id),
            Some("remove") => self.set_alias(id, |a| *a = None),
            Some("script") => {
                let code = current.as_ref().and_then(|a| a.model.script().map(str::to_owned)).unwrap_or_else(|| alias_template(&name, &iface));
                self.alias_script = Some((id, code, None));
            }
            _ => {}
        }
        self.alias_script_ui(ui, id);
    }

    /// The script being written for component `id`'s alias, if any.
    fn alias_script_ui(&mut self, ui: &mut Ui, id: DocId) {
        let Some((doc, code, error)) = &mut self.alias_script else { return };
        if *doc != id {
            return;
        }
        ui.add(egui::TextEdit::multiline(code).code_editor().desired_rows(12).desired_width(f32::INFINITY))
            .accessible_name("Alias script");
        if let Some(e) = error {
            ui.colored_label(ui.visuals().error_fg_color, e.as_str());
        }
        let (mut apply, mut cancel) = (false, false);
        ui.horizontal(|ui| {
            apply = ui.button("Apply").on_hover_text("Use it as the alias, checked against the circuit").clicked();
            cancel = ui.button("Cancel").clicked();
        });
        if cancel {
            self.alias_script = None;
        } else if apply {
            let code = code.clone();
            match self.apply_alias_script(id, &code) {
                Ok(()) => self.alias_script = None,
                Err(e) => {
                    if let Some(s) = &mut self.alias_script {
                        s.2 = Some(e);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_has_the_components_ports() {
        let iface = Interface {
            inputs: vec!["in".into()],
            outputs: vec!["a".into(), "b".into()],
            signal_inputs: vec!["v".into()],
            signal_outputs: vec!["q".into()],
        };
        let spec = ModelSpec::from_script(&alias_template("Switch", &iface)).unwrap();
        assert_eq!((spec.inputs.len(), spec.outputs.len(), spec.signal_inputs, spec.signal_outputs), (1, 2, vec!["v".to_owned()], vec!["q".to_owned()]));
    }
}
