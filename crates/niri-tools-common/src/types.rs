use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    pub id: u64,
    pub app_id: String,
    pub title: String,
    pub workspace_id: Option<u64>,
    pub is_focused: bool,
    pub is_floating: bool,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: u64,
    pub idx: u32,
    pub output: String,
    pub is_active: bool,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NiriEvent {
    WindowOpenedOrChanged(WindowInfo),
    WindowsChanged(Vec<WindowInfo>),
    WindowClosed { id: u64 },
    WindowFocusChanged { id: Option<u64> },
    WorkspaceActivated { id: u64, focused: bool },
    WorkspacesChanged,
    OutputFocusChanged { output: Option<String> },
    OutputsChanged(HashMap<String, OutputInfo>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_window() -> WindowInfo {
        WindowInfo {
            id: 1,
            app_id: "foot".to_string(),
            title: "Terminal".to_string(),
            workspace_id: Some(10),
            is_focused: true,
            is_floating: false,
            width: 800,
            height: 600,
        }
    }

    fn sample_workspace() -> WorkspaceInfo {
        WorkspaceInfo {
            id: 10,
            idx: 1,
            output: "eDP-1".to_string(),
            is_active: true,
            name: Some("main".to_string()),
        }
    }

    fn sample_output() -> OutputInfo {
        OutputInfo {
            name: "eDP-1".to_string(),
            width: 1920,
            height: 1080,
        }
    }

    // -- WindowInfo tests --

    #[test]
    fn window_info_serialization_roundtrip() {
        let window = sample_window();
        let json = serde_json::to_string(&window).unwrap();
        let decoded: WindowInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(window, decoded);
    }

    #[test]
    fn window_info_bincode_roundtrip() {
        let window = sample_window();
        let encoded = bincode::serde::encode_to_vec(&window, bincode::config::standard()).unwrap();
        let (decoded, _): (WindowInfo, _) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(window, decoded);
    }

    #[test]
    fn window_info_without_workspace() {
        let window = WindowInfo {
            workspace_id: None,
            ..sample_window()
        };
        let json = serde_json::to_string(&window).unwrap();
        let decoded: WindowInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(window, decoded);
        assert_eq!(decoded.workspace_id, None);
    }

    // -- WorkspaceInfo tests --

    #[test]
    fn workspace_info_serialization_roundtrip() {
        let ws = sample_workspace();
        let json = serde_json::to_string(&ws).unwrap();
        let decoded: WorkspaceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(ws, decoded);
    }

    #[test]
    fn workspace_info_bincode_roundtrip() {
        let ws = sample_workspace();
        let encoded = bincode::serde::encode_to_vec(&ws, bincode::config::standard()).unwrap();
        let (decoded, _): (WorkspaceInfo, _) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(ws, decoded);
    }

    #[test]
    fn workspace_info_without_name() {
        let ws = WorkspaceInfo {
            name: None,
            ..sample_workspace()
        };
        let json = serde_json::to_string(&ws).unwrap();
        let decoded: WorkspaceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.name, None);
    }

    // -- OutputInfo tests --

    #[test]
    fn output_info_serialization_roundtrip() {
        let output = sample_output();
        let json = serde_json::to_string(&output).unwrap();
        let decoded: OutputInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(output, decoded);
    }

    #[test]
    fn output_info_bincode_roundtrip() {
        let output = sample_output();
        let encoded = bincode::serde::encode_to_vec(&output, bincode::config::standard()).unwrap();
        let (decoded, _): (OutputInfo, _) =
            bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(output, decoded);
    }

    // -- NiriEvent tests --

    #[test]
    fn niri_event_window_opened_or_changed() {
        let event = NiriEvent::WindowOpenedOrChanged(sample_window());
        if let NiriEvent::WindowOpenedOrChanged(w) = event {
            assert_eq!(w.id, 1);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn niri_event_windows_changed() {
        let event = NiriEvent::WindowsChanged(vec![sample_window()]);
        if let NiriEvent::WindowsChanged(ws) = event {
            assert_eq!(ws.len(), 1);
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn niri_event_window_closed() {
        let event = NiriEvent::WindowClosed { id: 42 };
        assert_eq!(event, NiriEvent::WindowClosed { id: 42 });
    }

    #[test]
    fn niri_event_window_focus_changed() {
        let event = NiriEvent::WindowFocusChanged { id: Some(1) };
        assert_eq!(event, NiriEvent::WindowFocusChanged { id: Some(1) });

        let event_none = NiriEvent::WindowFocusChanged { id: None };
        assert_eq!(event_none, NiriEvent::WindowFocusChanged { id: None });
    }

    #[test]
    fn niri_event_workspace_activated() {
        let event = NiriEvent::WorkspaceActivated {
            id: 5,
            focused: true,
        };
        assert_eq!(
            event,
            NiriEvent::WorkspaceActivated {
                id: 5,
                focused: true
            }
        );
    }

    #[test]
    fn niri_event_workspaces_changed() {
        let event = NiriEvent::WorkspacesChanged;
        assert_eq!(event, NiriEvent::WorkspacesChanged);
    }

    #[test]
    fn niri_event_output_focus_changed() {
        let event = NiriEvent::OutputFocusChanged {
            output: Some("HDMI-A-1".to_string()),
        };
        if let NiriEvent::OutputFocusChanged { output } = event {
            assert_eq!(output.as_deref(), Some("HDMI-A-1"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn niri_event_outputs_changed() {
        let mut outputs = HashMap::new();
        outputs.insert("eDP-1".to_string(), sample_output());
        let event = NiriEvent::OutputsChanged(outputs);
        if let NiriEvent::OutputsChanged(map) = event {
            assert_eq!(map.len(), 1);
            assert!(map.contains_key("eDP-1"));
        } else {
            panic!("wrong variant");
        }
    }
}
