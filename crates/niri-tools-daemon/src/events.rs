use std::collections::{HashMap, HashSet};

use niri_tools_common::types::{NiriEvent, OutputInfo, WindowInfo, WorkspaceInfo};
use serde_json::Value;

use crate::state::DaemonState;

/// Actions the event loop should take after applying an event.
#[derive(Debug, PartialEq)]
pub enum EventAction {
    /// No further action needed.
    None,
    /// A new window appeared; the scratchpad manager should check if it
    /// matches a pending spawn.
    WindowOpened(WindowInfo),
    /// A full window list was received; reconcile scratchpad state with
    /// the given set of known window IDs.
    Reconcile(HashSet<u64>),
    /// Scratchpad state changed in a way that warrants persisting to disk.
    SaveState,
    /// The workspace list changed; caller should re-fetch workspaces.
    ReloadWorkspaces,
}

// ---------------------------------------------------------------------------
// Event parsing
// ---------------------------------------------------------------------------

/// Parse a JSON event line from niri's event stream into a `NiriEvent`.
pub fn parse_niri_event(json: &Value) -> Option<NiriEvent> {
    let obj = json.as_object()?;

    if let Some(data) = obj.get("WindowOpenedOrChanged") {
        let window_data = data.get("window").unwrap_or(data);
        let window = parse_window_info(window_data)?;
        return Some(NiriEvent::WindowOpenedOrChanged(window));
    }

    if let Some(data) = obj.get("WindowsChanged") {
        let windows_arr = data.get("windows").unwrap_or(data);
        let arr = windows_arr.as_array()?;
        let windows: Vec<WindowInfo> = arr.iter().filter_map(parse_window_info).collect();
        return Some(NiriEvent::WindowsChanged(windows));
    }

    if let Some(data) = obj.get("WindowClosed") {
        let id = data.get("id").and_then(|v| v.as_u64())?;
        return Some(NiriEvent::WindowClosed { id });
    }

    if let Some(data) = obj.get("WindowFocusChanged") {
        let id = data.get("id").and_then(|v| v.as_u64());
        return Some(NiriEvent::WindowFocusChanged { id });
    }

    if let Some(data) = obj.get("WorkspaceActivated") {
        let id = data.get("id").and_then(|v| v.as_u64())?;
        let focused = data
            .get("focused")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        return Some(NiriEvent::WorkspaceActivated { id, focused });
    }

    if obj.contains_key("WorkspacesChanged") {
        return Some(NiriEvent::WorkspacesChanged);
    }

    if let Some(data) = obj.get("OutputFocusChanged") {
        let output = data
            .get("output")
            .and_then(|v| v.as_str())
            .map(String::from);
        return Some(NiriEvent::OutputFocusChanged { output });
    }

    if let Some(data) = obj.get("OutputsChanged") {
        let outputs_data = data.get("outputs").unwrap_or(data);
        let map = outputs_data.as_object()?;
        let mut outputs = HashMap::new();
        for (name, val) in map {
            outputs.insert(name.clone(), parse_output_info(name, val));
        }
        return Some(NiriEvent::OutputsChanged(outputs));
    }

    None
}

/// Parse a niri window JSON object into `WindowInfo`.
fn parse_window_info(data: &Value) -> Option<WindowInfo> {
    let id = data.get("id").and_then(|v| v.as_u64())?;
    let app_id = data
        .get("app_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let title = data
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let workspace_id = data.get("workspace_id").and_then(|v| v.as_u64());
    let is_focused = data
        .get("is_focused")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let is_floating = data
        .get("is_floating")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // width/height may come from layout.window_size[0]/[1] or directly
    let (width, height) = if let Some(layout) = data.get("layout") {
        let ws = layout.get("window_size");
        let w = ws
            .and_then(|a| a.get(0))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let h = ws
            .and_then(|a| a.get(1))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        (w, h)
    } else {
        let w = data.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let h = data.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        (w, h)
    };

    Some(WindowInfo {
        id,
        app_id,
        title,
        workspace_id,
        is_focused,
        is_floating,
        width,
        height,
    })
}

/// Parse a niri workspace JSON object into `WorkspaceInfo`.
pub fn parse_workspace_info(data: &Value) -> Option<WorkspaceInfo> {
    let id = data.get("id").and_then(|v| v.as_u64())?;
    let idx = data.get("idx").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let output = data
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let is_active = data
        .get("is_active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let name = data.get("name").and_then(|v| v.as_str()).map(String::from);

    Some(WorkspaceInfo {
        id,
        idx,
        output,
        is_active,
        name,
    })
}

/// Parse a niri output JSON object into `OutputInfo`.
pub fn parse_output_info(name: &str, data: &Value) -> OutputInfo {
    let logical = data.get("logical");
    let width = logical
        .and_then(|l| l.get("width"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let height = logical
        .and_then(|l| l.get("height"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    OutputInfo {
        name: name.to_string(),
        width,
        height,
    }
}

// ---------------------------------------------------------------------------
// Event application
// ---------------------------------------------------------------------------

/// Apply a `NiriEvent` to the daemon state, returning an `EventAction` that
/// tells the caller what follow-up work (if any) is needed.
pub fn apply_event(state: &mut DaemonState, event: &NiriEvent) -> EventAction {
    match event {
        NiriEvent::WindowOpenedOrChanged(window) => {
            let is_new = !state.windows.contains_key(&window.id);
            state.windows.insert(window.id, window.clone());
            if is_new {
                EventAction::WindowOpened(window.clone())
            } else {
                EventAction::None
            }
        }

        NiriEvent::WindowsChanged(windows) => {
            let window_ids: HashSet<u64> = windows.iter().map(|w| w.id).collect();
            for w in windows {
                state.windows.insert(w.id, w.clone());
                if w.is_focused {
                    state.focused_window_id = Some(w.id);
                }
            }
            EventAction::Reconcile(window_ids)
        }

        NiriEvent::WindowClosed { id } => {
            state.windows.remove(id);
            let was_scratchpad = state.window_to_scratchpad.contains_key(id);
            state.unregister_scratchpad_window(*id);
            if was_scratchpad {
                EventAction::SaveState
            } else {
                EventAction::None
            }
        }

        NiriEvent::WindowFocusChanged { id } => {
            // Clear old focus flags
            for w in state.windows.values_mut() {
                w.is_focused = false;
            }

            let old_focus = state.focused_window_id;

            if let Some(id) = id {
                state.focused_window_id = Some(*id);
                if let Some(w) = state.windows.get_mut(id) {
                    w.is_focused = true;
                }
                state.update_scratchpad_recency(*id);
            } else {
                state.focused_window_id = None;
            }

            // Track previous focus (only when it actually changed)
            if old_focus != state.focused_window_id {
                if let Some(old) = old_focus {
                    state.previous_focused_window_id = Some(old);
                }
            }

            EventAction::None
        }

        NiriEvent::WorkspaceActivated { id, focused } => {
            if let Some(ws) = state.workspaces.get(id) {
                let output = ws.output.clone();
                for ws in state.workspaces.values_mut() {
                    if ws.output == output {
                        ws.is_active = ws.id == *id;
                    }
                }
                if *focused {
                    state.focused_output = Some(output);
                }
            }
            EventAction::None
        }

        NiriEvent::WorkspacesChanged => EventAction::ReloadWorkspaces,

        NiriEvent::OutputFocusChanged { output } => {
            state.focused_output = output.clone();
            EventAction::None
        }

        NiriEvent::OutputsChanged(outputs) => {
            state.outputs = outputs.clone();
            EventAction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // Event parsing tests
    // ---------------------------------------------------------------

    #[test]
    fn parse_window_opened_or_changed() {
        let json: Value = serde_json::from_str(
            r#"{
                "WindowOpenedOrChanged": {
                    "window": {
                        "id": 42,
                        "app_id": "ghostty",
                        "title": "Terminal",
                        "workspace_id": 1,
                        "is_focused": true,
                        "is_floating": false,
                        "width": 800,
                        "height": 600
                    }
                }
            }"#,
        )
        .unwrap();

        let event = parse_niri_event(&json).unwrap();
        match event {
            NiriEvent::WindowOpenedOrChanged(w) => {
                assert_eq!(w.id, 42);
                assert_eq!(w.app_id, "ghostty");
                assert_eq!(w.title, "Terminal");
                assert_eq!(w.workspace_id, Some(1));
                assert!(w.is_focused);
                assert!(!w.is_floating);
                assert_eq!(w.width, 800);
                assert_eq!(w.height, 600);
            }
            other => panic!("Expected WindowOpenedOrChanged, got {other:?}"),
        }
    }

    #[test]
    fn parse_window_opened_with_layout_size() {
        let json: Value = serde_json::from_str(
            r#"{
                "WindowOpenedOrChanged": {
                    "window": {
                        "id": 10,
                        "app_id": "foot",
                        "title": "bash",
                        "workspace_id": 2,
                        "is_focused": false,
                        "is_floating": true,
                        "layout": {
                            "window_size": [1200, 900]
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let event = parse_niri_event(&json).unwrap();
        match event {
            NiriEvent::WindowOpenedOrChanged(w) => {
                assert_eq!(w.id, 10);
                assert_eq!(w.width, 1200);
                assert_eq!(w.height, 900);
            }
            other => panic!("Expected WindowOpenedOrChanged, got {other:?}"),
        }
    }

    #[test]
    fn parse_windows_changed() {
        let json: Value = serde_json::from_str(
            r#"{
                "WindowsChanged": {
                    "windows": [
                        {
                            "id": 1,
                            "app_id": "ghostty",
                            "title": "Term 1",
                            "workspace_id": 1,
                            "is_focused": true,
                            "is_floating": false,
                            "width": 800,
                            "height": 600
                        },
                        {
                            "id": 2,
                            "app_id": "firefox",
                            "title": "Browser",
                            "workspace_id": 2,
                            "is_focused": false,
                            "is_floating": false,
                            "width": 1920,
                            "height": 1080
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let event = parse_niri_event(&json).unwrap();
        match event {
            NiriEvent::WindowsChanged(windows) => {
                assert_eq!(windows.len(), 2);
                assert_eq!(windows[0].id, 1);
                assert_eq!(windows[0].app_id, "ghostty");
                assert_eq!(windows[1].id, 2);
                assert_eq!(windows[1].app_id, "firefox");
            }
            other => panic!("Expected WindowsChanged, got {other:?}"),
        }
    }

    #[test]
    fn parse_window_closed() {
        let json: Value = serde_json::from_str(r#"{"WindowClosed": {"id": 99}}"#).unwrap();

        let event = parse_niri_event(&json).unwrap();
        assert_eq!(event, NiriEvent::WindowClosed { id: 99 });
    }

    #[test]
    fn parse_window_focus_changed_with_id() {
        let json: Value = serde_json::from_str(r#"{"WindowFocusChanged": {"id": 42}}"#).unwrap();

        let event = parse_niri_event(&json).unwrap();
        assert_eq!(event, NiriEvent::WindowFocusChanged { id: Some(42) });
    }

    #[test]
    fn parse_window_focus_changed_without_id() {
        let json: Value = serde_json::from_str(r#"{"WindowFocusChanged": {"id": null}}"#).unwrap();

        let event = parse_niri_event(&json).unwrap();
        assert_eq!(event, NiriEvent::WindowFocusChanged { id: None });
    }

    #[test]
    fn parse_workspace_activated() {
        let json: Value =
            serde_json::from_str(r#"{"WorkspaceActivated": {"id": 5, "focused": true}}"#).unwrap();

        let event = parse_niri_event(&json).unwrap();
        assert_eq!(
            event,
            NiriEvent::WorkspaceActivated {
                id: 5,
                focused: true
            }
        );
    }

    #[test]
    fn parse_workspace_activated_not_focused() {
        let json: Value =
            serde_json::from_str(r#"{"WorkspaceActivated": {"id": 3, "focused": false}}"#).unwrap();

        let event = parse_niri_event(&json).unwrap();
        assert_eq!(
            event,
            NiriEvent::WorkspaceActivated {
                id: 3,
                focused: false
            }
        );
    }

    #[test]
    fn parse_workspaces_changed() {
        let json: Value = serde_json::from_str(r#"{"WorkspacesChanged": {}}"#).unwrap();

        let event = parse_niri_event(&json).unwrap();
        assert_eq!(event, NiriEvent::WorkspacesChanged);
    }

    #[test]
    fn parse_output_focus_changed() {
        let json: Value =
            serde_json::from_str(r#"{"OutputFocusChanged": {"output": "HDMI-A-1"}}"#).unwrap();

        let event = parse_niri_event(&json).unwrap();
        assert_eq!(
            event,
            NiriEvent::OutputFocusChanged {
                output: Some("HDMI-A-1".to_string())
            }
        );
    }

    #[test]
    fn parse_output_focus_changed_null() {
        let json: Value =
            serde_json::from_str(r#"{"OutputFocusChanged": {"output": null}}"#).unwrap();

        let event = parse_niri_event(&json).unwrap();
        assert_eq!(event, NiriEvent::OutputFocusChanged { output: None });
    }

    #[test]
    fn parse_outputs_changed() {
        let json: Value = serde_json::from_str(
            r#"{
                "OutputsChanged": {
                    "outputs": {
                        "eDP-1": {
                            "logical": {"width": 1920, "height": 1080}
                        },
                        "HDMI-A-1": {
                            "logical": {"width": 2560, "height": 1440}
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let event = parse_niri_event(&json).unwrap();
        match event {
            NiriEvent::OutputsChanged(outputs) => {
                assert_eq!(outputs.len(), 2);
                let edp = outputs.get("eDP-1").unwrap();
                assert_eq!(edp.name, "eDP-1");
                assert_eq!(edp.width, 1920);
                assert_eq!(edp.height, 1080);
                let hdmi = outputs.get("HDMI-A-1").unwrap();
                assert_eq!(hdmi.name, "HDMI-A-1");
                assert_eq!(hdmi.width, 2560);
                assert_eq!(hdmi.height, 1440);
            }
            other => panic!("Expected OutputsChanged, got {other:?}"),
        }
    }

    #[test]
    fn parse_unknown_event_returns_none() {
        let json: Value = serde_json::from_str(r#"{"SomeFutureEvent": {"data": 123}}"#).unwrap();

        assert!(parse_niri_event(&json).is_none());
    }

    // ---------------------------------------------------------------
    // parse_workspace_info tests
    // ---------------------------------------------------------------

    #[test]
    fn parse_workspace_info_full() {
        let json: Value = serde_json::from_str(
            r#"{"id": 3, "idx": 2, "output": "eDP-1", "is_active": true, "name": "code"}"#,
        )
        .unwrap();

        let ws = parse_workspace_info(&json).unwrap();
        assert_eq!(ws.id, 3);
        assert_eq!(ws.idx, 2);
        assert_eq!(ws.output, "eDP-1");
        assert!(ws.is_active);
        assert_eq!(ws.name, Some("code".to_string()));
    }

    #[test]
    fn parse_workspace_info_without_name() {
        let json: Value = serde_json::from_str(
            r#"{"id": 1, "idx": 1, "output": "HDMI-A-1", "is_active": false}"#,
        )
        .unwrap();

        let ws = parse_workspace_info(&json).unwrap();
        assert_eq!(ws.id, 1);
        assert!(ws.name.is_none());
    }

    // ---------------------------------------------------------------
    // parse_output_info tests
    // ---------------------------------------------------------------

    #[test]
    fn parse_output_info_with_logical() {
        let json: Value =
            serde_json::from_str(r#"{"logical": {"width": 3840, "height": 2160}}"#).unwrap();

        let output = parse_output_info("DP-1", &json);
        assert_eq!(output.name, "DP-1");
        assert_eq!(output.width, 3840);
        assert_eq!(output.height, 2160);
    }

    #[test]
    fn parse_output_info_without_logical() {
        let json: Value = serde_json::from_str(r#"{}"#).unwrap();

        let output = parse_output_info("eDP-1", &json);
        assert_eq!(output.name, "eDP-1");
        assert_eq!(output.width, 0);
        assert_eq!(output.height, 0);
    }

    // ---------------------------------------------------------------
    // Event application tests
    // ---------------------------------------------------------------

    fn make_window(id: u64) -> WindowInfo {
        WindowInfo {
            id,
            app_id: "test".to_string(),
            title: "test".to_string(),
            workspace_id: Some(1),
            is_focused: false,
            is_floating: false,
            width: 800,
            height: 600,
        }
    }

    fn make_workspace(id: u64, output: &str, is_active: bool) -> WorkspaceInfo {
        WorkspaceInfo {
            id,
            idx: 1,
            output: output.to_string(),
            is_active,
            name: None,
        }
    }

    #[test]
    fn apply_window_opened_adds_to_state_and_returns_window_opened() {
        let mut state = DaemonState::default();
        let window = make_window(42);
        let event = NiriEvent::WindowOpenedOrChanged(window.clone());

        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::WindowOpened(window.clone()));
        assert!(state.windows.contains_key(&42));
        assert_eq!(state.windows.get(&42).unwrap().id, 42);
    }

    #[test]
    fn apply_window_opened_updates_existing_returns_none() {
        let mut state = DaemonState::default();
        let window = make_window(42);
        state.windows.insert(42, window.clone());

        let mut updated = window.clone();
        updated.title = "updated".to_string();
        let event = NiriEvent::WindowOpenedOrChanged(updated.clone());

        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::None);
        assert_eq!(state.windows.get(&42).unwrap().title, "updated");
    }

    #[test]
    fn apply_windows_changed_reconciles() {
        let mut state = DaemonState::default();
        let w1 = WindowInfo {
            is_focused: true,
            ..make_window(1)
        };
        let w2 = make_window(2);
        let event = NiriEvent::WindowsChanged(vec![w1, w2]);

        let action = apply_event(&mut state, &event);

        match action {
            EventAction::Reconcile(ids) => {
                assert!(ids.contains(&1));
                assert!(ids.contains(&2));
                assert_eq!(ids.len(), 2);
            }
            other => panic!("Expected Reconcile, got {other:?}"),
        }
        assert_eq!(state.focused_window_id, Some(1));
        assert_eq!(state.windows.len(), 2);
    }

    #[test]
    fn apply_window_closed_removes_from_state() {
        let mut state = DaemonState::default();
        state.windows.insert(42, make_window(42));

        let event = NiriEvent::WindowClosed { id: 42 };
        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::None);
        assert!(!state.windows.contains_key(&42));
    }

    #[test]
    fn apply_window_closed_returns_save_state_if_was_scratchpad() {
        let mut state = DaemonState::default();
        state.windows.insert(42, make_window(42));
        state.register_scratchpad_window("term", 42);

        let event = NiriEvent::WindowClosed { id: 42 };
        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::SaveState);
        assert!(!state.windows.contains_key(&42));
        assert!(!state.window_to_scratchpad.contains_key(&42));
    }

    #[test]
    fn apply_window_focus_changed_updates_focus_tracking() {
        let mut state = DaemonState::default();
        state.windows.insert(1, make_window(1));
        state.windows.insert(2, make_window(2));
        state.focused_window_id = Some(1);

        // Focus window 2
        let event = NiriEvent::WindowFocusChanged { id: Some(2) };
        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::None);
        assert_eq!(state.focused_window_id, Some(2));
        assert_eq!(state.previous_focused_window_id, Some(1));
        assert!(state.windows.get(&2).unwrap().is_focused);
        assert!(!state.windows.get(&1).unwrap().is_focused);
    }

    #[test]
    fn apply_window_focus_changed_to_none() {
        let mut state = DaemonState::default();
        state.windows.insert(1, make_window(1));
        state.focused_window_id = Some(1);

        let event = NiriEvent::WindowFocusChanged { id: None };
        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::None);
        assert_eq!(state.focused_window_id, None);
        assert_eq!(state.previous_focused_window_id, Some(1));
    }

    #[test]
    fn apply_window_focus_changed_does_not_update_previous_when_same() {
        let mut state = DaemonState::default();
        state.windows.insert(1, make_window(1));
        state.focused_window_id = Some(1);

        // "Focus" same window again
        let event = NiriEvent::WindowFocusChanged { id: Some(1) };
        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::None);
        assert_eq!(state.focused_window_id, Some(1));
        // previous should not be set since focus didn't change
        assert_eq!(state.previous_focused_window_id, None);
    }

    #[test]
    fn apply_workspace_activated_updates_active_state() {
        let mut state = DaemonState::default();
        state.workspaces.insert(1, make_workspace(1, "eDP-1", true));
        state
            .workspaces
            .insert(2, make_workspace(2, "eDP-1", false));
        state
            .workspaces
            .insert(3, make_workspace(3, "HDMI-A-1", true));

        let event = NiriEvent::WorkspaceActivated {
            id: 2,
            focused: true,
        };
        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::None);
        // Workspace 2 should be active, workspace 1 should not
        assert!(state.workspaces.get(&2).unwrap().is_active);
        assert!(!state.workspaces.get(&1).unwrap().is_active);
        // Workspace 3 on different output should be unaffected
        assert!(state.workspaces.get(&3).unwrap().is_active);
        assert_eq!(state.focused_output, Some("eDP-1".to_string()));
    }

    #[test]
    fn apply_workspace_activated_not_focused() {
        let mut state = DaemonState::default();
        state.workspaces.insert(1, make_workspace(1, "eDP-1", true));
        state
            .workspaces
            .insert(2, make_workspace(2, "eDP-1", false));
        state.focused_output = Some("HDMI-A-1".to_string());

        let event = NiriEvent::WorkspaceActivated {
            id: 2,
            focused: false,
        };
        apply_event(&mut state, &event);

        // focused_output should remain unchanged
        assert_eq!(state.focused_output, Some("HDMI-A-1".to_string()));
    }

    #[test]
    fn apply_workspaces_changed_returns_reload() {
        let mut state = DaemonState::default();
        let event = NiriEvent::WorkspacesChanged;
        let action = apply_event(&mut state, &event);
        assert_eq!(action, EventAction::ReloadWorkspaces);
    }

    #[test]
    fn apply_output_focus_changed() {
        let mut state = DaemonState::default();
        let event = NiriEvent::OutputFocusChanged {
            output: Some("HDMI-A-1".to_string()),
        };
        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::None);
        assert_eq!(state.focused_output, Some("HDMI-A-1".to_string()));
    }

    #[test]
    fn apply_outputs_changed_replaces_outputs() {
        let mut state = DaemonState::default();
        state.outputs.insert(
            "old".to_string(),
            OutputInfo {
                name: "old".to_string(),
                width: 100,
                height: 100,
            },
        );

        let mut new_outputs = HashMap::new();
        new_outputs.insert(
            "eDP-1".to_string(),
            OutputInfo {
                name: "eDP-1".to_string(),
                width: 1920,
                height: 1080,
            },
        );

        let event = NiriEvent::OutputsChanged(new_outputs.clone());
        let action = apply_event(&mut state, &event);

        assert_eq!(action, EventAction::None);
        assert_eq!(state.outputs.len(), 1);
        assert!(state.outputs.contains_key("eDP-1"));
        assert!(!state.outputs.contains_key("old"));
    }

    #[test]
    fn apply_window_focus_changed_updates_scratchpad_recency() {
        let mut state = DaemonState::default();
        state.windows.insert(42, make_window(42));
        state.register_scratchpad_window("term", 42);

        let before = state.scratchpads.get("term").unwrap().last_used;
        std::thread::sleep(std::time::Duration::from_millis(10));

        let event = NiriEvent::WindowFocusChanged { id: Some(42) };
        apply_event(&mut state, &event);

        let after = state.scratchpads.get("term").unwrap().last_used;
        assert!(after >= before);
    }
}
