mod css;
mod mode_overlay;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use niri_tools_common::config::{BindAction, BindOption, ModeConfig, UiConfig};
use niri_tools_common::protocol::Command;

use crate::mode::ModeState;

/// Commands sent from the tokio thread to the GTK main thread.
#[derive(Debug)]
pub enum UiCommand {
    ModeShow {
        mode: Option<String>,
        mode_config: Option<ModeConfig>,
        mode_configs: HashMap<String, ModeConfig>,
        ui_config: UiConfig,
    },
    ModeHide,
    ModeToggle {
        mode: Option<String>,
        mode_config: Option<ModeConfig>,
        mode_configs: HashMap<String, ModeConfig>,
        ui_config: UiConfig,
    },
    ScratchpadPick,
}

/// Shared state for the mode overlay, accessible from GTK event handlers.
struct OverlayState {
    mode_state: ModeState,
    ui_config: UiConfig,
    /// Keycode of the key that triggered a close action.
    /// The overlay hides on release of this key.
    exit_on_key_release: Option<u32>,
    /// Channel to send daemon commands back to the tokio thread.
    daemon_tx: tokio::sync::mpsc::Sender<Command>,
}

/// Manages both GTK4 layer-shell windows (mode overlay and scratchpad picker).
///
/// Created on the GTK main thread during `connect_activate`. Both windows
/// start hidden and are shown/hidden via IPC commands forwarded from the
/// tokio background thread.
pub struct UiManager {
    mode_window: gtk4::ApplicationWindow,
    overlay_state: Rc<RefCell<OverlayState>>,
}

impl UiManager {
    pub fn new(
        app: &gtk4::Application,
        ui_config: &UiConfig,
        daemon_tx: tokio::sync::mpsc::Sender<Command>,
    ) -> Self {
        let mode_window = mode_overlay::create_mode_overlay(app, ui_config);

        let overlay_state = Rc::new(RefCell::new(OverlayState {
            mode_state: ModeState::new(HashMap::new()),
            ui_config: ui_config.clone(),
            exit_on_key_release: None,
            daemon_tx,
        }));

        // Attach keyboard handler
        Self::attach_keyboard_handler(&mode_window, &overlay_state);

        tracing::info!("UI manager initialized");
        Self {
            mode_window,
            overlay_state,
        }
    }

    /// Attach key press/release handlers to the mode overlay window.
    fn attach_keyboard_handler(
        window: &gtk4::ApplicationWindow,
        state: &Rc<RefCell<OverlayState>>,
    ) {
        let key_controller = gtk4::EventControllerKey::new();

        // Key pressed handler
        {
            let state = state.clone();
            let window = window.clone();
            key_controller.connect_key_pressed(move |_, keyval, keycode, modifiers| {
                Self::handle_key_pressed(&window, &state, keyval, keycode, modifiers)
            });
        }

        // Key released handler
        {
            let state = state.clone();
            let window = window.clone();
            key_controller.connect_key_released(move |_, _keyval, keycode, _modifiers| {
                Self::handle_key_released(&window, &state, keycode);
            });
        }

        window.add_controller(key_controller);
    }

    fn handle_key_pressed(
        window: &gtk4::ApplicationWindow,
        state: &Rc<RefCell<OverlayState>>,
        keyval: gtk4::gdk::Key,
        keycode: u32,
        modifiers: gtk4::gdk::ModifierType,
    ) -> gtk4::glib::Propagation {
        let mut s = state.borrow_mut();

        // If we're waiting for a key release to exit, ignore new presses
        if s.exit_on_key_release.is_some() {
            return gtk4::glib::Propagation::Stop;
        }

        // Convert keyval to key name
        let key_name = keyval_to_key_name(keyval);

        // Check for close keys: Escape, Ctrl+[, Ctrl+g
        let has_ctrl = modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
        if key_name == "Escape"
            || (has_ctrl && key_name == "bracketleft")
            || (has_ctrl && key_name == "g")
        {
            s.mode_state.clear();
            s.exit_on_key_release = Some(keycode);
            window.set_child(None::<&gtk4::Widget>);
            return gtk4::glib::Propagation::Stop;
        }

        // Backspace: pop mode stack
        if key_name == "BackSpace" {
            if s.mode_state.depth() > 1 {
                s.mode_state.pop_mode();
                if let Some(mode) = s.mode_state.current_mode() {
                    let mode = mode.clone();
                    let ui_config = s.ui_config.clone();
                    drop(s);
                    mode_overlay::rebuild_mode(window, &mode, &ui_config);
                }
            } else {
                // At root mode, close
                s.mode_state.clear();
                s.exit_on_key_release = Some(keycode);
                window.set_child(None::<&gtk4::Widget>);
            }
            return gtk4::glib::Propagation::Stop;
        }

        // Look up bind in current mode (ignore Super/Mod in the modifier check)
        let bind = s.mode_state.lookup_bind(&key_name).cloned();

        if let Some(bind) = bind {
            let action = bind.action.clone();
            let keep_open = s.mode_state.current_keep_open();
            let has_keep_open_option = bind.options.contains(&BindOption::KeepOpen);
            let has_close_option = bind.options.contains(&BindOption::Close);

            // Determine if we should close after this action
            let should_close = has_close_option || (!keep_open && !has_keep_open_option);

            // Execute the action
            match &action {
                BindAction::SwitchMode(name) => {
                    let name = name.clone();
                    s.mode_state.push_mode(&name);
                    if let Some(mode) = s.mode_state.current_mode() {
                        let mode = mode.clone();
                        let ui_config = s.ui_config.clone();
                        drop(s);
                        mode_overlay::rebuild_mode(window, &mode, &ui_config);
                    }
                    return gtk4::glib::Propagation::Stop;
                }
                BindAction::SpawnSh(cmd) => {
                    spawn_sh(cmd);
                }
                BindAction::Spawn(args) => {
                    spawn_process(args);
                }
                _ => {
                    // Forward to daemon via channel
                    if let Some(cmd) = bind_action_to_command(&action) {
                        let _ = s.daemon_tx.try_send(cmd);
                    }
                }
            }

            if should_close {
                s.mode_state.clear();
                s.exit_on_key_release = Some(keycode);
                window.set_child(None::<&gtk4::Widget>);
            }
        }

        gtk4::glib::Propagation::Stop
    }

    fn handle_key_released(
        window: &gtk4::ApplicationWindow,
        state: &Rc<RefCell<OverlayState>>,
        keycode: u32,
    ) {
        let mut s = state.borrow_mut();
        if s.exit_on_key_release == Some(keycode) {
            s.exit_on_key_release = None;
            drop(s);
            window.set_visible(false);
        }
    }

    /// Handle a UI command dispatched from the tokio thread.
    pub fn handle_command(&self, cmd: UiCommand) {
        match cmd {
            UiCommand::ModeShow {
                mode,
                mode_config,
                mode_configs,
                ui_config,
            } => {
                tracing::info!(?mode, "showing mode overlay");
                let mut s = self.overlay_state.borrow_mut();
                s.mode_state.update_modes(mode_configs);
                s.ui_config = ui_config.clone();
                s.exit_on_key_release = None;

                // Push the requested mode (or first mode)
                s.mode_state.clear();
                if let Some(ref config) = mode_config {
                    s.mode_state.push_mode(&config.name);
                }

                if let Some(ref config) = mode_config {
                    mode_overlay::rebuild_mode(&self.mode_window, config, &ui_config);
                }
                drop(s);
                self.mode_window.present();
            }
            UiCommand::ModeHide => {
                tracing::info!("hiding mode overlay");
                let mut s = self.overlay_state.borrow_mut();
                s.mode_state.clear();
                s.exit_on_key_release = None;
                drop(s);
                self.mode_window.set_visible(false);
            }
            UiCommand::ModeToggle {
                mode,
                mode_config,
                mode_configs,
                ui_config,
            } => {
                if self.mode_window.is_visible() {
                    tracing::info!("toggling mode overlay: hiding");
                    let mut s = self.overlay_state.borrow_mut();
                    s.mode_state.clear();
                    s.exit_on_key_release = None;
                    drop(s);
                    self.mode_window.set_visible(false);
                } else {
                    // Delegate to ModeShow logic
                    self.handle_command(UiCommand::ModeShow {
                        mode,
                        mode_config,
                        mode_configs,
                        ui_config,
                    });
                }
            }
            UiCommand::ScratchpadPick => {
                tracing::info!("scratchpad picker (not yet implemented)");
            }
        }
    }
}

/// Convert a GDK keyval to a key name string suitable for config lookup.
fn keyval_to_key_name(keyval: gtk4::gdk::Key) -> String {
    keyval.name().map(|s| s.to_string()).unwrap_or_default()
}

/// Spawn a shell command detached from the daemon process.
fn spawn_sh(cmd: &str) {
    tracing::info!(cmd, "spawning shell command");
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => {}
        Err(e) => tracing::error!(%e, cmd, "failed to spawn shell command"),
    }
}

/// Spawn a process detached from the daemon.
fn spawn_process(args: &[String]) {
    if args.is_empty() {
        return;
    }
    tracing::info!(?args, "spawning process");
    match std::process::Command::new(&args[0])
        .args(&args[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => {}
        Err(e) => tracing::error!(%e, ?args, "failed to spawn process"),
    }
}

/// Convert a `BindAction` to a daemon `Command` for forwarding to the tokio thread.
fn bind_action_to_command(action: &BindAction) -> Option<Command> {
    match action {
        BindAction::ScratchpadToggle(name) => Some(Command::Toggle { name: name.clone() }),
        BindAction::ScratchpadHide => Some(Command::Hide),
        BindAction::ScratchpadFloat(name) => Some(Command::Float { name: name.clone() }),
        BindAction::ScratchpadTile(name) => Some(Command::Tile { name: name.clone() }),
        BindAction::ScratchpadToggleFloat => Some(Command::ToggleFloat { name: None }),
        BindAction::ScratchpadPick => Some(Command::ScratchpadPick),
        BindAction::NiriAction { name, args } => {
            // For niri actions, we spawn directly via niri msg
            spawn_niri_action(name, args);
            None
        }
        // SpawnSh, Spawn, SwitchMode are handled directly in the key handler
        _ => None,
    }
}

/// Execute a niri action via `niri msg action`.
fn spawn_niri_action(name: &str, args: &[String]) {
    tracing::info!(name, ?args, "executing niri action");
    let mut cmd_args = vec!["msg", "action", name];
    let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    cmd_args.extend(args_refs);

    match std::process::Command::new("niri")
        .args(&cmd_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(_) => {}
        Err(e) => tracing::error!(%e, name, "failed to execute niri action"),
    }
}
