//! Keyboard binding table — single source of truth for key dispatch AND help.
//!
//! The `BINDINGS` slice is consulted in order by `resolve_key` (first match
//! wins) and iterated by the help overlay (all rows shown). Each row carries
//! both its runtime predicate (`matches`) and its display metadata
//! (`keys`, `sep`, `desc`), so the help popup can never drift from the
//! actual bindings.

use winit::keyboard::{Key, ModifiersState, NamedKey};

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    NavigateNext,
    NavigatePrev,
    Pan(Dir),
    ResetZoom,
    ZoomIn,
    ZoomOut,
    ToggleMode,
    RotateCw,
    RotateCcw,
    ToggleInfo,
    RequestDelete,
    ShowHelp,
    OpenSettings,
    Quit,
}

#[derive(Clone, Copy)]
pub struct InputContext {
    pub is_zoomed:  bool,
    pub modifiers:  ModifiersState,
    pub has_images: bool,
}

#[derive(Clone, Copy)]
pub enum KeySep {
    /// Rendered as "/" between alternatives (e.g. `R / ]`).
    Or,
    /// Rendered as "+" between combo parts (e.g. `Ctrl/⌘ + ,`).
    Plus,
}

pub struct Binding {
    pub keys:    &'static [&'static str],
    pub sep:     KeySep,
    pub desc:    &'static str,
    pub matches: fn(&Key, InputContext) -> bool,
    pub action:  Action,
}

// ── Predicate helpers ────────────────────────────────────────────────────────

fn is_char(k: &Key, s: &str) -> bool {
    matches!(k, Key::Character(c) if c.as_str() == s)
}

fn is_char_ci(k: &Key, s: &str) -> bool {
    matches!(k, Key::Character(c) if c.as_str().eq_ignore_ascii_case(s))
}

fn has_cmd_or_ctrl(mods: ModifiersState) -> bool {
    mods.control_key() || mods.super_key()
}

// ── The table ────────────────────────────────────────────────────────────────
//
// Order matters — first match wins. More specific predicates MUST come before
// their general counterparts:
//
//   1. `is_zoomed`-gated arrows and Space
//   2. `Shift + Space`
//   3. `Ctrl/Cmd + ,` before plain `,`
//   4. `Ctrl/Cmd + W` before plain `W` (no plain W today, but the rule stands)
//   5. Everything else

pub const BINDINGS: &[Binding] = &[
    // ── Arrows (zoomed → pan) ────────────────────────────────────────────────
    Binding {
        keys: &["Right"], sep: KeySep::Or,
        desc: "Pan left (when zoomed)",
        matches: |k, c| c.is_zoomed
            && matches!(k, Key::Named(NamedKey::ArrowRight)),
        action: Action::Pan(Dir::Left),
    },
    Binding {
        keys: &["Left"], sep: KeySep::Or,
        desc: "Pan right (when zoomed)",
        matches: |k, c| c.is_zoomed
            && matches!(k, Key::Named(NamedKey::ArrowLeft)),
        action: Action::Pan(Dir::Right),
    },
    Binding {
        keys: &["Down"], sep: KeySep::Or,
        desc: "Pan up (when zoomed)",
        matches: |k, c| c.is_zoomed
            && matches!(k, Key::Named(NamedKey::ArrowDown)),
        action: Action::Pan(Dir::Up),
    },
    Binding {
        keys: &["Up"], sep: KeySep::Or,
        desc: "Pan down (when zoomed)",
        matches: |k, c| c.is_zoomed
            && matches!(k, Key::Named(NamedKey::ArrowUp)),
        action: Action::Pan(Dir::Down),
    },

    // ── Arrows (not zoomed → navigate) ───────────────────────────────────────
    Binding {
        keys: &["Right", "Down"], sep: KeySep::Or,
        desc: "Next image",
        matches: |k, _| matches!(k,
            Key::Named(NamedKey::ArrowRight | NamedKey::ArrowDown)),
        action: Action::NavigateNext,
    },
    Binding {
        keys: &["Left", "Up"], sep: KeySep::Or,
        desc: "Previous image",
        matches: |k, _| matches!(k,
            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowUp)),
        action: Action::NavigatePrev,
    },

    // ── Space variants ───────────────────────────────────────────────────────
    Binding {
        keys: &["Space"], sep: KeySep::Or,
        desc: "Reset zoom (when zoomed)",
        matches: |k, c| c.is_zoomed
            && matches!(k, Key::Named(NamedKey::Space)),
        action: Action::ResetZoom,
    },
    Binding {
        keys: &["Shift", "Space"], sep: KeySep::Plus,
        desc: "Previous image",
        matches: |k, c| c.modifiers.shift_key()
            && matches!(k, Key::Named(NamedKey::Space)),
        action: Action::NavigatePrev,
    },
    Binding {
        keys: &["Space"], sep: KeySep::Or,
        desc: "Next image",
        matches: |k, _| matches!(k, Key::Named(NamedKey::Space)),
        action: Action::NavigateNext,
    },

    // ── Navigation punctuation ───────────────────────────────────────────────
    Binding {
        keys: &["."], sep: KeySep::Or, desc: "Next image",
        matches: |k, _| is_char(k, "."),
        action: Action::NavigateNext,
    },
    Binding {
        keys: &["Ctrl/\u{2318}", ","], sep: KeySep::Plus,
        desc: "Open settings",
        matches: |k, c| has_cmd_or_ctrl(c.modifiers) && is_char(k, ","),
        action: Action::OpenSettings,
    },
    Binding {
        keys: &[","], sep: KeySep::Or, desc: "Previous image",
        matches: |k, _| is_char(k, ","),
        action: Action::NavigatePrev,
    },

    // ── Zoom ─────────────────────────────────────────────────────────────────
    Binding {
        keys: &["+", "="], sep: KeySep::Or, desc: "Zoom in",
        matches: |k, _| is_char(k, "+") || is_char(k, "="),
        action: Action::ZoomIn,
    },
    Binding {
        keys: &["-"], sep: KeySep::Or, desc: "Zoom out / reset",
        matches: |k, _| is_char(k, "-"),
        action: Action::ZoomOut,
    },

    // ── Display ──────────────────────────────────────────────────────────────
    Binding {
        keys: &["M"], sep: KeySep::Or, desc: "Cycle display mode",
        matches: |k, _| is_char_ci(k, "m"),
        action: Action::ToggleMode,
    },
    Binding {
        keys: &["R", "]"], sep: KeySep::Or,
        desc: "Rotate 90\u{00b0} clockwise",
        matches: |k, _| is_char_ci(k, "r") || is_char(k, "]"),
        action: Action::RotateCw,
    },
    Binding {
        keys: &["L", "["], sep: KeySep::Or,
        desc: "Rotate 90\u{00b0} counter-clockwise",
        matches: |k, _| is_char_ci(k, "l") || is_char(k, "["),
        action: Action::RotateCcw,
    },
    Binding {
        keys: &["I"], sep: KeySep::Or, desc: "Toggle image info panel",
        matches: |k, _| is_char_ci(k, "i"),
        action: Action::ToggleInfo,
    },

    // ── Destructive / help / windowing ───────────────────────────────────────
    Binding {
        keys: &["D"], sep: KeySep::Or, desc: "Delete current image",
        matches: |k, c| c.has_images && is_char_ci(k, "d"),
        action: Action::RequestDelete,
    },
    Binding {
        keys: &["H"], sep: KeySep::Or, desc: "Show this help",
        matches: |k, _| is_char_ci(k, "h"),
        action: Action::ShowHelp,
    },
    Binding {
        keys: &["Ctrl/\u{2318}", "W"], sep: KeySep::Plus,
        desc: "Close window",
        matches: |k, c| has_cmd_or_ctrl(c.modifiers) && is_char_ci(k, "w"),
        action: Action::Quit,
    },
    Binding {
        keys: &["Q", "Esc"], sep: KeySep::Or, desc: "Quit",
        matches: |k, _| is_char_ci(k, "q")
            || matches!(k, Key::Named(NamedKey::Escape)),
        action: Action::Quit,
    },
];

// ── Resolver ─────────────────────────────────────────────────────────────────

pub fn resolve_key(key: &Key, ctx: InputContext) -> Option<Action> {
    BINDINGS.iter()
        .find(|b| (b.matches)(key, ctx))
        .map(|b| b.action)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn ctx(is_zoomed: bool, has_images: bool, mods: ModifiersState) -> InputContext {
        InputContext { is_zoomed, has_images, modifiers: mods }
    }

    fn chr(s: &str) -> Key {
        Key::Character(SmolStr::new(s))
    }

    #[test]
    fn arrow_navigates_when_not_zoomed() {
        let c = ctx(false, true, ModifiersState::empty());
        assert_eq!(resolve_key(&Key::Named(NamedKey::ArrowRight), c),
                   Some(Action::NavigateNext));
        assert_eq!(resolve_key(&Key::Named(NamedKey::ArrowLeft), c),
                   Some(Action::NavigatePrev));
    }

    #[test]
    fn arrow_pans_when_zoomed() {
        let c = ctx(true, true, ModifiersState::empty());
        assert_eq!(resolve_key(&Key::Named(NamedKey::ArrowRight), c),
                   Some(Action::Pan(Dir::Left)));
        assert_eq!(resolve_key(&Key::Named(NamedKey::ArrowDown), c),
                   Some(Action::Pan(Dir::Up)));
    }

    #[test]
    fn space_variants() {
        let plain   = ctx(false, true, ModifiersState::empty());
        let shift   = ctx(false, true, ModifiersState::SHIFT);
        let zoomed  = ctx(true,  true, ModifiersState::empty());
        assert_eq!(resolve_key(&Key::Named(NamedKey::Space), plain),
                   Some(Action::NavigateNext));
        assert_eq!(resolve_key(&Key::Named(NamedKey::Space), shift),
                   Some(Action::NavigatePrev));
        assert_eq!(resolve_key(&Key::Named(NamedKey::Space), zoomed),
                   Some(Action::ResetZoom));
    }

    #[test]
    fn ctrl_comma_is_settings() {
        let ctrl = ctx(false, true, ModifiersState::CONTROL);
        let none = ctx(false, true, ModifiersState::empty());
        assert_eq!(resolve_key(&chr(","), ctrl), Some(Action::OpenSettings));
        assert_eq!(resolve_key(&chr(","), none), Some(Action::NavigatePrev));
    }

    #[test]
    fn delete_requires_images() {
        let with    = ctx(false, true,  ModifiersState::empty());
        let without = ctx(false, false, ModifiersState::empty());
        assert_eq!(resolve_key(&chr("d"), with), Some(Action::RequestDelete));
        assert_eq!(resolve_key(&chr("d"), without), None);
    }

    #[test]
    fn cmd_w_quits() {
        let cmd  = ctx(false, true, ModifiersState::SUPER);
        let ctrl = ctx(false, true, ModifiersState::CONTROL);
        assert_eq!(resolve_key(&chr("w"), cmd),  Some(Action::Quit));
        assert_eq!(resolve_key(&chr("w"), ctrl), Some(Action::Quit));
    }

    #[test]
    fn bindings_table_sane() {
        for b in BINDINGS {
            assert!(!b.keys.is_empty(),  "binding with empty keys: {}", b.desc);
            assert!(!b.desc.is_empty(),  "binding with empty desc");
        }
    }
}
