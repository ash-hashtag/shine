//! Blender-style keymap.

use egui::{Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    SelectAll,
    DeselectAll,
    InvertSelection,
    Delete,
    Duplicate,
    Grab,
    RotateCw,
    RotateCcw,
    Undo,
    Redo,
    FrameAll,
    FrameSelected,
    ToggleSimulation,
    ToggleToolbar,
    ToggleProperties,
    AddMenu,
    Save,
    Open,
    New,
    ConstrainX,
    ConstrainY,
    Confirm,
    Cancel,
    ShowShortcuts,
    /// Open the selected component's definition.
    Inspect,
    /// Arrow keys: move the selection (or the grab) by whole cells, or pan
    /// the view when nothing is selected.
    Nudge(i32, i32),
    ZoomIn,
    ZoomOut,
    /// Select the next (or previous) part or box, to select without a mouse.
    SelectNext,
    SelectPrevious,
    /// Wire from the selected item; again (or Enter) wires to the one
    /// selected then.
    Wire,
}

/// The cells an arrow key moves by: one, or five with Shift.
fn arrow(key: Key, m: Modifiers) -> Option<(i32, i32)> {
    let step = if m.shift { 5 } else { 1 };
    match key {
        Key::ArrowLeft => Some((-step, 0)),
        Key::ArrowRight => Some((step, 0)),
        Key::ArrowUp => Some((0, -step)),
        Key::ArrowDown => Some((0, step)),
        _ => None,
    }
}

/// Map a key press to an action. While grabbing, only the modal keys apply.
pub fn shortcut(key: Key, m: Modifiers, grabbing: bool) -> Option<Action> {
    use Action::*;
    if let Some((dx, dy)) = arrow(key, m).filter(|_| !m.command && !m.alt) {
        return Some(Nudge(dx, dy));
    }
    if grabbing {
        return match key {
            Key::X => Some(ConstrainX),
            Key::Y => Some(ConstrainY),
            Key::Enter | Key::Space => Some(Confirm),
            Key::Escape => Some(Cancel),
            _ => None,
        };
    }
    let plain = !m.command && !m.shift && !m.alt;
    Some(match key {
        Key::Z if m.command && m.shift => Redo,
        Key::Z if m.command => Undo,
        Key::Y if m.command => Redo,
        Key::S if m.command => Save,
        Key::O if m.command => Open,
        Key::N if m.command => New,
        Key::I if m.command => InvertSelection,
        // Ctrl+A (Cmd+A) too, as in every other program.
        Key::A if m.command && !m.shift && !m.alt => SelectAll,
        Key::A if m.alt => DeselectAll,
        Key::A if m.shift => AddMenu,
        Key::A if plain => SelectAll,
        Key::D if m.shift => Duplicate,
        Key::R if m.shift => RotateCcw,
        Key::R if plain => RotateCw,
        Key::G if plain => Grab,
        Key::X | Key::Delete if plain => Delete,
        Key::Home => FrameAll,
        Key::Period if plain => FrameSelected,
        Key::Space if plain => ToggleSimulation,
        Key::N if plain => ToggleProperties,
        Key::T if plain => ToggleToolbar,
        Key::Tab if plain => Inspect,
        Key::Plus | Key::Equals if !m.command => ZoomIn,
        Key::Minus if !m.command => ZoomOut,
        Key::CloseBracket if plain => SelectNext,
        Key::W if plain => Wire,
        Key::Enter if plain => Confirm,
        Key::OpenBracket if plain => SelectPrevious,
        Key::F1 => ShowShortcuts,
        Key::Questionmark => ShowShortcuts,
        Key::Escape => Cancel,
        _ => return None,
    })
}

/// Shown in the shortcuts window: `(keys, what they do)`.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Middle drag / Shift+Middle drag", "Pan"),
    ("Ctrl+Middle drag", "Zoom"),
    ("Wheel / pinch", "Zoom at cursor"),
    ("Trackpad scroll", "Pan"),
    ("Home", "Frame all"),
    (". (period)", "Frame selected"),
    ("Left click", "Select"),
    ("Shift+Left click", "Extend / toggle selection"),
    ("Left drag on empty space", "Box select (Shift extends)"),
    ("Left drag on component", "Move it"),
    ("Double click", "Rotate a part / open a component"),
    ("Click a number's bit", "Toggle that bit"),
    ("A or Ctrl+A / Alt+A / Ctrl+I", "Select all / none / invert"),
    ("G", "Grab (move); X / Y constrain axis"),
    ("Enter / Left click", "Confirm grab"),
    ("Esc / Right click", "Cancel grab"),
    ("R / Shift+R", "Rotate clockwise / counter-clockwise"),
    ("Shift+D", "Duplicate and move"),
    ("Arrow keys / Shift+arrows", "Move the selection (or a grab) 1 / 5 cells; with nothing selected, pan"),
    ("+ / −", "Zoom in / out"),
    ("] / [", "Select the next / previous part or box"),
    ("W, then ] / [ and Enter", "Wire the selected item to another, first free ports"),
    ("X / Delete", "Delete selected"),
    ("Shift+A / Right click", "Add menu / context menu"),
    (
        "Tab / Double click",
        "Open the selected component's definition in its own tab (otherwise Tab moves to the next control)",
    ),
    (
        "Drag from a port or terminal",
        "Wire it: light ports take waveguides, square terminals take signals",
    ),
    ("Ctrl+Z / Ctrl+Shift+Z", "Undo / redo"),
    ("Space", "Toggle simulation"),
    ("T / N", "Toggle toolbar / properties"),
    ("Ctrl+N / Ctrl+O / Ctrl+S", "New / open / save"),
    ("F1 / ?", "This help"),
];

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;

    fn ctrl() -> Modifiers {
        Modifiers {
            command: true,
            ctrl: true,
            ..NONE
        }
    }

    #[test]
    fn plain_keys() {
        let cases = [
            (Key::A, Action::SelectAll),
            (Key::G, Action::Grab),
            (Key::R, Action::RotateCw),
            (Key::X, Action::Delete),
            (Key::Delete, Action::Delete),
            (Key::Home, Action::FrameAll),
            (Key::Period, Action::FrameSelected),
            (Key::Space, Action::ToggleSimulation),
            (Key::N, Action::ToggleProperties),
            (Key::T, Action::ToggleToolbar),
            (Key::F1, Action::ShowShortcuts),
            (Key::Questionmark, Action::ShowShortcuts),
            (Key::Escape, Action::Cancel),
            (Key::Tab, Action::Inspect),
            (Key::ArrowLeft, Action::Nudge(-1, 0)),
            (Key::ArrowDown, Action::Nudge(0, 1)),
            (Key::Plus, Action::ZoomIn),
            (Key::Minus, Action::ZoomOut),
            (Key::CloseBracket, Action::SelectNext),
            (Key::OpenBracket, Action::SelectPrevious),
            (Key::W, Action::Wire),
            (Key::Enter, Action::Confirm),
        ];
        for (key, action) in cases {
            assert_eq!(shortcut(key, NONE, false), Some(action), "{key:?}");
        }
        assert_eq!(shortcut(Key::Q, NONE, false), None);
    }

    #[test]
    fn modified_keys() {
        assert_eq!(shortcut(Key::Z, ctrl(), false), Some(Action::Undo));
        assert_eq!(shortcut(Key::A, ctrl(), false), Some(Action::SelectAll));
        assert_eq!(
            shortcut(
                Key::Z,
                Modifiers {
                    shift: true,
                    ..ctrl()
                },
                false
            ),
            Some(Action::Redo)
        );
        assert_eq!(shortcut(Key::Y, ctrl(), false), Some(Action::Redo));
        assert_eq!(shortcut(Key::S, ctrl(), false), Some(Action::Save));
        assert_eq!(shortcut(Key::O, ctrl(), false), Some(Action::Open));
        assert_eq!(shortcut(Key::N, ctrl(), false), Some(Action::New));
        assert_eq!(
            shortcut(Key::I, ctrl(), false),
            Some(Action::InvertSelection)
        );
        assert_eq!(
            shortcut(Key::A, Modifiers::ALT, false),
            Some(Action::DeselectAll)
        );
        assert_eq!(
            shortcut(Key::A, Modifiers::SHIFT, false),
            Some(Action::AddMenu)
        );
        assert_eq!(
            shortcut(Key::D, Modifiers::SHIFT, false),
            Some(Action::Duplicate)
        );
        assert_eq!(
            shortcut(Key::R, Modifiers::SHIFT, false),
            Some(Action::RotateCcw)
        );
        // Modifiers must not leak into plain bindings.
        assert_eq!(shortcut(Key::G, ctrl(), false), None);
        assert_eq!(shortcut(Key::X, Modifiers::SHIFT, false), None);
        assert_eq!(shortcut(Key::D, NONE, false), None);
    }

    #[test]
    fn grab_mode_is_modal() {
        assert_eq!(shortcut(Key::X, NONE, true), Some(Action::ConstrainX));
        assert_eq!(shortcut(Key::Y, NONE, true), Some(Action::ConstrainY));
        assert_eq!(shortcut(Key::Enter, NONE, true), Some(Action::Confirm));
        assert_eq!(shortcut(Key::Space, NONE, true), Some(Action::Confirm));
        assert_eq!(shortcut(Key::Escape, NONE, true), Some(Action::Cancel));
        assert_eq!(shortcut(Key::A, NONE, true), None);
        assert_eq!(shortcut(Key::Z, ctrl(), true), None);
    }

    #[test]
    fn help_lists_every_binding_family() {
        let text: String = SHORTCUTS
            .iter()
            .map(|(k, d)| format!("{k} {d}\n"))
            .collect();
        for needle in [
            "Pan",
            "Zoom",
            "Grab",
            "Duplicate",
            "Undo",
            "Box select",
            "Frame",
        ] {
            assert!(text.contains(needle), "{needle}");
        }
    }
}
