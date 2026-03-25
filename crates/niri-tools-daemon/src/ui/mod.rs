mod mode_overlay;

use gtk4::prelude::*;

/// Commands sent from the tokio thread to the GTK main thread.
#[derive(Debug)]
pub enum UiCommand {
    ModeShow { mode: Option<String> },
    ModeHide,
    ModeToggle { mode: Option<String> },
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
    pub fn new(app: &gtk4::Application) -> Self {
        let mode_window = mode_overlay::create_mode_overlay(app);
        tracing::info!("UI manager initialized");
        Self { mode_window }
    }

    /// Handle a UI command dispatched from the tokio thread.
    pub fn handle_command(&self, cmd: UiCommand) {
        match cmd {
            UiCommand::ModeShow { mode } => {
                tracing::info!(?mode, "showing mode overlay");
                self.mode_window.present();
            }
            UiCommand::ModeHide => {
                tracing::info!("hiding mode overlay");
                self.mode_window.set_visible(false);
            }
            UiCommand::ModeToggle { mode } => {
                if self.mode_window.is_visible() {
                    tracing::info!("toggling mode overlay: hiding");
                    self.mode_window.set_visible(false);
                } else {
                    tracing::info!(?mode, "toggling mode overlay: showing");
                    self.mode_window.present();
                }
            }
            UiCommand::ScratchpadPick => {
                tracing::info!("scratchpad picker (not yet implemented)");
            }
        }
    }
}
