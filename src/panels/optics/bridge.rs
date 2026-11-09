//! Converters between light and electrical signals.
//!
//! Light and signals never connect directly. When someone drops a wire from
//! one onto the other, the editor explains why and offers to insert the part
//! that converts: a detector (light → signal) or a laser switched by the
//! signal (signal → light).

use egui::{Pos2, RichText, Vec2};

use super::canvas::Hit;
use super::model::{Item, OpticComponentKind, PlacedComponent, PortRef, Terminal, box_geometry, cell_center};
use super::{BlockRef, Builtin, OpticsPanel};

/// A converter that would make a dropped connection work.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bridge {
    /// Light from a box output read by a detector whose signal drives `signal`.
    Detect { light: PortRef, signal: Terminal },
    /// A laser in front of a box input, switched by `signal`.
    Drive { signal: Terminal, light: PortRef },
}

/// An offer shown where the wire was dropped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offer {
    pub bridge: Bridge,
    pub screen: Pos2,
    opened_at: f64,
}

/// The converter needed between two ports, if they are light and signal.
pub fn needed(from: Hit, to: Hit) -> Option<Bridge> {
    match (from, to) {
        (Hit::LightOut(light), Hit::SignalIn(signal)) | (Hit::SignalIn(signal), Hit::LightOut(light)) => {
            Some(Bridge::Detect { light, signal })
        }
        (Hit::SignalOut(signal), Hit::LightIn(light)) | (Hit::LightIn(light), Hit::SignalOut(signal)) => {
            Some(Bridge::Drive { signal, light })
        }
        _ => None,
    }
}

/// Why two ports can't be wired, and what to use instead.
pub fn mismatch_hint(from: Hit, to: Hit) -> Option<&'static str> {
    let light = |h: Hit| matches!(h, Hit::LightIn(_) | Hit::LightOut(_));
    let signal = |h: Hit| matches!(h, Hit::SignalIn(_) | Hit::SignalOut(_));
    match needed(from, to) {
        Some(Bridge::Detect { .. }) => Some("Light can't drive a signal directly: it needs a detector"),
        Some(Bridge::Drive { .. }) => Some("A signal can't feed light directly: it needs a laser it switches"),
        None if (light(from) && signal(to)) || (signal(from) && light(to)) => {
            Some("Light and signals don't connect, and both ends are inputs or both outputs")
        }
        None => None,
    }
}

impl Bridge {
    pub fn title(self) -> &'static str {
        match self {
            Bridge::Detect { .. } => "Light → signal needs a detector",
            Bridge::Drive { .. } => "Signal → light needs a laser",
        }
    }

    pub fn explanation(self) -> &'static str {
        match self {
            Bridge::Detect { .. } => {
                "A detector absorbs the light and outputs its power as a signal; with “bit” on it outputs 1 above a threshold, else 0. Shine can place one in the beam leaving this port and wire it for you."
            }
            Bridge::Drive { .. } => {
                "A laser wired to a signal shines with amplitude × signal: 1 turns it on, 0 off. Shine can place one in front of this port and wire the signal to it."
            }
        }
    }

    pub fn action(self) -> &'static str {
        match self {
            Bridge::Detect { .. } => "Insert detector",
            Bridge::Drive { .. } => "Insert laser",
        }
    }
}

impl OpticsPanel {
    /// Show the converter offer for a dropped connection.
    pub(super) fn offer_bridge(&mut self, bridge: Bridge) {
        let screen = self.mouse_screen.unwrap_or_default();
        self.bridge = Some(Offer { bridge, screen, opened_at: self.time });
    }

    /// Insert the converter and wire it. Returns an error when there's no room.
    pub fn insert_bridge(&mut self, bridge: Bridge) -> Result<(), String> {
        let catalog = &self.catalog;
        let counts = |b: &BlockRef| catalog.counts(b);
        let geometry = |p: PortRef| {
            let inst = self.layout.instance(p.instance)?;
            Some((inst.cell, box_geometry(inst.cell, counts(&inst.block))))
        };
        match bridge {
            Bridge::Detect { light, signal } => {
                let (cell, g) = geometry(light).ok_or("that box no longer exists")?;
                let width = g.cells.iter().map(|c| c.0).max().unwrap_or(cell.0) - cell.0 + 1;
                let at = (cell.0 + width, cell.1 + light.port as i32);
                if self.layout.occupied(at, &counts) {
                    return Err(format!("no room right of the port (cell {}, {}); clear it or place a detector in the beam", at.0, at.1));
                }
                // Digital inputs get a 0/1 detector, everything else the power.
                let digital = matches!(
                    signal.item,
                    Item::Box(id) if matches!(
                        self.layout.instance(id).map(|i| &i.block),
                        Some(BlockRef::Builtin(Builtin::Display { .. } | Builtin::Register { .. }))
                    )
                );
                let threshold = if digital { 0.5 } else { 0.0 };
                self.layout.components.push(PlacedComponent {
                    kind: OpticComponentKind::Detector { threshold },
                    pos: cell_center(at.0, at.1),
                });
                let detector = self.layout.components.len() - 1;
                self.layout.connect_signal(Terminal { item: Item::Part(detector), port: 0 }, signal);
                self.editor.clear_selection();
                self.editor.selection.select_only(detector);
            }
            Bridge::Drive { signal, light } => {
                let (cell, _) = geometry(light).ok_or("that box no longer exists")?;
                let at = (cell.0 - 1, cell.1 + light.port as i32);
                if self.layout.occupied(at, &counts) {
                    return Err(format!("no room left of the port (cell {}, {}); clear it or place a laser aimed at the port", at.0, at.1));
                }
                self.layout.components.push(PlacedComponent {
                    kind: OpticComponentKind::Laser { amplitude: 1.0, phase: 0.0, wavelength: 1550.0, dir: Vec2::new(1.0, 0.0), source: 0, color: None },
                    pos: cell_center(at.0, at.1),
                });
                let laser = self.layout.components.len() - 1;
                // A waveguide into this port would take priority over the beam.
                self.layout.wires.retain(|w| w.to != light);
                self.layout.connect_signal(signal, Terminal { item: Item::Part(laser), port: 0 });
                self.editor.clear_selection();
                self.editor.selection.select_only(laser);
            }
        }
        Ok(())
    }

    /// Accept (insert the converter) or dismiss the open offer.
    pub fn apply_bridge_choice(&mut self, accept: bool) {
        let Some(offer) = self.bridge.take() else { return };
        if !accept {
            return;
        }
        match self.insert_bridge(offer.bridge) {
            Ok(()) => self.notify(format!("{}: done", offer.bridge.action())),
            Err(e) => self.notify(format!("Can't insert it: {e}")),
        }
    }

    /// The offer popup: what's wrong, why, and a button that fixes it.
    pub(super) fn bridge_ui(&mut self, ctx: &egui::Context) {
        let Some(offer) = self.bridge else { return };
        let mut choice = None;
        let shown = egui::Area::new(egui::Id::unique("bridge_offer"))
            .order(egui::Order::Foreground)
            .fixed_pos(offer.screen + Vec2::new(12.0, 12.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(300.0);
                    ui.label(RichText::new(format!("⚠ {}", offer.bridge.title())).strong().color(ui.visuals().warn_fg_color));
                    ui.label(RichText::new(offer.bridge.explanation()).small());
                    ui.horizontal(|ui| {
                        if ui.button(offer.bridge.action()).clicked() {
                            choice = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            choice = Some(false);
                        }
                    });
                });
            });
        match choice {
            Some(accept) => self.apply_bridge_choice(accept),
            None => {
                let pressed_outside = ctx.input(|i| i.pointer.any_pressed()) && !shown.response.contains_pointer();
                if offer.opened_at < self.time && (pressed_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape))) {
                    self.bridge = None;
                }
            }
        }
    }
}
