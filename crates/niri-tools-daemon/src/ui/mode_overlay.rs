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

    // Set a placeholder child so GTK has something to measure during
    // the initial layout pass triggered by init_layer_shell().
    // Without this, GTK warns: "Allocating size to GtkBox without
    // calling gtk_widget_measure()".
    let placeholder = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    placeholder.set_size_request(1, 1);
    window.set_child(Some(&placeholder));

    tracing::info!("mode overlay window created (hidden)");

    window
}

/// Rebuild the overlay widget tree to display binds from the given mode.
///
/// Layout: binds flow horizontally (like a status bar) and wrap to the next
/// row if they exceed the window width. This matches wlr-which-key's
/// `rows_per_column: 1` behavior.
///
/// `column_padding` controls spacing between entries.
/// `min_width` sets a minimum width for the container.
pub fn rebuild_mode(window: &ApplicationWindow, mode: &ModeConfig, ui_config: &UiConfig) {
    let separator = ui_config.modes.separator.as_deref().unwrap_or("  ");
    let column_padding = ui_config.modes.column_padding.unwrap_or(50.0) as i32;
    let min_width = ui_config.modes.min_width.unwrap_or(0.0) as i32;

    // FlowBox: horizontal flow with automatic wrapping
    let flow = gtk4::FlowBox::new();
    flow.set_orientation(gtk4::Orientation::Horizontal);
    flow.set_selection_mode(gtk4::SelectionMode::None);
    flow.set_homogeneous(false);
    flow.set_column_spacing(column_padding as u32);
    flow.set_row_spacing(0);
    flow.set_min_children_per_line(1);
    flow.set_max_children_per_line(mode.binds.len().max(1) as u32);
    flow.add_css_class("mode-flow");

    for bind in &mode.binds {
        let entry = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);

        let key_label = Label::new(Some(&bind.key));
        key_label.add_css_class("mode-key");
        entry.append(&key_label);

        let sep_label = Label::new(Some(separator));
        sep_label.add_css_class("mode-sep");
        entry.append(&sep_label);

        let desc_label = Label::new(Some(&bind.description));
        desc_label.add_css_class("mode-desc");
        if matches!(&bind.action, BindAction::SwitchMode(_)) {
            desc_label.add_css_class("mode-desc-mode");
        }
        entry.append(&desc_label);

        flow.insert(&entry, -1);
    }

    // Outer container with background styling and min-width
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.add_css_class("mode-container");
    if min_width > 0 {
        container.set_size_request(min_width, -1);
    }
    container.append(&flow);

    // Reset the window's default size so GTK re-measures from the new content
    // rather than trying to fit it into the old allocation.
    window.set_default_size(-1, -1);
    window.set_child(Some(&container));
}
