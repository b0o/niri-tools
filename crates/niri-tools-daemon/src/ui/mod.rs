mod css;
mod mode_overlay;

use gtk4::prelude::*;
use niri_tools_common::config::{ModeConfig, UiConfig};

/// Commands sent from the tokio thread to the GTK main thread.
#[derive(Debug)]
pub enum UiCommand {
    ModeShow {
        mode: Option<String>,
        mode_config: Option<ModeConfig>,
        ui_config: UiConfig,
    },
    ModeHide,
    ModeToggle {
        mode: Option<String>,
        mode_config: Option<ModeConfig>,
        ui_config: UiConfig,
    },
    ScratchpadPick,
}

/// Manages both GTK4 layer-shell windows (mode overlay and scratchpad picker).
///
/// Created on the GTK main thread during `connect_activate`. Both windows
/// start hidden and are shown/hidden via IPC commands forwarded from the
/// tokio background thread.
pub struct UiManager {
    mode_window: gtk4::ApplicationWindow,
}

impl UiManager {
    pub fn new(app: &gtk4::Application, ui_config: &UiConfig) -> Self {
        let mode_window = mode_overlay::create_mode_overlay(app, ui_config);
        tracing::info!("UI manager initialized");
        Self { mode_window }
    }

    /// Handle a UI command dispatched from the tokio thread.
    pub fn handle_command(&self, cmd: UiCommand) {
        match cmd {
            UiCommand::ModeShow {
                mode,
                mode_config,
                ui_config,
            } => {
                tracing::info!(?mode, "showing mode overlay");
                if let Some(ref config) = mode_config {
                    mode_overlay::rebuild_mode(&self.mode_window, config, &ui_config);
                }
                self.mode_window.present();
            }
            UiCommand::ModeHide => {
                tracing::info!("hiding mode overlay");
                self.mode_window.set_visible(false);
            }
            UiCommand::ModeToggle {
                mode,
                mode_config,
                ui_config,
            } => {
                if self.mode_window.is_visible() {
                    tracing::info!("toggling mode overlay: hiding");
                    self.mode_window.set_visible(false);
                } else {
                    tracing::info!(?mode, "toggling mode overlay: showing");
                    if let Some(ref config) = mode_config {
                        mode_overlay::rebuild_mode(&self.mode_window, config, &ui_config);
                    }
                    self.mode_window.present();
                }
            }
            UiCommand::ScratchpadPick => {
                tracing::info!("scratchpad picker (not yet implemented)");
            }
        }
    }
}
