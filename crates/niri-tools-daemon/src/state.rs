use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use niri_tools_common::config::ScratchpadConfig;
use niri_tools_common::paths::state_file_path;
use niri_tools_common::types::{OutputInfo, WindowInfo, WorkspaceInfo};

pub const SCRATCHPAD_WORKSPACE: &str = "󰪷";

#[derive(Debug, Default)]
pub struct ScratchpadState {
    pub window_id: Option<u64>,
    pub visible: bool,
    pub last_used: f64,
}

#[derive(Debug, Default)]
pub struct DaemonState {
    // Niri state
    pub windows: HashMap<u64, WindowInfo>,
    pub workspaces: HashMap<u64, WorkspaceInfo>,
    pub outputs: HashMap<String, OutputInfo>,
    pub focused_output: Option<String>,
    pub focused_window_id: Option<u64>,
    pub previous_focused_window_id: Option<u64>,

    // Scratchpad state
    pub scratchpads: HashMap<String, ScratchpadState>,
    pub pending_spawns: HashSet<String>,
    pub window_to_scratchpad: HashMap<u64, String>,

    // Config
    pub scratchpad_configs: HashMap<String, ScratchpadConfig>,
    pub config_files: HashSet<PathBuf>,
    pub watch_config: bool,
}

pub fn get_niri_session_id() -> Option<String> {
    std::env::var("NIRI_SOCKET").ok()
}

fn now_timestamp() -> f64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// Serialization types for state persistence
#[derive(Serialize, Deserialize)]
struct PersistedScratchpad {
    window_id: Option<u64>,
    visible: bool,
    last_used: f64,
}

#[derive(Serialize, Deserialize)]
struct PersistedState {
    niri_session: String,
    scratchpads: HashMap<String, PersistedScratchpad>,
    window_to_scratchpad: HashMap<String, String>,
}

impl DaemonState {
    /// Returns the active workspace for the given output name.
    pub fn get_active_workspace_for_output(&self, output: &str) -> Option<&WorkspaceInfo> {
        self.workspaces
            .values()
            .find(|ws| ws.output == output && ws.is_active)
    }

    /// Returns the active workspace on the currently focused output.
    pub fn get_focused_workspace(&self) -> Option<&WorkspaceInfo> {
        let output = self.focused_output.as_deref()?;
        self.get_active_workspace_for_output(output)
    }

    /// Checks if a window is on the special scratchpad workspace.
    pub fn is_on_scratchpad_workspace(&self, window: &WindowInfo) -> bool {
        let Some(ws_id) = window.workspace_id else {
            return false;
        };
        let Some(ws) = self.workspaces.get(&ws_id) else {
            return false;
        };
        ws.name.as_deref() == Some(SCRATCHPAD_WORKSPACE)
    }

    /// Returns the scratchpad name for a given window ID, if any.
    pub fn get_scratchpad_for_window(&self, window_id: u64) -> Option<&str> {
        self.window_to_scratchpad
            .get(&window_id)
            .map(|s| s.as_str())
    }

    /// Registers a window as belonging to a named scratchpad.
    /// If the scratchpad already had a different window, the old mapping is removed.
    pub fn register_scratchpad_window(&mut self, name: &str, window_id: u64) {
        // If this scratchpad already had a window, remove the old mapping
        if let Some(sp) = self.scratchpads.get(name) {
            if let Some(old_id) = sp.window_id {
                self.window_to_scratchpad.remove(&old_id);
            }
        }

        // Create or update the scratchpad state
        let sp = self.scratchpads.entry(name.to_string()).or_default();
        sp.window_id = Some(window_id);
        sp.last_used = now_timestamp();

        // Add the reverse mapping
        self.window_to_scratchpad
            .insert(window_id, name.to_string());
    }

    /// Removes a window from scratchpad tracking.
    pub fn unregister_scratchpad_window(&mut self, window_id: u64) {
        if let Some(name) = self.window_to_scratchpad.remove(&window_id) {
            if let Some(sp) = self.scratchpads.get_mut(&name) {
                sp.window_id = None;
                sp.visible = false;
            }
        }
    }

    /// Marks a scratchpad as visible and updates its last-used timestamp.
    pub fn mark_scratchpad_visible(&mut self, name: &str) {
        if let Some(sp) = self.scratchpads.get_mut(name) {
            sp.visible = true;
            sp.last_used = now_timestamp();
        }
    }

    /// Marks a scratchpad as hidden and updates its last-used timestamp.
    pub fn mark_scratchpad_hidden(&mut self, name: &str) {
        if let Some(sp) = self.scratchpads.get_mut(name) {
            sp.visible = false;
            sp.last_used = now_timestamp();
        }
    }

    /// Updates the last-used timestamp for the scratchpad owning the given window.
    /// Does nothing if the window is not a scratchpad window.
    pub fn update_scratchpad_recency(&mut self, window_id: u64) {
        if let Some(name) = self.window_to_scratchpad.get(&window_id).cloned() {
            if let Some(sp) = self.scratchpads.get_mut(&name) {
                sp.last_used = now_timestamp();
            }
        }
    }

    /// Returns the name of the most recently used scratchpad that has a window
    /// and is currently hidden.
    pub fn get_most_recent_hidden_scratchpad(&self) -> Option<&str> {
        self.scratchpads
            .iter()
            .filter(|(_, sp)| sp.window_id.is_some() && !sp.visible)
            .max_by(|(_, a), (_, b)| a.last_used.partial_cmp(&b.last_used).unwrap())
            .map(|(name, _)| name.as_str())
    }

    /// Saves scratchpad state to disk as JSON. Uses atomic write (temp + rename).
    pub fn save_scratchpad_state(&self) -> Result<(), std::io::Error> {
        let session = get_niri_session_id().unwrap_or_default();

        let mut persisted_scratchpads = HashMap::new();
        for (name, sp) in &self.scratchpads {
            persisted_scratchpads.insert(
                name.clone(),
                PersistedScratchpad {
                    window_id: sp.window_id,
                    visible: sp.visible,
                    last_used: sp.last_used,
                },
            );
        }

        let mut persisted_w2s = HashMap::new();
        for (wid, name) in &self.window_to_scratchpad {
            persisted_w2s.insert(wid.to_string(), name.clone());
        }

        let state = PersistedState {
            niri_session: session,
            scratchpads: persisted_scratchpads,
            window_to_scratchpad: persisted_w2s,
        };

        let path = state_file_path();
        let json = serde_json::to_string_pretty(&state).map_err(std::io::Error::other)?;

        // Atomic write: write to temp file in the same directory, then rename
        let dir = path.parent().unwrap_or(std::path::Path::new("/tmp"));
        let tmp_path = dir.join(".niri-tools-state.json.tmp");
        {
            let mut f = std::fs::File::create(&tmp_path)?;
            f.write_all(json.as_bytes())?;
            f.flush()?;
        }
        std::fs::rename(&tmp_path, &path)?;

        Ok(())
    }

    /// Loads scratchpad state from disk. Returns true if state was loaded successfully.
    /// Returns false if the file doesn't exist, session mismatches, or there's a parse error.
    pub fn load_scratchpad_state(&mut self) -> bool {
        let path = state_file_path();
        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => return false,
        };

        let persisted: PersistedState = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(_) => return false,
        };

        // Validate session
        let current_session = get_niri_session_id().unwrap_or_default();
        if persisted.niri_session != current_session {
            return false;
        }

        // Restore scratchpad states
        for (name, ps) in persisted.scratchpads {
            self.scratchpads.insert(
                name,
                ScratchpadState {
                    window_id: ps.window_id,
                    visible: ps.visible,
                    last_used: ps.last_used,
                },
            );
        }

        // Restore window-to-scratchpad mappings
        for (wid_str, name) in persisted.window_to_scratchpad {
            if let Ok(wid) = wid_str.parse::<u64>() {
                self.window_to_scratchpad.insert(wid, name);
            }
        }

        true
    }

    /// Removes scratchpad mappings for windows that no longer exist.
    pub fn reconcile_with_windows(&mut self, window_ids: &HashSet<u64>) {
        // Collect orphaned window IDs
        let orphaned: Vec<u64> = self
            .window_to_scratchpad
            .keys()
            .filter(|wid| !window_ids.contains(wid))
            .copied()
            .collect();

        for wid in orphaned {
            self.unregister_scratchpad_window(wid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_workspace(id: u64, output: &str, is_active: bool, name: Option<&str>) -> WorkspaceInfo {
        WorkspaceInfo {
            id,
            idx: 1,
            output: output.to_string(),
            is_active,
            name: name.map(|s| s.to_string()),
        }
    }

    fn make_window(id: u64, workspace_id: Option<u64>) -> WindowInfo {
        WindowInfo {
            id,
            app_id: "test".to_string(),
            title: "test".to_string(),
            workspace_id,
            is_focused: false,
            is_floating: false,
            width: 800,
            height: 600,
        }
    }

    fn make_output(name: &str) -> OutputInfo {
        OutputInfo {
            name: name.to_string(),
            width: 1920,
            height: 1080,
        }
    }

    fn setup_state_with_workspaces() -> DaemonState {
        let mut state = DaemonState::default();

        let ws1 = make_workspace(1, "eDP-1", true, Some("main"));
        let ws2 = make_workspace(2, "eDP-1", false, Some("code"));
        let ws3 = make_workspace(3, "HDMI-A-1", true, Some("web"));

        state.workspaces.insert(1, ws1);
        state.workspaces.insert(2, ws2);
        state.workspaces.insert(3, ws3);

        state
            .outputs
            .insert("eDP-1".to_string(), make_output("eDP-1"));
        state
            .outputs
            .insert("HDMI-A-1".to_string(), make_output("HDMI-A-1"));

        state.focused_output = Some("eDP-1".to_string());
        state
    }

    // -- Workspace lookup tests --

    #[test]
    fn get_active_workspace_for_output_returns_correct_workspace() {
        let state = setup_state_with_workspaces();
        let ws = state.get_active_workspace_for_output("eDP-1").unwrap();
        assert_eq!(ws.id, 1);
        assert_eq!(ws.name.as_deref(), Some("main"));
    }

    #[test]
    fn get_active_workspace_for_output_returns_none_for_unknown() {
        let state = setup_state_with_workspaces();
        assert!(state.get_active_workspace_for_output("VGA-1").is_none());
    }

    #[test]
    fn get_focused_workspace_returns_workspace_on_focused_output() {
        let state = setup_state_with_workspaces();
        let ws = state.get_focused_workspace().unwrap();
        assert_eq!(ws.id, 1);
        assert_eq!(ws.output, "eDP-1");
    }

    #[test]
    fn get_focused_workspace_returns_none_when_no_focused_output() {
        let mut state = setup_state_with_workspaces();
        state.focused_output = None;
        assert!(state.get_focused_workspace().is_none());
    }

    // -- Scratchpad workspace detection --

    #[test]
    fn is_on_scratchpad_workspace_returns_true() {
        let mut state = DaemonState::default();
        let ws = make_workspace(10, "eDP-1", true, Some(SCRATCHPAD_WORKSPACE));
        state.workspaces.insert(10, ws);

        let window = make_window(1, Some(10));
        assert!(state.is_on_scratchpad_workspace(&window));
    }

    #[test]
    fn is_on_scratchpad_workspace_returns_false_for_normal() {
        let mut state = DaemonState::default();
        let ws = make_workspace(10, "eDP-1", true, Some("main"));
        state.workspaces.insert(10, ws);

        let window = make_window(1, Some(10));
        assert!(!state.is_on_scratchpad_workspace(&window));
    }

    #[test]
    fn is_on_scratchpad_workspace_returns_false_for_no_workspace() {
        let state = DaemonState::default();
        let window = make_window(1, None);
        assert!(!state.is_on_scratchpad_workspace(&window));
    }

    // -- Register/unregister --

    #[test]
    fn register_scratchpad_window_creates_mapping_and_state() {
        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);

        assert_eq!(
            state.window_to_scratchpad.get(&42),
            Some(&"term".to_string())
        );
        let sp = state.scratchpads.get("term").unwrap();
        assert_eq!(sp.window_id, Some(42));
        assert!(sp.last_used > 0.0);
    }

    #[test]
    fn register_scratchpad_window_on_existing_updates_window_id() {
        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);
        state.register_scratchpad_window("term", 99);

        // Old mapping removed
        assert!(state.window_to_scratchpad.get(&42).is_none());
        // New mapping present
        assert_eq!(
            state.window_to_scratchpad.get(&99),
            Some(&"term".to_string())
        );
        let sp = state.scratchpads.get("term").unwrap();
        assert_eq!(sp.window_id, Some(99));
    }

    #[test]
    fn unregister_scratchpad_window_clears_mappings() {
        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);
        state.unregister_scratchpad_window(42);

        assert!(state.window_to_scratchpad.get(&42).is_none());
        let sp = state.scratchpads.get("term").unwrap();
        assert_eq!(sp.window_id, None);
        assert!(!sp.visible);
    }

    #[test]
    fn get_scratchpad_for_window_returns_correct_name() {
        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);
        assert_eq!(state.get_scratchpad_for_window(42), Some("term"));
        assert_eq!(state.get_scratchpad_for_window(99), None);
    }

    // -- Visibility --

    #[test]
    fn mark_scratchpad_visible_sets_visible_and_updates_last_used() {
        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);

        let before = state.scratchpads.get("term").unwrap().last_used;
        // Sleep briefly so timestamp advances
        std::thread::sleep(std::time::Duration::from_millis(10));

        state.mark_scratchpad_visible("term");

        let sp = state.scratchpads.get("term").unwrap();
        assert!(sp.visible);
        assert!(sp.last_used >= before);
    }

    #[test]
    fn mark_scratchpad_hidden_clears_visible_and_updates_last_used() {
        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);
        state.mark_scratchpad_visible("term");

        std::thread::sleep(std::time::Duration::from_millis(10));
        state.mark_scratchpad_hidden("term");

        let sp = state.scratchpads.get("term").unwrap();
        assert!(!sp.visible);
    }

    // -- Recency --

    #[test]
    fn update_scratchpad_recency_updates_last_used() {
        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);

        let before = state.scratchpads.get("term").unwrap().last_used;
        std::thread::sleep(std::time::Duration::from_millis(10));

        state.update_scratchpad_recency(42);

        let after = state.scratchpads.get("term").unwrap().last_used;
        assert!(after >= before);
    }

    #[test]
    fn update_scratchpad_recency_ignores_non_scratchpad_windows() {
        let mut state = DaemonState::default();
        // Should not panic or modify anything
        state.update_scratchpad_recency(999);
    }

    #[test]
    fn get_most_recent_hidden_scratchpad_returns_most_recent() {
        let mut state = DaemonState::default();

        // Register two scratchpads
        state.register_scratchpad_window("term", 42);
        std::thread::sleep(std::time::Duration::from_millis(10));
        state.register_scratchpad_window("browser", 43);

        // Both have windows, both not visible (default)
        // browser was registered more recently
        let result = state.get_most_recent_hidden_scratchpad();
        assert_eq!(result, Some("browser"));
    }

    #[test]
    fn get_most_recent_hidden_scratchpad_skips_visible() {
        let mut state = DaemonState::default();

        state.register_scratchpad_window("term", 42);
        std::thread::sleep(std::time::Duration::from_millis(10));
        state.register_scratchpad_window("browser", 43);
        state.mark_scratchpad_visible("browser");

        // browser is visible, so term should be returned
        let result = state.get_most_recent_hidden_scratchpad();
        assert_eq!(result, Some("term"));
    }

    #[test]
    fn get_most_recent_hidden_scratchpad_skips_no_window() {
        let mut state = DaemonState::default();

        // Scratchpad exists but has no window
        state
            .scratchpads
            .insert("empty".to_string(), ScratchpadState::default());

        state.register_scratchpad_window("term", 42);

        let result = state.get_most_recent_hidden_scratchpad();
        assert_eq!(result, Some("term"));
    }

    #[test]
    fn get_most_recent_hidden_scratchpad_returns_none_when_all_visible() {
        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);
        state.mark_scratchpad_visible("term");

        assert!(state.get_most_recent_hidden_scratchpad().is_none());
    }

    // -- Persistence --

    #[test]
    fn save_and_load_scratchpad_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        // Set env vars for the test
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", dir.path());
            std::env::set_var("NIRI_SOCKET", "/run/user/1000/niri.1234.0.sock");
        }

        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);
        state.mark_scratchpad_visible("term");
        state.register_scratchpad_window("browser", 43);

        state.save_scratchpad_state().unwrap();

        // Load into fresh state
        let mut state2 = DaemonState::default();
        assert!(state2.load_scratchpad_state());

        assert_eq!(
            state2.window_to_scratchpad.get(&42),
            Some(&"term".to_string())
        );
        assert_eq!(
            state2.window_to_scratchpad.get(&43),
            Some(&"browser".to_string())
        );

        let sp_term = state2.scratchpads.get("term").unwrap();
        assert_eq!(sp_term.window_id, Some(42));
        assert!(sp_term.visible);

        let sp_browser = state2.scratchpads.get("browser").unwrap();
        assert_eq!(sp_browser.window_id, Some(43));
        assert!(!sp_browser.visible);
    }

    #[test]
    fn load_scratchpad_state_returns_false_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", dir.path());
        }

        let mut state = DaemonState::default();
        assert!(!state.load_scratchpad_state());
    }

    #[test]
    fn load_scratchpad_state_returns_false_for_session_mismatch() {
        let dir = tempfile::tempdir().unwrap();

        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", dir.path());
            std::env::set_var("NIRI_SOCKET", "/run/user/1000/niri.1234.0.sock");
        }

        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);
        state.save_scratchpad_state().unwrap();

        // Change session
        unsafe {
            std::env::set_var("NIRI_SOCKET", "/run/user/1000/niri.9999.0.sock");
        }

        let mut state2 = DaemonState::default();
        assert!(!state2.load_scratchpad_state());
    }

    #[test]
    fn reconcile_with_windows_removes_orphaned_mappings() {
        let mut state = DaemonState::default();
        state.register_scratchpad_window("term", 42);
        state.register_scratchpad_window("browser", 43);

        // Only window 42 still exists
        let mut existing = HashSet::new();
        existing.insert(42);

        state.reconcile_with_windows(&existing);

        // term (42) should remain
        assert_eq!(
            state.window_to_scratchpad.get(&42),
            Some(&"term".to_string())
        );
        assert_eq!(state.scratchpads.get("term").unwrap().window_id, Some(42));

        // browser (43) should be cleaned up
        assert!(state.window_to_scratchpad.get(&43).is_none());
        let sp_browser = state.scratchpads.get("browser").unwrap();
        assert_eq!(sp_browser.window_id, None);
        assert!(!sp_browser.visible);
    }
}
