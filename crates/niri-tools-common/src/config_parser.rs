use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use kdl::KdlDocument;
use kdl::KdlNode;

use crate::config::{
    DaemonSettings, NotifyLevel, OutputOverride, PositionConfig, ScratchpadConfig, SizeConfig,
};
use crate::error::NiriToolsError;

/// Result of loading and parsing a KDL configuration file.
#[derive(Debug, Default)]
pub struct LoadedConfig {
    pub settings: DaemonSettings,
    pub scratchpads: HashMap<String, ScratchpadConfig>,
    pub config_files: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// Load configuration from a KDL file.
///
/// If `config_path` is `None`, uses the default config path
/// (`~/.config/niri/scratchpads.kdl`).
///
/// Missing config files at the default path result in default settings with no
/// scratchpads. Missing config files at an explicit path return an error.
pub fn load_config(config_path: Option<&Path>) -> Result<LoadedConfig, NiriToolsError> {
    let path = match config_path {
        Some(p) => p.to_path_buf(),
        None => crate::paths::default_config_path(),
    };

    if !path.exists() {
        if config_path.is_some() {
            return Err(NiriToolsError::Config(format!(
                "Config file not found: {}",
                path.display()
            )));
        }
        // Default path missing → empty config
        return Ok(LoadedConfig::default());
    }

    let mut config = LoadedConfig::default();
    let mut visited = HashSet::new();

    load_file(&path, &mut visited, &mut config)?;

    Ok(config)
}

/// Load and parse a single KDL file, processing includes recursively.
fn load_file(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    config: &mut LoadedConfig,
) -> Result<(), NiriToolsError> {
    let resolved = path.canonicalize().map_err(|e| {
        NiriToolsError::Config(format!("Cannot resolve path {}: {e}", path.display()))
    })?;

    // Cycle detection
    if !visited.insert(resolved.clone()) {
        return Ok(());
    }

    config.config_files.push(resolved.clone());

    let content = std::fs::read_to_string(&resolved)
        .map_err(|e| NiriToolsError::Config(format!("Cannot read {}: {e}", resolved.display())))?;

    let doc: KdlDocument = content.parse().map_err(|e: kdl::KdlError| {
        NiriToolsError::Config(format!("KDL parse error in {}: {e}", resolved.display()))
    })?;

    let parent_dir = resolved.parent().unwrap_or(Path::new("."));

    parse_document(&doc, parent_dir, visited, config)?;

    Ok(())
}

/// Parse a KDL document, processing includes, settings, and scratchpads.
///
/// Nodes are processed in order. `include` nodes are processed inline,
/// so included content appears as defaults that later nodes in the same
/// file can override.
fn parse_document(
    doc: &KdlDocument,
    parent_dir: &Path,
    visited: &mut HashSet<PathBuf>,
    config: &mut LoadedConfig,
) -> Result<(), NiriToolsError> {
    for node in doc.nodes() {
        let name = node.name().value();
        match name {
            "include" => process_include(node, parent_dir, visited, config),
            "settings" => parse_settings(node, config),
            "scratchpad" => parse_scratchpad(node, config),
            _ => {
                // Unknown top-level nodes are silently ignored
            }
        }
    }
    Ok(())
}

/// Process an `include` node: resolve the path and recursively load the file.
fn process_include(
    node: &KdlNode,
    parent_dir: &Path,
    visited: &mut HashSet<PathBuf>,
    config: &mut LoadedConfig,
) {
    let Some(path_str) = node.get(0).and_then(|e| e.value().as_string()) else {
        config
            .warnings
            .push("include node missing path argument".to_string());
        return;
    };

    let include_path = parent_dir.join(path_str);

    if !include_path.exists() {
        config.warnings.push(format!(
            "Include file not found: {}",
            include_path.display()
        ));
        return;
    }

    if let Err(e) = load_file(&include_path, visited, config) {
        config.warnings.push(format!(
            "Error loading include {}: {e}",
            include_path.display()
        ));
    }
}

/// Parse a `settings` node into `DaemonSettings`.
fn parse_settings(node: &KdlNode, config: &mut LoadedConfig) {
    let Some(children) = node.children() else {
        return;
    };

    if let Some(notify_val) = children.get_arg("notify") {
        if let Some(level_str) = notify_val.as_string() {
            match level_str {
                "all" => config.settings.notify_level = NotifyLevel::All,
                "error" => config.settings.notify_level = NotifyLevel::Error,
                "warning" => config.settings.notify_level = NotifyLevel::Warning,
                "none" => config.settings.notify_level = NotifyLevel::None,
                other => {
                    config
                        .warnings
                        .push(format!("Unknown notify level \"{other}\". Valid values: all, error, warning, none. Using \"all\"."));
                    config.settings.notify_level = NotifyLevel::All;
                }
            }
        }
    }

    if let Some(watch_val) = children.get_arg("watch") {
        if let Some(b) = watch_val.as_bool() {
            config.settings.watch_config = b;
        } else if let Some(s) = watch_val.as_string() {
            config.settings.watch_config = s == "true";
        }
    }
}

/// Parse a `scratchpad` node into a `ScratchpadConfig`.
fn parse_scratchpad(node: &KdlNode, config: &mut LoadedConfig) {
    // Name is the first argument
    let Some(name) = node.get(0).and_then(|e| e.value().as_string()) else {
        config
            .warnings
            .push("Scratchpad node missing name argument".to_string());
        return;
    };
    let name = name.to_string();

    let Some(children) = node.children() else {
        config
            .warnings
            .push(format!("Scratchpad \"{name}\" has no body"));
        return;
    };

    // app-id is required
    let app_id = children
        .get_arg("app-id")
        .and_then(|v| v.as_string())
        .map(String::from);
    if app_id.is_none() {
        config.warnings.push(format!(
            "Scratchpad \"{name}\" missing required app-id, skipping"
        ));
        return;
    }

    // title (optional)
    let title = children
        .get_arg("title")
        .and_then(|v| v.as_string())
        .map(String::from);

    // command: all positional arguments of the `command` node
    let command = children.get("command").map(|cmd_node| {
        cmd_node
            .entries()
            .iter()
            .filter(|e| e.name().is_none())
            .filter_map(|e| e.value().as_string().map(String::from))
            .collect::<Vec<_>>()
    });

    // size (optional)
    let size = parse_size_from_doc(children);

    // position (optional)
    let position = parse_position_from_doc(children);

    // output overrides
    let mut output_overrides = HashMap::new();
    for child in children.nodes() {
        if child.name().value() == "output" {
            if let Some(output_name) = child.get(0).and_then(|e| e.value().as_string()) {
                let ov = parse_output_override(child);
                output_overrides.insert(output_name.to_string(), ov);
            }
        }
    }

    let scratchpad = ScratchpadConfig {
        name: name.clone(),
        command,
        app_id,
        title,
        size,
        position,
        output_overrides,
    };

    config.scratchpads.insert(name, scratchpad);
}

/// Parse a `size` child node from a KDL document (children block).
/// Looks for: `size width="60%" height="60%"`
fn parse_size_from_doc(doc: &KdlDocument) -> Option<SizeConfig> {
    let size_node = doc.get("size")?;
    parse_size_node(size_node)
}

fn parse_size_node(node: &KdlNode) -> Option<SizeConfig> {
    let width = node.get("width")?.value().as_string()?.to_string();
    let height = node.get("height")?.value().as_string()?.to_string();
    Some(SizeConfig { width, height })
}

/// Parse a `position` child node from a KDL document (children block).
/// Looks for: `position x="10%" y="35%"`
fn parse_position_from_doc(doc: &KdlDocument) -> Option<PositionConfig> {
    let pos_node = doc.get("position")?;
    parse_position_node(pos_node)
}

fn parse_position_node(node: &KdlNode) -> Option<PositionConfig> {
    let x = node.get("x")?.value().as_string()?.to_string();
    let y = node.get("y")?.value().as_string()?.to_string();
    Some(PositionConfig { x, y })
}

/// Parse an `output` override node within a scratchpad.
fn parse_output_override(node: &KdlNode) -> OutputOverride {
    let children = match node.children() {
        Some(c) => c,
        None => return OutputOverride::default(),
    };

    OutputOverride {
        size: parse_size_from_doc(children),
        position: parse_position_from_doc(children),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::config::NotifyLevel;

    /// Helper: write a KDL string to a tempfile and parse it via load_config.
    fn load_from_str(content: &str) -> Result<LoadedConfig, NiriToolsError> {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("scratchpads.kdl");
        fs::write(&path, content).unwrap();
        load_config(Some(&path))
    }

    // ── Settings parsing ──────────────────────────────────────────

    #[test]
    fn parse_settings_notify_all() {
        let cfg = load_from_str(r#"settings { notify "all"; }"#).unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::All);
    }

    #[test]
    fn parse_settings_notify_error() {
        let cfg = load_from_str(r#"settings { notify "error"; }"#).unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::Error);
    }

    #[test]
    fn parse_settings_notify_warning() {
        let cfg = load_from_str(r#"settings { notify "warning"; }"#).unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::Warning);
    }

    #[test]
    fn parse_settings_notify_none() {
        let cfg = load_from_str(r#"settings { notify "none"; }"#).unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::None);
    }

    #[test]
    fn parse_settings_watch_true() {
        let cfg = load_from_str("settings { watch true; }").unwrap();
        assert!(cfg.settings.watch_config);
    }

    #[test]
    fn parse_settings_watch_false() {
        let cfg = load_from_str("settings { watch false; }").unwrap();
        assert!(!cfg.settings.watch_config);
    }

    #[test]
    fn parse_settings_defaults_when_missing() {
        let cfg = load_from_str("").unwrap();
        assert_eq!(cfg.settings, DaemonSettings::default());
    }

    #[test]
    fn parse_settings_unknown_notify_level_warns_and_uses_default() {
        let cfg = load_from_str(r#"settings { notify "banana"; }"#).unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::All); // default
        assert!(cfg.warnings.iter().any(|w| w.contains("banana")));
    }

    // ── Scratchpad parsing ────────────────────────────────────────

    #[test]
    fn parse_scratchpad_all_fields() {
        let kdl = r#"
scratchpad "term" {
    app-id "com.mitchellh.ghostty"
    command "ghostty"
    size width="60%" height="60%"
    position x="10%" y="35%"
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        let sp = cfg
            .scratchpads
            .get("term")
            .expect("scratchpad 'term' not found");
        assert_eq!(sp.name, "term");
        assert_eq!(sp.app_id.as_deref(), Some("com.mitchellh.ghostty"));
        assert_eq!(sp.command, Some(vec!["ghostty".to_string()]));
        let size = sp.size.as_ref().unwrap();
        assert_eq!(size.width, "60%");
        assert_eq!(size.height, "60%");
        let pos = sp.position.as_ref().unwrap();
        assert_eq!(pos.x, "10%");
        assert_eq!(pos.y, "35%");
    }

    #[test]
    fn parse_scratchpad_multi_arg_command() {
        let kdl = r#"
scratchpad "dms" {
    app-id "org.quickshell"
    command "dms" "ipc" "call" "settings" "open"
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        let sp = cfg.scratchpads.get("dms").unwrap();
        assert_eq!(
            sp.command,
            Some(vec![
                "dms".to_string(),
                "ipc".to_string(),
                "call".to_string(),
                "settings".to_string(),
                "open".to_string(),
            ])
        );
    }

    #[test]
    fn parse_scratchpad_title_field() {
        let kdl = r#"
scratchpad "settings" {
    app-id "org.quickshell"
    title "^Settings$"
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        let sp = cfg.scratchpads.get("settings").unwrap();
        assert_eq!(sp.title.as_deref(), Some("^Settings$"));
    }

    #[test]
    fn parse_scratchpad_output_overrides() {
        let kdl = r#"
scratchpad "term" {
    app-id "ghostty"
    size width="60%" height="60%"
    position x="10%" y="35%"

    output "DP-2" {
        position x="50%" y="35%"
    }
    output "eDP-1" {
        size width="80%" height="80%"
    }
    output "HDMI-1" {
        size width="40%" height="40%"
        position x="30%" y="30%"
    }
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        let sp = cfg.scratchpads.get("term").unwrap();
        assert_eq!(sp.output_overrides.len(), 3);

        // DP-2: position only
        let dp2 = &sp.output_overrides["DP-2"];
        assert!(dp2.size.is_none());
        let dp2_pos = dp2.position.as_ref().unwrap();
        assert_eq!(dp2_pos.x, "50%");
        assert_eq!(dp2_pos.y, "35%");

        // eDP-1: size only
        let edp = &sp.output_overrides["eDP-1"];
        assert!(edp.position.is_none());
        let edp_size = edp.size.as_ref().unwrap();
        assert_eq!(edp_size.width, "80%");
        assert_eq!(edp_size.height, "80%");

        // HDMI-1: both
        let hdmi = &sp.output_overrides["HDMI-1"];
        assert!(hdmi.size.is_some());
        assert!(hdmi.position.is_some());
    }

    #[test]
    fn parse_scratchpad_without_name_warns_and_skips() {
        let kdl = r#"
scratchpad {
    app-id "ghostty"
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        assert!(cfg.scratchpads.is_empty());
        assert!(cfg.warnings.iter().any(|w| w.contains("name")));
    }

    #[test]
    fn parse_scratchpad_without_app_id_warns_and_skips() {
        let kdl = r#"
scratchpad "orphan" {
    command "foo"
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        assert!(cfg.scratchpads.is_empty());
        assert!(cfg.warnings.iter().any(|w| w.contains("app-id")));
    }

    // ── Include resolution ────────────────────────────────────────

    #[test]
    fn include_resolves_relative_paths() {
        let dir = TempDir::new().unwrap();

        let main_content = r#"include "./extra.kdl""#;
        let extra_content = r#"
scratchpad "included" {
    app-id "test.included"
}
"#;

        fs::write(dir.path().join("main.kdl"), main_content).unwrap();
        fs::write(dir.path().join("extra.kdl"), extra_content).unwrap();

        let cfg = load_config(Some(&dir.path().join("main.kdl"))).unwrap();
        assert!(cfg.scratchpads.contains_key("included"));
        assert_eq!(cfg.config_files.len(), 2);
    }

    #[test]
    fn include_cycle_detection() {
        let dir = TempDir::new().unwrap();

        // A includes B, B includes A
        fs::write(dir.path().join("a.kdl"), r#"include "./b.kdl""#).unwrap();
        fs::write(dir.path().join("b.kdl"), r#"include "./a.kdl""#).unwrap();

        let cfg = load_config(Some(&dir.path().join("a.kdl"))).unwrap();
        // Should not infinite loop and should have 2 files
        assert_eq!(cfg.config_files.len(), 2);
    }

    #[test]
    fn include_missing_file_warns_no_error() {
        let dir = TempDir::new().unwrap();
        let main_content = r#"include "./nonexistent.kdl""#;
        fs::write(dir.path().join("main.kdl"), main_content).unwrap();

        let cfg = load_config(Some(&dir.path().join("main.kdl"))).unwrap();
        assert!(cfg.warnings.iter().any(|w| w.contains("nonexistent.kdl")));
    }

    #[test]
    fn include_merges_with_main_overriding() {
        let dir = TempDir::new().unwrap();

        let included = r#"
settings {
    notify "error"
    watch #false
}
scratchpad "term" {
    app-id "foot"
    size width="40%" height="40%"
}
"#;
        let main = r#"
include "./included.kdl"
settings {
    notify "all"
}
scratchpad "term" {
    app-id "ghostty"
    size width="60%" height="60%"
}
"#;

        fs::write(dir.path().join("included.kdl"), included).unwrap();
        fs::write(dir.path().join("main.kdl"), main).unwrap();

        let cfg = load_config(Some(&dir.path().join("main.kdl"))).unwrap();
        // Main file overrides included
        assert_eq!(cfg.settings.notify_level, NotifyLevel::All);
        let sp = cfg.scratchpads.get("term").unwrap();
        assert_eq!(sp.app_id.as_deref(), Some("ghostty"));
        assert_eq!(sp.size.as_ref().unwrap().width, "60%");
    }

    // ── File handling ─────────────────────────────────────────────

    #[test]
    fn missing_config_at_explicit_path_returns_error() {
        let cfg = load_config(Some(Path::new("/tmp/definitely-does-not-exist-niri.kdl")));
        assert!(cfg.is_err());
    }

    #[test]
    fn empty_config_file_returns_defaults() {
        let cfg = load_from_str("").unwrap();
        assert_eq!(cfg.settings, DaemonSettings::default());
        assert!(cfg.scratchpads.is_empty());
        assert!(cfg.warnings.is_empty());
    }

    #[test]
    fn invalid_kdl_returns_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.kdl");
        fs::write(&path, "this is {{{{ not valid kdl").unwrap();
        let result = load_config(Some(&path));
        assert!(result.is_err());
        match result.unwrap_err() {
            NiriToolsError::Config(msg) => assert!(!msg.is_empty()),
            other => panic!("Expected Config error, got: {other:?}"),
        }
    }

    #[test]
    fn config_files_tracks_loaded_files() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.kdl");
        fs::write(&path, "").unwrap();
        let cfg = load_config(Some(&path)).unwrap();
        assert_eq!(cfg.config_files.len(), 1);
        assert_eq!(cfg.config_files[0], path.canonicalize().unwrap());
    }

    // ── Complete config ───────────────────────────────────────────

    #[test]
    fn parse_complete_config() {
        let kdl = r#"
settings {
    notify "all"
    watch true
}

scratchpad "term" {
    app-id "com.mitchellh.ghostty"
    command "ghostty"
    size width="60%" height="60%"
    position x="10%" y="35%"

    output "DP-2" {
        position x="50%" y="35%"
    }
}

scratchpad "dms-settings" {
    app-id "org.quickshell"
    title "^Settings$"
    command "dms" "ipc" "call" "settings" "open"
    size width="40%" height="60%"
    position x="10%" y="35%"

    output "DP-2" {
        position x="50%" y="35%"
    }
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::All);
        assert!(cfg.settings.watch_config);
        assert_eq!(cfg.scratchpads.len(), 2);
        assert!(cfg.scratchpads.contains_key("term"));
        assert!(cfg.scratchpads.contains_key("dms-settings"));
        assert!(cfg.warnings.is_empty());
    }

    // ── paths::default_config_path ────────────────────────────────

    #[test]
    fn default_config_path_uses_xdg_config_home() {
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/custom/config") };
        let path = crate::paths::default_config_path();
        assert_eq!(path, PathBuf::from("/custom/config/niri/scratchpads.kdl"));
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }

    #[test]
    fn default_config_path_falls_back_to_home_dotconfig() {
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        unsafe { std::env::set_var("HOME", "/home/testuser") };
        let path = crate::paths::default_config_path();
        assert_eq!(
            path,
            PathBuf::from("/home/testuser/.config/niri/scratchpads.kdl")
        );
    }
}
