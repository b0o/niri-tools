/// Manages both GTK4 layer-shell windows (mode overlay and scratchpad picker).
///
/// Created on the GTK main thread during `connect_activate`. Both windows
/// start hidden and are shown/hidden via IPC commands forwarded from the
/// tokio background thread.
pub struct UiManager {
    _app: gtk4::Application,
}

impl UiManager {
    pub fn new(app: &gtk4::Application) -> Self {
        tracing::info!("UI manager initialized (no windows yet)");
        Self { _app: app.clone() }
    }
}
