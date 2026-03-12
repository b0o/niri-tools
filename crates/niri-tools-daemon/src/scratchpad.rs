use niri_tools_common::config::ScratchpadConfig;
use niri_tools_common::traits::NiriClient;
use niri_tools_common::types::{OutputInfo, WindowInfo};

use crate::state::{DaemonState, SCRATCHPAD_WORKSPACE};

/// Convert position strings (percentage or pixel) to pixel coordinates.
///
/// - Percentage values (e.g. "50%"): position within valid range
///   where 0% = left/top edge, 100% = right/bottom edge, 50% = centered.
///   Formula: max_pos = max(0, output_dim - window_dim); pos = max_pos * pct / 100
/// - Pixel values (e.g. "200"): used directly as integers
pub fn convert_position_to_pixels(
    x_str: &str,
    y_str: &str,
    output_width: u32,
    output_height: u32,
    window_width: u32,
    window_height: u32,
) -> (i32, i32) {
    let x = parse_position_value(x_str, output_width, window_width);
    let y = parse_position_value(y_str, output_height, window_height);
    (x, y)
}

fn parse_position_value(value: &str, output_dim: u32, window_dim: u32) -> i32 {
    let trimmed = value.trim();
    if let Some(pct_str) = trimmed.strip_suffix('%') {
        let pct: f64 = pct_str.trim().parse().unwrap_or(0.0);
        let max_pos = (output_dim as i64 - window_dim as i64).max(0) as f64;
        (max_pos * pct / 100.0).round() as i32
    } else {
        trimmed.parse::<i32>().unwrap_or(0)
    }
}

/// Calculate the expected window size given a config and output, resolving
/// percentage sizes against output dimensions and considering per-output overrides.
pub fn calculate_expected_window_size(
    config: &ScratchpadConfig,
    output: &OutputInfo,
) -> (u32, u32) {
    // Check for output-specific overrides first
    let (size_config, _pos_config) = resolve_output_overrides(config, &output.name);

    let size = match size_config {
        Some(s) => s,
        None => return (output.width, output.height), // no size config = full output
    };

    let width = parse_size_value(&size.width, output.width);
    let height = parse_size_value(&size.height, output.height);
    (width, height)
}

fn resolve_output_overrides<'a>(
    config: &'a ScratchpadConfig,
    output_name: &str,
) -> (
    Option<&'a niri_tools_common::config::SizeConfig>,
    Option<&'a niri_tools_common::config::PositionConfig>,
) {
    let override_cfg = config.output_overrides.get(output_name);

    let size = override_cfg
        .and_then(|o| o.size.as_ref())
        .or(config.size.as_ref());

    let position = override_cfg
        .and_then(|o| o.position.as_ref())
        .or(config.position.as_ref());

    (size, position)
}

fn parse_size_value(value: &str, output_dim: u32) -> u32 {
    let trimmed = value.trim();
    if let Some(pct_str) = trimmed.strip_suffix('%') {
        let pct: f64 = pct_str.trim().parse().unwrap_or(100.0);
        (output_dim as f64 * pct / 100.0).round() as u32
    } else {
        trimmed.parse::<u32>().unwrap_or(output_dim)
    }
}

/// Check if a window matches a scratchpad config's app_id and/or title criteria.
///
/// For app_id and title fields:
/// - Values starting with `/` are regex patterns (strip the leading `/` and compile)
/// - Values starting with `^` are also treated as regex
/// - Other values are exact string matches
pub fn matches_config(window: &WindowInfo, config: &ScratchpadConfig) -> bool {
    let app_id_matches = match &config.app_id {
        Some(pattern) => matches_pattern(pattern, &window.app_id),
        None => true, // no constraint = matches
    };

    let title_matches = match &config.title {
        Some(pattern) => matches_pattern(pattern, &window.title),
        None => true,
    };

    // Both must match (if specified)
    app_id_matches && title_matches
}

fn matches_pattern(pattern: &str, value: &str) -> bool {
    if let Some(regex_str) = pattern.strip_prefix('/') {
        // Regex pattern: strip leading '/'
        match regex::Regex::new(regex_str) {
            Ok(re) => re.is_match(value),
            Err(_) => false,
        }
    } else if pattern.starts_with('^') {
        // Also regex
        match regex::Regex::new(pattern) {
            Ok(re) => re.is_match(value),
            Err(_) => false,
        }
    } else {
        // Exact match
        pattern == value
    }
}

pub struct ScratchpadManager<'a> {
    state: &'a mut DaemonState,
    niri: &'a dyn NiriClient,
}

impl<'a> ScratchpadManager<'a> {
    pub fn new(state: &'a mut DaemonState, niri: &'a dyn NiriClient) -> Self {
        Self { state, niri }
    }

    /// Toggle a named scratchpad.
    pub async fn toggle(&mut self, name: &str) -> niri_tools_common::Result<()> {
        let config = self
            .state
            .scratchpad_configs
            .get(name)
            .cloned()
            .ok_or_else(|| {
                niri_tools_common::NiriToolsError::Other(format!(
                    "No scratchpad config for '{name}'"
                ))
            })?;

        let sp = self.state.scratchpads.get(name);
        let window_id = sp.and_then(|s| s.window_id);

        match window_id {
            None => {
                // No window exists: spawn
                self.spawn_scratchpad(name, &config).await?;
            }
            Some(wid) => {
                let window = self.state.windows.get(&wid).cloned();
                match window {
                    None => {
                        // Window tracked but no longer exists, spawn fresh
                        self.spawn_scratchpad(name, &config).await?;
                    }
                    Some(win) => {
                        self.toggle_existing(name, &win, &config).await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn toggle_existing(
        &mut self,
        name: &str,
        window: &WindowInfo,
        config: &ScratchpadConfig,
    ) -> niri_tools_common::Result<()> {
        let wid = window.id;

        if window.is_focused {
            if window.is_floating {
                // Focused + floating: hide it
                self.hide_scratchpad(name, wid).await?;
            } else {
                // Focused + tiled: focus previous window
                self.focus_previous_window().await?;
            }
        } else {
            // Not focused - check workspace
            let window_ws_id = window.workspace_id;
            let focused_ws = self.state.get_focused_workspace().map(|ws| ws.id);

            let same_workspace =
                window_ws_id.is_some() && focused_ws.is_some() && window_ws_id == focused_ws;

            if same_workspace {
                // Same workspace, not focused: just focus it
                self.focus_window(wid).await?;
            } else if window.is_floating {
                // Different workspace + floating: show on current monitor
                self.show_scratchpad(name, wid, config).await?;
            } else {
                // Different workspace + tiled
                let on_scratchpad_ws = self.state.is_on_scratchpad_workspace(window);
                if on_scratchpad_ws {
                    // On scratchpad workspace: move to current workspace and focus
                    self.show_scratchpad(name, wid, config).await?;
                } else {
                    // Not on scratchpad workspace: focus where it is
                    self.focus_window(wid).await?;
                }
            }
        }

        Ok(())
    }

    /// Smart toggle: context-aware toggle without a name.
    pub async fn smart_toggle(&mut self) -> niri_tools_common::Result<()> {
        // Check if focused window is a scratchpad
        if let Some(focused_id) = self.state.focused_window_id {
            if let Some(name) = self.state.get_scratchpad_for_window(focused_id) {
                let name = name.to_string();
                let window = self.state.windows.get(&focused_id).cloned();
                if let Some(win) = window {
                    if win.is_floating {
                        // Focused floating scratchpad: hide it
                        self.hide_scratchpad(&name, focused_id).await?;
                        return Ok(());
                    } else {
                        // Focused tiled scratchpad: focus previous
                        self.focus_previous_window().await?;
                        return Ok(());
                    }
                }
            }
        }

        // Check if previous focused window was a visible scratchpad (brief focus loss)
        if let Some(prev_id) = self.state.previous_focused_window_id {
            if let Some(name) = self.state.get_scratchpad_for_window(prev_id) {
                let name = name.to_string();
                let sp = self.state.scratchpads.get(&name);
                if sp.is_some_and(|s| s.visible) {
                    let window = self.state.windows.get(&prev_id).cloned();
                    if let Some(win) = window {
                        if win.is_floating {
                            self.hide_scratchpad(&name, prev_id).await?;
                            return Ok(());
                        } else {
                            self.focus_previous_window().await?;
                            return Ok(());
                        }
                    }
                }
            }
        }

        // Otherwise: show the most recently hidden scratchpad
        let most_recent = self.state.get_most_recent_hidden_scratchpad().map(String::from);
        if let Some(name) = most_recent {
            let sp = self.state.scratchpads.get(&name);
            let wid = sp.and_then(|s| s.window_id);
            if let Some(wid) = wid {
                let config = self.state.scratchpad_configs.get(&name).cloned();
                if let Some(config) = config {
                    self.show_scratchpad(&name, wid, &config).await?;
                }
            }
        }

        Ok(())
    }

    /// Hide focused scratchpad (move floating window to scratchpad workspace).
    pub async fn hide(&mut self) -> niri_tools_common::Result<()> {
        let focused_id = match self.state.focused_window_id {
            Some(id) => id,
            None => return Ok(()),
        };

        let window = self.state.windows.get(&focused_id).cloned();
        let is_floating = window.as_ref().is_some_and(|w| w.is_floating);

        if !is_floating {
            return Ok(());
        }

        // Check if it's a tracked scratchpad
        if let Some(name) = self.state.get_scratchpad_for_window(focused_id) {
            let name = name.to_string();
            self.hide_scratchpad(&name, focused_id).await?;
        } else {
            // Untracked floating window: just move to scratchpad workspace
            self.move_to_scratchpad_workspace(focused_id).await?;
        }

        Ok(())
    }

    /// Toggle float/tile for a scratchpad.
    pub async fn toggle_float(
        &mut self,
        name: Option<&str>,
    ) -> niri_tools_common::Result<()> {
        let (resolved_name, wid, config) = self.resolve_scratchpad(name)?;

        let window = self.state.windows.get(&wid).cloned();
        let is_floating = window.as_ref().is_some_and(|w| w.is_floating);
        let is_focused = window.as_ref().is_some_and(|w| w.is_focused);

        if !is_focused {
            // Bring to current workspace first
            self.show_scratchpad(&resolved_name, wid, &config).await?;
        }

        if is_floating {
            // Float -> tile
            self.tile_window(wid).await?;
        } else {
            // Tile -> float and configure
            self.configure_window(wid, &config).await?;
        }

        Ok(())
    }

    /// Make a scratchpad floating (no-op if already floating).
    pub async fn float_scratchpad(
        &mut self,
        name: Option<&str>,
    ) -> niri_tools_common::Result<()> {
        let (resolved_name, wid, config) = self.resolve_scratchpad(name)?;

        let window = self.state.windows.get(&wid).cloned();
        let is_floating = window.as_ref().is_some_and(|w| w.is_floating);

        if is_floating {
            return Ok(()); // Already floating
        }

        let is_focused = window.as_ref().is_some_and(|w| w.is_focused);
        if !is_focused {
            self.show_scratchpad(&resolved_name, wid, &config).await?;
        }

        self.configure_window(wid, &config).await?;

        Ok(())
    }

    /// Make a scratchpad tiled (no-op if already tiled).
    pub async fn tile_scratchpad(
        &mut self,
        name: Option<&str>,
    ) -> niri_tools_common::Result<()> {
        let (_resolved_name, wid, _config) = self.resolve_scratchpad(name)?;

        let window = self.state.windows.get(&wid).cloned();
        let is_floating = window.as_ref().is_some_and(|w| w.is_floating);

        if !is_floating {
            return Ok(()); // Already tiled
        }

        self.tile_window(wid).await?;

        Ok(())
    }

    /// Called when a new window opens. Checks pending_spawns for a config match.
    pub async fn handle_window_opened(
        &mut self,
        window: &WindowInfo,
    ) -> niri_tools_common::Result<()> {
        // Check pending spawns
        let matching_name = self
            .state
            .pending_spawns
            .iter()
            .find(|name| {
                self.state
                    .scratchpad_configs
                    .get(*name)
                    .is_some_and(|cfg| matches_config(window, cfg))
            })
            .cloned();

        let name = match matching_name {
            Some(n) => n,
            None => return Ok(()),
        };

        let config = match self.state.scratchpad_configs.get(&name).cloned() {
            Some(c) => c,
            None => return Ok(()),
        };

        // Remove from pending
        self.state.pending_spawns.remove(&name);

        // Register window
        self.state.register_scratchpad_window(&name, window.id);
        self.state.mark_scratchpad_visible(&name);

        // Configure (float + size + position)
        self.configure_window(window.id, &config).await?;

        // Focus it
        self.focus_window(window.id).await?;

        // Save state
        let _ = self.state.save_scratchpad_state();

        Ok(())
    }

    // -- Helper methods --

    async fn spawn_scratchpad(
        &mut self,
        name: &str,
        config: &ScratchpadConfig,
    ) -> niri_tools_common::Result<()> {
        let command = config.command.as_ref().ok_or_else(|| {
            niri_tools_common::NiriToolsError::Other(format!(
                "Scratchpad '{name}' has no command to spawn"
            ))
        })?;

        let mut args: Vec<&str> = vec!["--"];
        for part in command {
            args.push(part.as_str());
        }

        self.niri.run_action("spawn", &args).await?;
        self.state.pending_spawns.insert(name.to_string());

        Ok(())
    }

    async fn show_scratchpad(
        &mut self,
        name: &str,
        window_id: u64,
        config: &ScratchpadConfig,
    ) -> niri_tools_common::Result<()> {
        // Configure window (float + size + position)
        self.configure_window(window_id, config).await?;

        // Move to current monitor
        if let Some(output) = &self.state.focused_output.clone() {
            let id_str = window_id.to_string();
            self.niri
                .run_action("move-window-to-monitor", &["--id", &id_str, output])
                .await?;
        }

        // Focus
        self.focus_window(window_id).await?;

        // Mark visible + save state
        self.state.mark_scratchpad_visible(name);
        let _ = self.state.save_scratchpad_state();

        Ok(())
    }

    async fn hide_scratchpad(
        &mut self,
        name: &str,
        window_id: u64,
    ) -> niri_tools_common::Result<()> {
        self.move_to_scratchpad_workspace(window_id).await?;

        // Mark hidden + save state
        self.state.mark_scratchpad_hidden(name);
        let _ = self.state.save_scratchpad_state();

        Ok(())
    }

    async fn configure_window(
        &mut self,
        window_id: u64,
        config: &ScratchpadConfig,
    ) -> niri_tools_common::Result<()> {
        let id_str = window_id.to_string();

        // Float the window
        self.niri
            .run_action("move-window-to-floating", &["--id", &id_str])
            .await?;

        // Resolve output-specific overrides
        let output_name = self.state.focused_output.clone().unwrap_or_default();
        let output = self.state.outputs.get(&output_name).cloned();
        let (size_cfg, pos_cfg) = resolve_output_overrides(config, &output_name);

        // Set size if configured
        if let Some(size) = size_cfg {
            self.niri
                .run_action("set-window-width", &["--id", &id_str, &size.width])
                .await?;
            self.niri
                .run_action("set-window-height", &["--id", &id_str, &size.height])
                .await?;
        }

        // Position window
        if let Some(pos) = pos_cfg {
            if let Some(out) = &output {
                // Calculate expected window size for positioning
                let (win_w, win_h) = calculate_expected_window_size(config, out);
                let (x, y) = convert_position_to_pixels(
                    &pos.x, &pos.y, out.width, out.height, win_w, win_h,
                );
                let x_str = x.to_string();
                let y_str = y.to_string();
                self.niri
                    .run_action(
                        "move-floating-window",
                        &["--id", &id_str, "--x", &x_str, "--y", &y_str],
                    )
                    .await?;
            }
        }

        Ok(())
    }

    async fn focus_window(
        &mut self,
        window_id: u64,
    ) -> niri_tools_common::Result<()> {
        let id_str = window_id.to_string();
        self.niri
            .run_action("focus-window", &["--id", &id_str])
            .await?;

        // Update focus tracking
        self.state.previous_focused_window_id = self.state.focused_window_id;
        self.state.focused_window_id = Some(window_id);

        Ok(())
    }

    async fn focus_previous_window(&mut self) -> niri_tools_common::Result<()> {
        if let Some(prev_id) = self.state.previous_focused_window_id {
            self.focus_window(prev_id).await?;
        }
        Ok(())
    }

    async fn move_to_scratchpad_workspace(
        &mut self,
        window_id: u64,
    ) -> niri_tools_common::Result<()> {
        let id_str = window_id.to_string();
        self.niri
            .run_action(
                "move-window-to-workspace",
                &["--window-id", &id_str, "--focus", "false", SCRATCHPAD_WORKSPACE],
            )
            .await?;
        Ok(())
    }

    async fn tile_window(&mut self, window_id: u64) -> niri_tools_common::Result<()> {
        let id_str = window_id.to_string();
        self.niri
            .run_action("move-window-to-tiling", &["--id", &id_str])
            .await?;
        Ok(())
    }

    fn resolve_scratchpad(
        &self,
        name: Option<&str>,
    ) -> niri_tools_common::Result<(String, u64, ScratchpadConfig)> {
        if let Some(name) = name {
            // By name
            let sp = self.state.scratchpads.get(name);
            let wid = sp
                .and_then(|s| s.window_id)
                .ok_or_else(|| {
                    niri_tools_common::NiriToolsError::Other(format!(
                        "Scratchpad '{name}' has no active window. Use 'toggle {name}' to spawn it."
                    ))
                })?;
            let config = self
                .state
                .scratchpad_configs
                .get(name)
                .cloned()
                .ok_or_else(|| {
                    niri_tools_common::NiriToolsError::Other(format!(
                        "No config for scratchpad '{name}'"
                    ))
                })?;
            Ok((name.to_string(), wid, config))
        } else {
            // Try focused window
            if let Some(focused_id) = self.state.focused_window_id {
                if let Some(name) = self.state.get_scratchpad_for_window(focused_id) {
                    let name = name.to_string();
                    let config = self
                        .state
                        .scratchpad_configs
                        .get(&name)
                        .cloned()
                        .ok_or_else(|| {
                            niri_tools_common::NiriToolsError::Other(format!(
                                "No config for scratchpad '{name}'"
                            ))
                        })?;
                    return Ok((name, focused_id, config));
                }
            }

            // Try most recent with window
            let most_recent = self
                .state
                .scratchpads
                .iter()
                .filter(|(_, sp)| sp.window_id.is_some())
                .max_by(|(_, a), (_, b)| a.last_used.partial_cmp(&b.last_used).unwrap())
                .map(|(name, sp)| (name.clone(), sp.window_id.unwrap()));

            if let Some((name, wid)) = most_recent {
                let config = self
                    .state
                    .scratchpad_configs
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| {
                        niri_tools_common::NiriToolsError::Other(format!(
                            "No config for scratchpad '{name}'"
                        ))
                    })?;
                return Ok((name, wid, config));
            }

            Err(niri_tools_common::NiriToolsError::Other(
                "No scratchpad to act on. Specify a scratchpad name or focus a scratchpad window first.".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use futures_core::Stream;
    use niri_tools_common::config::{OutputOverride, PositionConfig, SizeConfig};
    use niri_tools_common::types::{NiriEvent, WorkspaceInfo};

    // -- MockNiriClient --

    #[derive(Debug, Default, Clone)]
    struct MockActions {
        calls: Vec<(String, Vec<String>)>,
    }

    struct MockNiriClient {
        actions: Arc<Mutex<MockActions>>,
        windows: Vec<WindowInfo>,
        workspaces: Vec<WorkspaceInfo>,
        outputs: HashMap<String, OutputInfo>,
        focused_output: String,
    }

    impl MockNiriClient {
        fn new() -> Self {
            Self {
                actions: Arc::new(Mutex::new(MockActions::default())),
                windows: vec![],
                workspaces: vec![],
                outputs: HashMap::new(),
                focused_output: "eDP-1".to_string(),
            }
        }

        fn get_actions(&self) -> Vec<(String, Vec<String>)> {
            self.actions.lock().unwrap().calls.clone()
        }
    }

    #[async_trait::async_trait]
    impl NiriClient for MockNiriClient {
        async fn run_action(&self, action: &str, args: &[&str]) -> niri_tools_common::Result<()> {
            self.actions.lock().unwrap().calls.push((
                action.to_string(),
                args.iter().map(|s| s.to_string()).collect(),
            ));
            Ok(())
        }

        async fn get_windows(&self) -> niri_tools_common::Result<Vec<WindowInfo>> {
            Ok(self.windows.clone())
        }

        async fn get_workspaces(&self) -> niri_tools_common::Result<Vec<WorkspaceInfo>> {
            Ok(self.workspaces.clone())
        }

        async fn get_outputs(&self) -> niri_tools_common::Result<HashMap<String, OutputInfo>> {
            Ok(self.outputs.clone())
        }

        async fn get_focused_output(&self) -> niri_tools_common::Result<String> {
            Ok(self.focused_output.clone())
        }

        async fn subscribe_events(
            &self,
        ) -> niri_tools_common::Result<Pin<Box<dyn Stream<Item = niri_tools_common::Result<NiriEvent>> + Send>>>
        {
            unimplemented!("not needed for unit tests")
        }
    }

    // -- Test helpers --

    fn make_config(name: &str) -> ScratchpadConfig {
        ScratchpadConfig {
            name: name.to_string(),
            command: Some(vec!["ghostty".to_string()]),
            app_id: Some("ghostty".to_string()),
            title: None,
            size: Some(SizeConfig {
                width: "60%".to_string(),
                height: "60%".to_string(),
            }),
            position: Some(PositionConfig {
                x: "50%".to_string(),
                y: "50%".to_string(),
            }),
            output_overrides: HashMap::new(),
        }
    }

    fn make_window(id: u64, app_id: &str, is_focused: bool, is_floating: bool, workspace_id: Option<u64>) -> WindowInfo {
        WindowInfo {
            id,
            app_id: app_id.to_string(),
            title: "test".to_string(),
            workspace_id,
            is_focused,
            is_floating,
            width: 800,
            height: 600,
        }
    }

    fn make_workspace(id: u64, output: &str, is_active: bool, name: Option<&str>) -> WorkspaceInfo {
        WorkspaceInfo {
            id,
            idx: 1,
            output: output.to_string(),
            is_active,
            name: name.map(|s| s.to_string()),
        }
    }

    fn setup_state() -> DaemonState {
        let mut state = DaemonState::default();

        // Active workspace on eDP-1
        state.workspaces.insert(1, make_workspace(1, "eDP-1", true, Some("main")));
        // Scratchpad workspace
        state.workspaces.insert(2, make_workspace(2, "eDP-1", false, Some(SCRATCHPAD_WORKSPACE)));

        state.outputs.insert("eDP-1".to_string(), OutputInfo {
            name: "eDP-1".to_string(),
            width: 1920,
            height: 1080,
        });

        state.focused_output = Some("eDP-1".to_string());

        state
    }

    fn setup_state_with_config(name: &str) -> DaemonState {
        let mut state = setup_state();
        state.scratchpad_configs.insert(name.to_string(), make_config(name));
        state
    }

    // ============================================================
    // Pure function tests
    // ============================================================

    // -- convert_position_to_pixels --

    #[test]
    fn convert_position_to_pixels_percentage_values() {
        // 20% of (1920 - 800) = 20% of 1120 = 224
        // 30% of (1080 - 600) = 30% of 480 = 144
        let (x, y) = convert_position_to_pixels("20%", "30%", 1920, 1080, 800, 600);
        assert_eq!(x, 224);
        assert_eq!(y, 144);
    }

    #[test]
    fn convert_position_to_pixels_pixel_values() {
        let (x, y) = convert_position_to_pixels("100", "200", 1920, 1080, 800, 600);
        assert_eq!(x, 100);
        assert_eq!(y, 200);
    }

    #[test]
    fn convert_position_to_pixels_50_percent_centers_window() {
        // 50% of (1920 - 800) = 50% of 1120 = 560
        // 50% of (1080 - 600) = 50% of 480 = 240
        let (x, y) = convert_position_to_pixels("50%", "50%", 1920, 1080, 800, 600);
        assert_eq!(x, 560);
        assert_eq!(y, 240);
    }

    #[test]
    fn convert_position_to_pixels_0_percent_is_top_left() {
        let (x, y) = convert_position_to_pixels("0%", "0%", 1920, 1080, 800, 600);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }

    #[test]
    fn convert_position_to_pixels_100_percent_is_bottom_right() {
        // 100% of (1920 - 800) = 1120
        // 100% of (1080 - 600) = 480
        let (x, y) = convert_position_to_pixels("100%", "100%", 1920, 1080, 800, 600);
        assert_eq!(x, 1120);
        assert_eq!(y, 480);
    }

    #[test]
    fn convert_position_to_pixels_window_larger_than_output() {
        // max_pos = max(0, 800 - 1920) = 0
        let (x, y) = convert_position_to_pixels("50%", "50%", 800, 600, 1920, 1080);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }

    // -- calculate_expected_window_size --

    #[test]
    fn calculate_expected_window_size_percentage() {
        let config = make_config("term");
        let output = OutputInfo {
            name: "eDP-1".to_string(),
            width: 1920,
            height: 1080,
        };
        // 60% of 1920 = 1152, 60% of 1080 = 648
        let (w, h) = calculate_expected_window_size(&config, &output);
        assert_eq!(w, 1152);
        assert_eq!(h, 648);
    }

    #[test]
    fn calculate_expected_window_size_pixel_values() {
        let config = ScratchpadConfig {
            name: "term".to_string(),
            command: Some(vec!["foot".to_string()]),
            app_id: Some("foot".to_string()),
            title: None,
            size: Some(SizeConfig {
                width: "800".to_string(),
                height: "600".to_string(),
            }),
            position: None,
            output_overrides: HashMap::new(),
        };
        let output = OutputInfo {
            name: "eDP-1".to_string(),
            width: 1920,
            height: 1080,
        };
        let (w, h) = calculate_expected_window_size(&config, &output);
        assert_eq!(w, 800);
        assert_eq!(h, 600);
    }

    #[test]
    fn calculate_expected_window_size_with_output_override() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "eDP-1".to_string(),
            OutputOverride {
                size: Some(SizeConfig {
                    width: "80%".to_string(),
                    height: "80%".to_string(),
                }),
                position: None,
            },
        );

        let config = ScratchpadConfig {
            name: "term".to_string(),
            command: Some(vec!["foot".to_string()]),
            app_id: Some("foot".to_string()),
            title: None,
            size: Some(SizeConfig {
                width: "60%".to_string(),
                height: "60%".to_string(),
            }),
            position: None,
            output_overrides: overrides,
        };
        let output = OutputInfo {
            name: "eDP-1".to_string(),
            width: 1920,
            height: 1080,
        };
        // Override: 80% of 1920 = 1536, 80% of 1080 = 864
        let (w, h) = calculate_expected_window_size(&config, &output);
        assert_eq!(w, 1536);
        assert_eq!(h, 864);
    }

    #[test]
    fn calculate_expected_window_size_no_size_config() {
        let config = ScratchpadConfig {
            name: "term".to_string(),
            command: None,
            app_id: None,
            title: None,
            size: None,
            position: None,
            output_overrides: HashMap::new(),
        };
        let output = OutputInfo {
            name: "eDP-1".to_string(),
            width: 1920,
            height: 1080,
        };
        // No size config = full output
        let (w, h) = calculate_expected_window_size(&config, &output);
        assert_eq!(w, 1920);
        assert_eq!(h, 1080);
    }

    // -- matches_config --

    #[test]
    fn matches_config_exact_app_id_match() {
        let config = make_config("term");
        let window = make_window(1, "ghostty", false, false, Some(1));
        assert!(matches_config(&window, &config));
    }

    #[test]
    fn matches_config_exact_app_id_no_match() {
        let config = make_config("term");
        let window = make_window(1, "foot", false, false, Some(1));
        assert!(!matches_config(&window, &config));
    }

    #[test]
    fn matches_config_regex_app_id_with_slash() {
        let config = ScratchpadConfig {
            name: "term".to_string(),
            command: None,
            app_id: Some("/ghost.*".to_string()),
            title: None,
            size: None,
            position: None,
            output_overrides: HashMap::new(),
        };
        let window = make_window(1, "ghostty", false, false, Some(1));
        assert!(matches_config(&window, &config));

        let window2 = make_window(2, "foot", false, false, Some(1));
        assert!(!matches_config(&window2, &config));
    }

    #[test]
    fn matches_config_regex_title_with_caret() {
        let config = ScratchpadConfig {
            name: "term".to_string(),
            command: None,
            app_id: None,
            title: Some("^Terminal.*".to_string()),
            size: None,
            position: None,
            output_overrides: HashMap::new(),
        };
        let mut window = make_window(1, "foot", false, false, Some(1));
        window.title = "Terminal - bash".to_string();
        assert!(matches_config(&window, &config));

        let mut window2 = make_window(2, "foot", false, false, Some(1));
        window2.title = "Not a terminal".to_string();
        assert!(!matches_config(&window2, &config));
    }

    #[test]
    fn matches_config_no_criteria_matches_everything() {
        let config = ScratchpadConfig {
            name: "term".to_string(),
            command: None,
            app_id: None,
            title: None,
            size: None,
            position: None,
            output_overrides: HashMap::new(),
        };
        let window = make_window(1, "anything", false, false, Some(1));
        assert!(matches_config(&window, &config));
    }

    #[test]
    fn matches_config_both_app_id_and_title_must_match() {
        let config = ScratchpadConfig {
            name: "term".to_string(),
            command: None,
            app_id: Some("ghostty".to_string()),
            title: Some("Terminal".to_string()),
            size: None,
            position: None,
            output_overrides: HashMap::new(),
        };

        let mut window = make_window(1, "ghostty", false, false, Some(1));
        window.title = "Terminal".to_string();
        assert!(matches_config(&window, &config));

        // app_id matches but title doesn't
        let mut window2 = make_window(2, "ghostty", false, false, Some(1));
        window2.title = "Other".to_string();
        assert!(!matches_config(&window2, &config));

        // title matches but app_id doesn't
        let mut window3 = make_window(3, "foot", false, false, Some(1));
        window3.title = "Terminal".to_string();
        assert!(!matches_config(&window3, &config));
    }

    // ============================================================
    // ScratchpadManager async tests
    // ============================================================

    // -- toggle tests --

    #[tokio::test]
    async fn toggle_no_window_spawns() {
        let mut state = setup_state_with_config("term");
        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.toggle("term").await.unwrap();
        }

        let actions = niri.get_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].0, "spawn");
        assert_eq!(actions[0].1, vec!["--", "ghostty"]);
        assert!(state.pending_spawns.contains("term"));
    }

    #[tokio::test]
    async fn toggle_focused_floating_hides() {
        let mut state = setup_state_with_config("term");

        // Register a floating focused window
        let window = make_window(42, "ghostty", true, true, Some(1));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);
        state.mark_scratchpad_visible("term");
        state.focused_window_id = Some(42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.toggle("term").await.unwrap();
        }

        let actions = niri.get_actions();
        // Should have move-window-to-workspace action
        assert!(actions.iter().any(|(action, args)| {
            action == "move-window-to-workspace"
                && args.contains(&SCRATCHPAD_WORKSPACE.to_string())
        }));
        assert!(!state.scratchpads.get("term").unwrap().visible);
    }

    #[tokio::test]
    async fn toggle_focused_tiled_focuses_previous() {
        let mut state = setup_state_with_config("term");

        // Register a tiled focused window
        let window = make_window(42, "ghostty", true, false, Some(1));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);
        state.focused_window_id = Some(42);
        state.previous_focused_window_id = Some(99);

        // Add the previous window too
        let prev_window = make_window(99, "firefox", false, false, Some(1));
        state.windows.insert(99, prev_window);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.toggle("term").await.unwrap();
        }

        let actions = niri.get_actions();
        assert!(actions.iter().any(|(action, args)| {
            action == "focus-window" && args.contains(&"99".to_string())
        }));
    }

    #[tokio::test]
    async fn toggle_different_workspace_floating_shows() {
        let mut state = setup_state_with_config("term");

        // Add a second workspace on another output
        state.workspaces.insert(3, make_workspace(3, "HDMI-A-1", true, Some("other")));

        // Window on workspace 3 (different from focused workspace 1), floating
        let window = make_window(42, "ghostty", false, true, Some(3));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.toggle("term").await.unwrap();
        }

        let actions = niri.get_actions();
        // Should configure (float, set size, set height) + move to monitor + focus
        assert!(actions.iter().any(|(action, _)| action == "move-window-to-floating"));
        assert!(actions.iter().any(|(action, _)| action == "move-window-to-monitor"));
        assert!(actions.iter().any(|(action, args)| {
            action == "focus-window" && args.contains(&"42".to_string())
        }));
        assert!(state.scratchpads.get("term").unwrap().visible);
    }

    #[tokio::test]
    async fn toggle_same_workspace_not_focused_focuses() {
        let mut state = setup_state_with_config("term");

        // Window on same workspace (1), not focused
        let window = make_window(42, "ghostty", false, true, Some(1));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);
        state.focused_window_id = Some(99); // something else is focused

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.toggle("term").await.unwrap();
        }

        let actions = niri.get_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].0, "focus-window");
        assert!(actions[0].1.contains(&"42".to_string()));
    }

    #[tokio::test]
    async fn toggle_different_workspace_tiled_on_scratchpad_ws_shows() {
        let mut state = setup_state_with_config("term");

        // Window on scratchpad workspace, tiled
        let window = make_window(42, "ghostty", false, false, Some(2));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.toggle("term").await.unwrap();
        }

        let actions = niri.get_actions();
        // Should show: configure + move to monitor + focus
        assert!(actions.iter().any(|(action, _)| action == "move-window-to-floating"));
        assert!(actions.iter().any(|(action, _)| action == "move-window-to-monitor"));
        assert!(actions.iter().any(|(action, args)| {
            action == "focus-window" && args.contains(&"42".to_string())
        }));
    }

    #[tokio::test]
    async fn toggle_different_workspace_tiled_not_scratchpad_ws_focuses() {
        let mut state = setup_state_with_config("term");

        // Add a third workspace (not scratchpad)
        state.workspaces.insert(3, make_workspace(3, "eDP-1", false, Some("code")));

        // Window on workspace 3, tiled, not scratchpad workspace
        let window = make_window(42, "ghostty", false, false, Some(3));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.toggle("term").await.unwrap();
        }

        let actions = niri.get_actions();
        // Should just focus where it is
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].0, "focus-window");
        assert!(actions[0].1.contains(&"42".to_string()));
    }

    // -- smart_toggle tests --

    #[tokio::test]
    async fn smart_toggle_focused_floating_scratchpad_hides() {
        let mut state = setup_state_with_config("term");

        let window = make_window(42, "ghostty", true, true, Some(1));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);
        state.mark_scratchpad_visible("term");
        state.focused_window_id = Some(42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.smart_toggle().await.unwrap();
        }

        let actions = niri.get_actions();
        assert!(actions.iter().any(|(action, args)| {
            action == "move-window-to-workspace"
                && args.contains(&SCRATCHPAD_WORKSPACE.to_string())
        }));
        assert!(!state.scratchpads.get("term").unwrap().visible);
    }

    #[tokio::test]
    async fn smart_toggle_no_scratchpad_focused_shows_most_recent_hidden() {
        let mut state = setup_state_with_config("term");

        // Register window but mark hidden
        let window = make_window(42, "ghostty", false, true, Some(2));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);
        state.mark_scratchpad_hidden("term");

        // Focus is on a non-scratchpad window
        state.focused_window_id = Some(99);
        let other = make_window(99, "firefox", true, false, Some(1));
        state.windows.insert(99, other);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.smart_toggle().await.unwrap();
        }

        let actions = niri.get_actions();
        // Should show the hidden scratchpad
        assert!(actions.iter().any(|(action, _)| action == "move-window-to-floating"));
        assert!(actions.iter().any(|(action, _)| action == "move-window-to-monitor"));
        assert!(actions.iter().any(|(action, args)| {
            action == "focus-window" && args.contains(&"42".to_string())
        }));
        assert!(state.scratchpads.get("term").unwrap().visible);
    }

    // -- hide tests --

    #[tokio::test]
    async fn hide_focused_floating_window_moves_to_scratchpad_workspace() {
        let mut state = setup_state_with_config("term");

        let window = make_window(42, "ghostty", true, true, Some(1));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);
        state.mark_scratchpad_visible("term");
        state.focused_window_id = Some(42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.hide().await.unwrap();
        }

        let actions = niri.get_actions();
        assert!(actions.iter().any(|(action, args)| {
            action == "move-window-to-workspace"
                && args.contains(&SCRATCHPAD_WORKSPACE.to_string())
        }));
        assert!(!state.scratchpads.get("term").unwrap().visible);
    }

    #[tokio::test]
    async fn hide_untracked_floating_window_moves_to_scratchpad_workspace() {
        let mut state = setup_state();

        // Floating window that's not a tracked scratchpad
        let window = make_window(42, "random-app", true, true, Some(1));
        state.windows.insert(42, window);
        state.focused_window_id = Some(42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.hide().await.unwrap();
        }

        let actions = niri.get_actions();
        assert!(actions.iter().any(|(action, args)| {
            action == "move-window-to-workspace"
                && args.contains(&SCRATCHPAD_WORKSPACE.to_string())
        }));
    }

    #[tokio::test]
    async fn hide_tiled_window_does_nothing() {
        let mut state = setup_state();

        let window = make_window(42, "ghostty", true, false, Some(1));
        state.windows.insert(42, window);
        state.focused_window_id = Some(42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.hide().await.unwrap();
        }

        let actions = niri.get_actions();
        assert!(actions.is_empty());
    }

    // -- toggle_float tests --

    #[tokio::test]
    async fn toggle_float_floating_to_tiled() {
        let mut state = setup_state_with_config("term");

        let window = make_window(42, "ghostty", true, true, Some(1));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);
        state.mark_scratchpad_visible("term");
        state.focused_window_id = Some(42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.toggle_float(Some("term")).await.unwrap();
        }

        let actions = niri.get_actions();
        assert!(actions.iter().any(|(action, args)| {
            action == "move-window-to-tiling" && args.contains(&"42".to_string())
        }));
    }

    #[tokio::test]
    async fn toggle_float_tiled_to_floating() {
        let mut state = setup_state_with_config("term");

        let window = make_window(42, "ghostty", true, false, Some(1));
        state.windows.insert(42, window);
        state.register_scratchpad_window("term", 42);
        state.focused_window_id = Some(42);

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.toggle_float(Some("term")).await.unwrap();
        }

        let actions = niri.get_actions();
        // Should configure: float + set size + set height + position
        assert!(actions.iter().any(|(action, _)| action == "move-window-to-floating"));
        assert!(actions.iter().any(|(action, _)| action == "set-window-width"));
        assert!(actions.iter().any(|(action, _)| action == "set-window-height"));
    }

    // -- handle_window_opened tests --

    #[tokio::test]
    async fn handle_window_opened_matches_pending_spawn() {
        let mut state = setup_state_with_config("term");
        state.pending_spawns.insert("term".to_string());

        let window = make_window(42, "ghostty", false, false, Some(1));

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.handle_window_opened(&window).await.unwrap();
        }

        // Should be registered
        assert_eq!(state.get_scratchpad_for_window(42), Some("term"));
        assert!(state.scratchpads.get("term").unwrap().visible);
        assert!(!state.pending_spawns.contains("term"));

        let actions = niri.get_actions();
        // Should configure: float + set size + set height + position + focus
        assert!(actions.iter().any(|(action, _)| action == "move-window-to-floating"));
        assert!(actions.iter().any(|(action, args)| {
            action == "focus-window" && args.contains(&"42".to_string())
        }));
    }

    #[tokio::test]
    async fn handle_window_opened_no_match_does_nothing() {
        let mut state = setup_state_with_config("term");
        state.pending_spawns.insert("term".to_string());

        // Window with non-matching app_id
        let window = make_window(42, "firefox", false, false, Some(1));

        let niri = MockNiriClient::new();

        {
            let mut mgr = ScratchpadManager::new(&mut state, &niri);
            mgr.handle_window_opened(&window).await.unwrap();
        }

        // Should NOT be registered
        assert!(state.get_scratchpad_for_window(42).is_none());
        assert!(state.pending_spawns.contains("term"));

        let actions = niri.get_actions();
        assert!(actions.is_empty());
    }
}
