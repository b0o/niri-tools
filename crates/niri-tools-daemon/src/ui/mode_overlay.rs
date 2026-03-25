use gtk4::prelude::*;
use gtk4::ApplicationWindow;
use gtk4_layer_shell::LayerShell;

/// Creates the mode overlay layer-shell window.
///
/// The window starts hidden. Call `window.present()` to show it,
/// `window.set_visible(false)` to hide it.
pub fn create_mode_overlay(app: &gtk4::Application) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("niri-tools-mode")
        .resizable(false)
        .decorated(false)
        .build();

    // Layer shell setup
    window.init_layer_shell();
    window.set_layer(gtk4_layer_shell::Layer::Overlay);
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::Exclusive);
    window.set_namespace(Some("niri-tools-mode"));

    // Default anchor: bottom center
    window.set_anchor(gtk4_layer_shell::Edge::Bottom, true);

    // Placeholder content (will be replaced in Task 2.3)
    let label = gtk4::Label::new(Some("Mode overlay (placeholder)"));
    window.set_child(Some(&label));

    tracing::info!("mode overlay window created (hidden)");

    window
}
