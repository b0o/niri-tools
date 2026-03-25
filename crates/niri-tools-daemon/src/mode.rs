use std::collections::HashMap;

use niri_tools_common::config::{BindConfig, BindOption, ModeConfig};

/// Manages the mode stack for the mode overlay.
///
/// The mode stack tracks nested mode navigation. When a `switch-mode` bind
/// is activated, the target mode is pushed onto the stack. Backspace pops
/// the stack. Escape/close clears it entirely.
pub struct ModeState {
    mode_stack: Vec<String>,
    modes: HashMap<String, ModeConfig>,
}

impl ModeState {
    pub fn new(modes: HashMap<String, ModeConfig>) -> Self {
        Self {
            mode_stack: Vec::new(),
            modes,
        }
    }

    /// Update the mode definitions (e.g., on config reload).
    pub fn update_modes(&mut self, modes: HashMap<String, ModeConfig>) {
        self.modes = modes;
    }

    /// Get the current (topmost) mode config, if any.
    pub fn current_mode(&self) -> Option<&ModeConfig> {
        self.mode_stack.last().and_then(|name| self.modes.get(name))
    }

    /// Push a mode onto the stack. Returns `true` if the mode exists.
    pub fn push_mode(&mut self, name: &str) -> bool {
        if self.modes.contains_key(name) {
            self.mode_stack.push(name.to_string());
            true
        } else {
            false
        }
    }

    /// Pop the top mode from the stack. Returns `false` if the stack is empty.
    pub fn pop_mode(&mut self) -> bool {
        self.mode_stack.pop().is_some()
    }

    /// Clear the entire mode stack.
    pub fn clear(&mut self) {
        self.mode_stack.clear();
    }

    /// Get the mode stack depth.
    pub fn depth(&self) -> usize {
        self.mode_stack.len()
    }

    /// Look up a bind by key in the current mode.
    ///
    /// Super/Mod is stripped from the key name before lookup -- only
    /// Ctrl, Shift, and Alt modifiers are significant.
    pub fn lookup_bind(&self, key: &str) -> Option<&BindConfig> {
        let mode = self.current_mode()?;

        // First try exact match
        if let Some(bind) = mode.binds.iter().find(|b| b.key == key) {
            return Some(bind);
        }

        // Then try alias match
        mode.binds.iter().find(|b| {
            b.options
                .iter()
                .any(|opt| matches!(opt, BindOption::Alias(a) if a == key))
        })
    }

    /// Check whether the current mode has `keep_open` set.
    pub fn current_keep_open(&self) -> bool {
        self.current_mode().is_some_and(|m| m.keep_open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use niri_tools_common::config::{BindAction, BindConfig, BindOption, ModeConfig};

    fn make_modes() -> HashMap<String, ModeConfig> {
        let mut modes = HashMap::new();
        modes.insert(
            "root".to_string(),
            ModeConfig {
                name: "root".to_string(),
                keep_open: false,
                binds: vec![
                    BindConfig {
                        key: "Space".to_string(),
                        description: "Launcher".to_string(),
                        options: vec![],
                        action: BindAction::SpawnSh("rofi -show drun".to_string()),
                    },
                    BindConfig {
                        key: "b".to_string(),
                        description: "Brightness".to_string(),
                        options: vec![],
                        action: BindAction::SwitchMode("brightness".to_string()),
                    },
                ],
            },
        );
        modes.insert(
            "brightness".to_string(),
            ModeConfig {
                name: "brightness".to_string(),
                keep_open: true,
                binds: vec![
                    BindConfig {
                        key: "j".to_string(),
                        description: "-5".to_string(),
                        options: vec![BindOption::KeepOpen],
                        action: BindAction::SpawnSh("brightness -5".to_string()),
                    },
                    BindConfig {
                        key: "k".to_string(),
                        description: "+5".to_string(),
                        options: vec![],
                        action: BindAction::SpawnSh("brightness +5".to_string()),
                    },
                    BindConfig {
                        key: "?".to_string(),
                        description: "Query".to_string(),
                        options: vec![BindOption::Alias("q".to_string())],
                        action: BindAction::SpawnSh("brightness -q".to_string()),
                    },
                ],
            },
        );
        modes
    }

    #[test]
    fn push_and_current_mode() {
        let mut state = ModeState::new(make_modes());
        assert!(state.current_mode().is_none());

        assert!(state.push_mode("root"));
        assert_eq!(state.current_mode().unwrap().name, "root");

        assert!(state.push_mode("brightness"));
        assert_eq!(state.current_mode().unwrap().name, "brightness");
    }

    #[test]
    fn push_nonexistent_mode_returns_false() {
        let mut state = ModeState::new(make_modes());
        assert!(!state.push_mode("nonexistent"));
        assert!(state.current_mode().is_none());
    }

    #[test]
    fn pop_mode() {
        let mut state = ModeState::new(make_modes());
        state.push_mode("root");
        state.push_mode("brightness");

        assert!(state.pop_mode());
        assert_eq!(state.current_mode().unwrap().name, "root");

        assert!(state.pop_mode());
        assert!(state.current_mode().is_none());

        assert!(!state.pop_mode()); // empty stack
    }

    #[test]
    fn clear_mode_stack() {
        let mut state = ModeState::new(make_modes());
        state.push_mode("root");
        state.push_mode("brightness");
        assert_eq!(state.depth(), 2);

        state.clear();
        assert_eq!(state.depth(), 0);
        assert!(state.current_mode().is_none());
    }

    #[test]
    fn lookup_bind_exact_match() {
        let mut state = ModeState::new(make_modes());
        state.push_mode("root");

        let bind = state.lookup_bind("Space").unwrap();
        assert_eq!(bind.description, "Launcher");

        let bind = state.lookup_bind("b").unwrap();
        assert_eq!(bind.description, "Brightness");

        assert!(state.lookup_bind("z").is_none());
    }

    #[test]
    fn lookup_bind_alias_match() {
        let mut state = ModeState::new(make_modes());
        state.push_mode("brightness");

        // "q" is an alias for "?"
        let bind = state.lookup_bind("q").unwrap();
        assert_eq!(bind.key, "?");
        assert_eq!(bind.description, "Query");
    }

    #[test]
    fn lookup_bind_empty_stack_returns_none() {
        let state = ModeState::new(make_modes());
        assert!(state.lookup_bind("Space").is_none());
    }

    #[test]
    fn current_keep_open() {
        let mut state = ModeState::new(make_modes());

        // No mode → false
        assert!(!state.current_keep_open());

        // Root mode → false
        state.push_mode("root");
        assert!(!state.current_keep_open());

        // Brightness mode → true
        state.push_mode("brightness");
        assert!(state.current_keep_open());
    }

    #[test]
    fn update_modes() {
        let mut state = ModeState::new(make_modes());
        state.push_mode("root");
        assert_eq!(state.current_mode().unwrap().binds.len(), 2);

        // Update with new modes
        let mut new_modes = HashMap::new();
        new_modes.insert(
            "root".to_string(),
            ModeConfig {
                name: "root".to_string(),
                keep_open: false,
                binds: vec![BindConfig {
                    key: "a".to_string(),
                    description: "Only bind".to_string(),
                    options: vec![],
                    action: BindAction::SpawnSh("echo a".to_string()),
                }],
            },
        );
        state.update_modes(new_modes);

        // Stack still has "root" on it, and new mode definition is used
        assert_eq!(state.current_mode().unwrap().binds.len(), 1);
    }
}
