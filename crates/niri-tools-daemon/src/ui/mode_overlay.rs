use gtk4::prelude::*;
use gtk4::{ApplicationWindow, CssProvider, Label};
use gtk4_layer_shell::LayerShell;
use niri_tools_common::config::{BindAction, ModeConfig, UiConfig};
use niri_tools_common::niri_config;

use super::css;

/// Creates the mode overlay layer-shell window.
///
/// The window starts hidden. Call `window.present()` to show it,
/// `window.set_visible(false)` to hide it.
pub fn create_mode_overlay(app: &gtk4::Application, ui_config: &UiConfig) -> ApplicationWindow {
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

    // Anchor from config
    let anchor = ui_config.modes.anchor.as_deref().unwrap_or("bottom");
    match anchor {
        "top" => window.set_anchor(gtk4_layer_shell::Edge::Top, true),
        "bottom" => window.set_anchor(gtk4_layer_shell::Edge::Bottom, true),
        _ => window.set_anchor(gtk4_layer_shell::Edge::Bottom, true),
    }

    // Margins from config
    if let Some(m) = ui_config.modes.margin_top {
        window.set_margin(gtk4_layer_shell::Edge::Top, m);
    }
    if let Some(m) = ui_config.modes.margin_right {
        window.set_margin(gtk4_layer_shell::Edge::Right, m);
    }
    if let Some(m) = ui_config.modes.margin_bottom {
        window.set_margin(gtk4_layer_shell::Edge::Bottom, m);
    }
    if let Some(m) = ui_config.modes.margin_left {
        window.set_margin(gtk4_layer_shell::Edge::Left, m);
    }

    // Load CSS with niri style hints
    let hints = niri_config::read_niri_style_hints();
    let css_text = css::generate_css(ui_config, &hints);
    let provider = CssProvider::new();
    provider.load_from_data(&css_text);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("Could not connect to a display"),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    tracing::info!("mode overlay window created (hidden)");

    window
}

/// Rebuild the overlay widget tree to display binds from the given mode.
///
/// The separator between key and description, and the column padding,
/// are controlled by `UiConfig`.
pub fn rebuild_mode(window: &ApplicationWindow, mode: &ModeConfig, ui_config: &UiConfig) {
    let separator = ui_config.modes.separator.as_deref().unwrap_or("  ");

    let column_padding = ui_config.modes.column_padding.unwrap_or(50.0) as i32;

    // Outer container (horizontal box of columns)
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, column_padding);
    container.add_css_class("mode-container");

    // We pack all binds into a single column for now.
    // Multi-column layout can be added later based on min_width.
    let column = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    column.add_css_class("mode-column");

    for bind in &mode.binds {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);

        let key_label = Label::new(Some(&bind.key));
        key_label.add_css_class("mode-key");
        row.append(&key_label);

        let sep_label = Label::new(Some(separator));
        sep_label.add_css_class("mode-sep");
        row.append(&sep_label);

        let desc_label = Label::new(Some(&bind.description));
        desc_label.add_css_class("mode-desc");
        // Accent color for switch-mode entries
        if matches!(&bind.action, BindAction::SwitchMode(_)) {
            desc_label.add_css_class("mode-desc-mode");
        }
        row.append(&desc_label);

        column.append(&row);
    }

    container.append(&column);
    window.set_child(Some(&container));
}
