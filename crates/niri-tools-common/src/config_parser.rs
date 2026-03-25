use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use kdl::KdlDocument;
use kdl::KdlNode;

use crate::config::{
    BindAction, BindConfig, BindOption, DaemonSettings, ModeConfig, ModesUiConfig, NotifyLevel,
    OutputOverride, PositionConfig, ScratchpadConfig, ScratchpadsUiConfig, SizeConfig, UiConfig,
};
use crate::error::NiriToolsError;

/// Result of loading and parsing a KDL configuration file.
#[derive(Debug, Default)]
pub struct LoadedConfig {
    pub settings: DaemonSettings,
    pub scratchpads: HashMap<String, ScratchpadConfig>,
    pub modes: HashMap<String, ModeConfig>,
    pub ui_config: UiConfig,
    pub config_files: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

/// Load configuration from a KDL file.
///
/// If `config_path` is `None`, uses the default config path
/// (`~/.config/niri/niri-tools.kdl`).
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

    // Validate after all parsing
    validate_config(&mut config);

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
            "notifications" => parse_notifications(node, config),
            "scratchpad" => parse_scratchpad(node, config),
            "mode" => parse_mode(node, config),
            "ui" => parse_ui(node, config),
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

    // auto-adopt (optional, default false)
    let auto_adopt = children
        .get_arg("auto-adopt")
        .map(|v| {
            if let Some(b) = v.as_bool() {
                b
            } else if let Some(s) = v.as_string() {
                s == "true"
            } else {
                false
            }
        })
        .unwrap_or(false);

    // key (optional) - shortcut key in picker
    let key = children
        .get_arg("key")
        .and_then(|v| v.as_string())
        .map(String::from);

    // desc (optional) - display name in picker
    let desc = children
        .get_arg("desc")
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
        auto_adopt,
        key,
        desc,
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

/// Parse a top-level `notifications` node.
///
/// The `notifications` node takes a single string argument: the notify level.
/// This takes precedence over `settings { notify "..." }` if both are present.
fn parse_notifications(node: &KdlNode, config: &mut LoadedConfig) {
    if let Some(level_str) = node.get(0).and_then(|e| e.value().as_string()) {
        config.settings.notify_level = match level_str {
            "none" => NotifyLevel::None,
            "error" => NotifyLevel::Error,
            "warning" => NotifyLevel::Warning,
            "all" => NotifyLevel::All,
            other => {
                config
                    .warnings
                    .push(format!("Unknown notification level: {other}"));
                NotifyLevel::All
            }
        };
    }
}

/// Parse a top-level `mode` node into a `ModeConfig`.
fn parse_mode(node: &KdlNode, config: &mut LoadedConfig) {
    let Some(name) = node.get(0).and_then(|e| e.value().as_string()) else {
        config
            .warnings
            .push("Mode node missing name argument".to_string());
        return;
    };
    let name = name.to_string();

    let Some(children) = node.children() else {
        config.warnings.push(format!("Mode \"{name}\" has no body"));
        return;
    };

    // Check for keep-open flag
    let keep_open = children.get("keep-open").is_some();

    // Parse binds block
    let binds = if let Some(binds_node) = children.get("binds") {
        parse_binds(binds_node, config)
    } else {
        config
            .warnings
            .push(format!("Mode \"{name}\" has no binds block"));
        Vec::new()
    };

    let mode = ModeConfig {
        name: name.clone(),
        keep_open,
        binds,
    };
    config.modes.insert(name, mode);
}

/// Parse a `binds` block inside a mode into a `Vec<BindConfig>`.
fn parse_binds(node: &KdlNode, config: &mut LoadedConfig) -> Vec<BindConfig> {
    let Some(children) = node.children() else {
        return Vec::new();
    };

    let mut binds = Vec::new();
    for bind_node in children.nodes() {
        let key = bind_node.name().value().to_string();

        // Description is the first positional argument
        let description = bind_node
            .get(0)
            .and_then(|e| e.value().as_string())
            .unwrap_or("")
            .to_string();

        let (options, action) = parse_bind_children(bind_node, config);

        if let Some(action) = action {
            binds.push(BindConfig {
                key,
                description,
                options,
                action,
            });
        }
    }
    binds
}

/// Parse the children of a bind node into options and an action.
fn parse_bind_children(
    node: &KdlNode,
    _config: &mut LoadedConfig,
) -> (Vec<BindOption>, Option<BindAction>) {
    let Some(children) = node.children() else {
        return (Vec::new(), None);
    };

    let mut options = Vec::new();
    let mut action = None;

    for child in children.nodes() {
        let name = child.name().value();
        match name {
            "keep-open" => options.push(BindOption::KeepOpen),
            "close" => options.push(BindOption::Close),
            "alias" => {
                if let Some(alias_str) = child.get(0).and_then(|e| e.value().as_string()) {
                    options.push(BindOption::Alias(alias_str.to_string()));
                }
            }
            "spawn-sh" => {
                if let Some(cmd) = child.get(0).and_then(|e| e.value().as_string()) {
                    action = Some(BindAction::SpawnSh(cmd.to_string()));
                }
            }
            "spawn" => {
                let args: Vec<String> = child
                    .entries()
                    .iter()
                    .filter(|e| e.name().is_none())
                    .filter_map(|e| e.value().as_string().map(String::from))
                    .collect();
                action = Some(BindAction::Spawn(args));
            }
            "switch-mode" => {
                if let Some(mode_name) = child.get(0).and_then(|e| e.value().as_string()) {
                    action = Some(BindAction::SwitchMode(mode_name.to_string()));
                }
            }
            "scratchpad-pick" => action = Some(BindAction::ScratchpadPick),
            "scratchpad-toggle" => {
                let name = child
                    .get(0)
                    .and_then(|e| e.value().as_string().map(String::from));
                action = Some(BindAction::ScratchpadToggle(name));
            }
            "scratchpad-hide" => action = Some(BindAction::ScratchpadHide),
            "scratchpad-float" => {
                let name = child
                    .get(0)
                    .and_then(|e| e.value().as_string().map(String::from));
                action = Some(BindAction::ScratchpadFloat(name));
            }
            "scratchpad-tile" => {
                let name = child
                    .get(0)
                    .and_then(|e| e.value().as_string().map(String::from));
                action = Some(BindAction::ScratchpadTile(name));
            }
            "scratchpad-toggle-float" => action = Some(BindAction::ScratchpadToggleFloat),
            "scratchpad-adopt" => action = Some(BindAction::ScratchpadAdopt),
            "scratchpad-disown" => action = Some(BindAction::ScratchpadDisown),
            // Unknown action names are treated as niri action pass-through
            other => {
                let args: Vec<String> = child
                    .entries()
                    .iter()
                    .filter(|e| e.name().is_none())
                    .filter_map(|e| e.value().as_string().map(String::from))
                    .collect();
                action = Some(BindAction::NiriAction {
                    name: other.to_string(),
                    args,
                });
            }
        }
    }
    (options, action)
}

/// Parse a top-level `ui` node into `UiConfig`.
fn parse_ui(node: &KdlNode, config: &mut LoadedConfig) {
    let Some(children) = node.children() else {
        return;
    };

    // Global UI properties
    config.ui_config.font = children
        .get_arg("font")
        .and_then(|v| v.as_string())
        .map(String::from);
    config.ui_config.background_color = children
        .get_arg("background-color")
        .and_then(|v| v.as_string())
        .map(String::from);
    config.ui_config.color = children
        .get_arg("color")
        .and_then(|v| v.as_string())
        .map(String::from);
    config.ui_config.corner_radius = children
        .get_arg("corner-radius")
        .and_then(|v| v.as_i64().map(|i| i as f64).or_else(|| v.as_f64()));

    // Sub-blocks
    if let Some(modes_node) = children.get("modes") {
        config.ui_config.modes = parse_modes_ui(modes_node);
    }
    if let Some(sp_node) = children.get("scratchpads") {
        config.ui_config.scratchpads = parse_scratchpads_ui(sp_node);
    }
}

/// Parse a `modes` sub-block inside `ui`.
fn parse_modes_ui(node: &KdlNode) -> ModesUiConfig {
    let Some(children) = node.children() else {
        return ModesUiConfig::default();
    };

    ModesUiConfig {
        font: children
            .get_arg("font")
            .and_then(|v| v.as_string())
            .map(String::from),
        background_color: children
            .get_arg("background-color")
            .and_then(|v| v.as_string())
            .map(String::from),
        color: children
            .get_arg("color")
            .and_then(|v| v.as_string())
            .map(String::from),
        corner_radius: children
            .get_arg("corner-radius")
            .and_then(|v| v.as_i64().map(|i| i as f64).or_else(|| v.as_f64())),
        anchor: children
            .get_arg("anchor")
            .and_then(|v| v.as_string())
            .map(String::from),
        separator: children
            .get_arg("separator")
            .and_then(|v| v.as_string())
            .map(String::from),
        margin_top: children
            .get_arg("margin-top")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        margin_right: children
            .get_arg("margin-right")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        margin_bottom: children
            .get_arg("margin-bottom")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        margin_left: children
            .get_arg("margin-left")
            .and_then(|v| v.as_i64())
            .map(|i| i as i32),
        padding: children
            .get_arg("padding")
            .and_then(|v| v.as_i64().map(|i| i as f64).or_else(|| v.as_f64())),
        column_padding: children
            .get_arg("column-padding")
            .and_then(|v| v.as_i64().map(|i| i as f64).or_else(|| v.as_f64())),
        min_width: children
            .get_arg("min-width")
            .and_then(|v| v.as_i64().map(|i| i as f64).or_else(|| v.as_f64())),
        border_width: children
            .get_arg("border-width")
            .and_then(|v| v.as_i64().map(|i| i as f64).or_else(|| v.as_f64())),
    }
}

/// Validate the fully-loaded config and emit warnings for issues.
fn validate_config(config: &mut LoadedConfig) {
    let mode_names: HashSet<&str> = config.modes.keys().map(|s| s.as_str()).collect();

    for mode in config.modes.values() {
        // Check for duplicate keys within a mode
        let mut seen_keys: HashSet<&str> = HashSet::new();
        for bind in &mode.binds {
            if !seen_keys.insert(&bind.key) {
                config.warnings.push(format!(
                    "Mode \"{}\": duplicate key \"{}\"",
                    mode.name, bind.key
                ));
            }
            // Also check aliases
            for opt in &bind.options {
                if let BindOption::Alias(alias) = opt {
                    if !seen_keys.insert(alias.as_str()) {
                        config.warnings.push(format!(
                            "Mode \"{}\": duplicate key/alias \"{}\"",
                            mode.name, alias
                        ));
                    }
                }
            }
        }

        // Check for invalid switch-mode references
        for bind in &mode.binds {
            if let BindAction::SwitchMode(ref target) = bind.action {
                if !mode_names.contains(target.as_str()) {
                    config.warnings.push(format!(
                        "Mode \"{}\": bind \"{}\" references undefined mode \"{}\"",
                        mode.name, bind.key, target
                    ));
                }
            }
        }
    }

    // Check for scratchpad key conflicts
    let mut key_owners: HashMap<&str, &str> = HashMap::new();
    for sp in config.scratchpads.values() {
        if let Some(ref key) = sp.key {
            if let Some(existing) = key_owners.get(key.as_str()) {
                config.warnings.push(format!(
                    "Scratchpad key \"{}\" conflict: used by both \"{}\" and \"{}\"",
                    key, existing, sp.name
                ));
            } else {
                key_owners.insert(key.as_str(), &sp.name);
            }
        }
    }
}

/// Parse a `scratchpads` sub-block inside `ui`.
fn parse_scratchpads_ui(node: &KdlNode) -> ScratchpadsUiConfig {
    let Some(children) = node.children() else {
        return ScratchpadsUiConfig::default();
    };

    ScratchpadsUiConfig {
        font: children
            .get_arg("font")
            .and_then(|v| v.as_string())
            .map(String::from),
        background_color: children
            .get_arg("background-color")
            .and_then(|v| v.as_string())
            .map(String::from),
        color: children
            .get_arg("color")
            .and_then(|v| v.as_string())
            .map(String::from),
        corner_radius: children
            .get_arg("corner-radius")
            .and_then(|v| v.as_i64().map(|i| i as f64).or_else(|| v.as_f64())),
        anchor: children
            .get_arg("anchor")
            .and_then(|v| v.as_string())
            .map(String::from),
        padding: children
            .get_arg("padding")
            .and_then(|v| v.as_i64().map(|i| i as f64).or_else(|| v.as_f64())),
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
        let path = dir.path().join("niri-tools.kdl");
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
        // auto_adopt defaults to false
        assert!(!cfg.scratchpads.get("term").unwrap().auto_adopt);
        assert!(!cfg.scratchpads.get("dms-settings").unwrap().auto_adopt);
        assert!(cfg.warnings.is_empty());
    }

    // ── key/desc on scratchpads ──────────────────────────────────

    #[test]
    fn parse_scratchpad_with_key_and_desc() {
        let cfg = load_from_str(
            r#"
scratchpad "term" {
    key "t"
    desc "Terminal"
    app-id "com.mitchellh.ghostty"
    command "ghostty"
}
"#,
        )
        .unwrap();
        let sp = &cfg.scratchpads["term"];
        assert_eq!(sp.key.as_deref(), Some("t"));
        assert_eq!(sp.desc.as_deref(), Some("Terminal"));
    }

    // ── notifications top-level node ──────────────────────────────

    #[test]
    fn parse_notifications_warning() {
        let cfg = load_from_str(r#"notifications "warning""#).unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::Warning);
    }

    #[test]
    fn parse_notifications_none() {
        let cfg = load_from_str(r#"notifications "none""#).unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::None);
    }

    #[test]
    fn parse_notifications_overrides_settings() {
        let cfg = load_from_str(
            r#"
settings { notify "all"; }
notifications "error"
"#,
        )
        .unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::Error);
    }

    #[test]
    fn parse_notifications_unknown_level_warns() {
        let cfg = load_from_str(r#"notifications "banana""#).unwrap();
        assert_eq!(cfg.settings.notify_level, NotifyLevel::All);
        assert!(cfg.warnings.iter().any(|w| w.contains("banana")));
    }

    // ── UI config parsing ─────────────────────────────────────────

    #[test]
    fn parse_ui_config() {
        let cfg = load_from_str(
            r##"
ui {
    font "Mono 12"
    background-color "#282828"
    color "#fbf1c7"
    corner-radius 4
    modes {
        anchor "bottom"
        separator "  "
        margin-bottom -33
        padding 4
        column-padding 50
        min-width 1000
    }
    scratchpads {
        anchor "center"
        padding 12
    }
}
"##,
        )
        .unwrap();
        assert_eq!(cfg.ui_config.font.as_deref(), Some("Mono 12"));
        assert_eq!(cfg.ui_config.background_color.as_deref(), Some("#282828"));
        assert_eq!(cfg.ui_config.color.as_deref(), Some("#fbf1c7"));
        assert_eq!(cfg.ui_config.corner_radius, Some(4.0));
        assert_eq!(cfg.ui_config.modes.anchor.as_deref(), Some("bottom"));
        assert_eq!(cfg.ui_config.modes.separator.as_deref(), Some("  "));
        assert_eq!(cfg.ui_config.modes.margin_bottom, Some(-33));
        assert_eq!(cfg.ui_config.modes.padding, Some(4.0));
        assert_eq!(cfg.ui_config.modes.column_padding, Some(50.0));
        assert_eq!(cfg.ui_config.modes.min_width, Some(1000.0));
        assert_eq!(cfg.ui_config.scratchpads.anchor.as_deref(), Some("center"));
        assert_eq!(cfg.ui_config.scratchpads.padding, Some(12.0));
    }

    // ── Mode config parsing ───────────────────────────────────────

    #[test]
    fn parse_mode_config() {
        let cfg = load_from_str(
            r#"
mode "root" {
    binds {
        Space "Launcher" { spawn-sh "rofi -show drun"; }
        o "Open" { switch-mode "open"; }
        b "Brightness" { switch-mode "brightness"; }
    }
}
mode "brightness" {
    keep-open
    binds {
        j "-5" { keep-open; spawn-sh "brightness -5"; }
        k "+5" { spawn-sh "brightness +5"; }
        "?" "Query" { alias "q"; spawn-sh "brightness -q"; }
    }
}
"#,
        )
        .unwrap();

        // Root mode
        let root = &cfg.modes["root"];
        assert_eq!(root.binds.len(), 3);
        assert_eq!(root.binds[0].key, "Space");
        assert_eq!(root.binds[0].description, "Launcher");
        assert!(matches!(
            root.binds[0].action,
            BindAction::SpawnSh(ref s) if s == "rofi -show drun"
        ));
        assert!(matches!(
            root.binds[1].action,
            BindAction::SwitchMode(ref s) if s == "open"
        ));
        assert!(!root.keep_open);

        // Brightness mode
        let bright = &cfg.modes["brightness"];
        assert!(bright.keep_open);
        assert_eq!(bright.binds.len(), 3);
        assert_eq!(bright.binds[0].key, "j");
        assert!(bright.binds[0].options.contains(&BindOption::KeepOpen));
        assert_eq!(bright.binds[2].key, "?");
        assert!(bright.binds[2]
            .options
            .iter()
            .any(|o| matches!(o, BindOption::Alias(s) if s == "q")));
    }

    #[test]
    fn parse_niri_action_passthrough() {
        let cfg = load_from_str(
            r#"
mode "resize" {
    binds {
        "5" "50%" { set-window-width "50%"; }
        e "Expand" { expand-column-to-available-width; }
    }
}
"#,
        )
        .unwrap();
        let resize = &cfg.modes["resize"];
        assert!(matches!(
            &resize.binds[0].action,
            BindAction::NiriAction { name, args }
            if name == "set-window-width" && args == &["50%"]
        ));
        assert!(matches!(
            &resize.binds[1].action,
            BindAction::NiriAction { name, args }
            if name == "expand-column-to-available-width" && args.is_empty()
        ));
    }

    #[test]
    fn parse_duplicate_mode_name_overrides() {
        let cfg = load_from_str(
            r#"
mode "root" {
    binds {
        a "A" { spawn-sh "echo a"; }
    }
}
mode "root" {
    binds {
        b "B" { spawn-sh "echo b"; }
    }
}
"#,
        )
        .unwrap();
        // Second definition overrides first
        assert_eq!(cfg.modes["root"].binds[0].key, "b");
    }

    // ── match field parsing ─────────────────────────────────────

    #[test]
    fn parse_scratchpad_auto_adopt_true() {
        let kdl = r#"
scratchpad "browser" {
    app-id "firefox"
    auto-adopt true
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        let sp = cfg.scratchpads.get("browser").unwrap();
        assert!(sp.auto_adopt);
    }

    #[test]
    fn parse_scratchpad_auto_adopt_false() {
        let kdl = r#"
scratchpad "term" {
    app-id "ghostty"
    auto-adopt false
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        let sp = cfg.scratchpads.get("term").unwrap();
        assert!(!sp.auto_adopt);
    }

    #[test]
    fn parse_scratchpad_auto_adopt_defaults_to_false() {
        let kdl = r#"
scratchpad "term" {
    app-id "ghostty"
    command "ghostty"
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        let sp = cfg.scratchpads.get("term").unwrap();
        assert!(!sp.auto_adopt);
    }

    #[test]
    fn parse_scratchpad_auto_adopt_with_all_fields() {
        let kdl = r#"
scratchpad "browser" {
    app-id "firefox"
    title "^Mozilla.*"
    auto-adopt true
    size width="80%" height="80%"
    position x="10%" y="10%"
}
"#;
        let cfg = load_from_str(kdl).unwrap();
        let sp = cfg.scratchpads.get("browser").unwrap();
        assert!(sp.auto_adopt);
        assert_eq!(sp.app_id.as_deref(), Some("firefox"));
        assert_eq!(sp.title.as_deref(), Some("^Mozilla.*"));
        assert!(sp.size.is_some());
        assert!(sp.position.is_some());
        assert!(sp.command.is_none());
    }

    // ── Validation ─────────────────────────────────────────────────

    #[test]
    fn validate_warns_on_invalid_switch_mode_reference() {
        let cfg = load_from_str(
            r#"
mode "root" {
    binds {
        b "Bad" { switch-mode "nonexistent"; }
    }
}
"#,
        )
        .unwrap();
        assert!(cfg
            .warnings
            .iter()
            .any(|w| w.contains("nonexistent") && w.contains("undefined mode")));
    }

    #[test]
    fn validate_warns_on_duplicate_keys_in_mode() {
        let cfg = load_from_str(
            r#"
mode "root" {
    binds {
        a "First" { spawn-sh "echo 1"; }
        a "Second" { spawn-sh "echo 2"; }
    }
}
"#,
        )
        .unwrap();
        assert!(cfg
            .warnings
            .iter()
            .any(|w| w.contains("duplicate key") && w.contains("\"a\"")));
    }

    #[test]
    fn validate_warns_on_scratchpad_key_conflict() {
        let cfg = load_from_str(
            r#"
scratchpad "term" {
    app-id "ghostty"
    key "t"
}
scratchpad "todo" {
    app-id "todoist"
    key "t"
}
"#,
        )
        .unwrap();
        assert!(cfg
            .warnings
            .iter()
            .any(|w| w.contains("key \"t\" conflict")));
    }

    #[test]
    fn validate_no_warnings_for_valid_config() {
        let cfg = load_from_str(
            r#"
mode "root" {
    binds {
        a "A" { spawn-sh "echo a"; }
        b "B" { switch-mode "sub"; }
    }
}
mode "sub" {
    binds {
        x "X" { spawn-sh "echo x"; }
    }
}
scratchpad "term" {
    app-id "ghostty"
    key "t"
}
scratchpad "browser" {
    app-id "firefox"
    key "b"
}
"#,
        )
        .unwrap();
        assert!(
            cfg.warnings.is_empty(),
            "unexpected warnings: {:?}",
            cfg.warnings
        );
    }

    // ── paths::default_config_path ────────────────────────────────

    /// Combined into a single test to avoid env-var race conditions
    /// when tests run in parallel (both tests mutate XDG_CONFIG_HOME).
    #[test]
    fn default_config_path_respects_xdg_and_falls_back() {
        // Case 1: XDG_CONFIG_HOME is set
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/custom/config") };
        let path = crate::paths::default_config_path();
        assert_eq!(path, PathBuf::from("/custom/config/niri/niri-tools.kdl"));

        // Case 2: XDG_CONFIG_HOME is unset, falls back to $HOME/.config
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        unsafe { std::env::set_var("HOME", "/home/testuser") };
        let path = crate::paths::default_config_path();
        assert_eq!(
            path,
            PathBuf::from("/home/testuser/.config/niri/niri-tools.kdl")
        );
    }
}
