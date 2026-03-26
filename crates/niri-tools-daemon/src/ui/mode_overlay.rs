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

    // Placeholder child for initial layout pass
    let placeholder = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    placeholder.set_size_request(1, 1);
    window.set_child(Some(&placeholder));

    tracing::info!("mode overlay window created (hidden)");

    window
}

/// Rebuild the overlay widget tree to display binds from the given mode.
///
/// Layout: a single horizontal row of entries, centered within the container.
/// Each entry shows `key description` as a cohesive unit with clear visual
/// separation between entries via dot separators.
///
/// When in a sub-mode, a breadcrumb is shown (e.g., "root > brightness").
pub fn rebuild_mode(
    window: &ApplicationWindow,
    mode: &ModeConfig,
    ui_config: &UiConfig,
    breadcrumb: Option<&str>,
) {
    let min_width = ui_config.modes.min_width.unwrap_or(0.0) as i32;
    let padding = ui_config.modes.padding.unwrap_or(4.0) as i32;

    // Outer container: background, padding, min-width
    let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    container.add_css_class("mode-container");
    container.set_halign(gtk4::Align::Center);
    if min_width > 0 {
        container.set_size_request(min_width, -1);
    }

    // Inner row: entries centered with even spacing
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    row.set_halign(gtk4::Align::Center);
    row.set_hexpand(true);
    row.set_margin_start(padding);
    row.set_margin_end(padding);
    row.set_margin_top(padding);
    row.set_margin_bottom(padding);

    // Breadcrumb when in a sub-mode
    if let Some(crumb) = breadcrumb {
        let crumb_label = Label::new(Some(crumb));
        crumb_label.add_css_class("mode-breadcrumb");
        row.append(&crumb_label);

        let sep = Label::new(Some("│"));
        sep.add_css_class("mode-entry-sep");
        row.append(&sep);
    }

    let visible_binds: Vec<_> = mode
        .binds
        .iter()
        .filter(|b| {
            !b.options
                .contains(&niri_tools_common::config::BindOption::Hide)
        })
        .collect();

    for (i, bind) in visible_binds.iter().enumerate() {
        if i > 0 {
            let spacer = Label::new(Some("·"));
            spacer.add_css_class("mode-entry-sep");
            row.append(&spacer);
        }

        let entry = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        entry.add_css_class("mode-entry");

        let key_label = Label::new(Some(&bind.key));
        key_label.add_css_class("mode-key");
        entry.append(&key_label);

        let desc_label = Label::new(Some(&bind.description));
        desc_label.add_css_class("mode-desc");
        if matches!(&bind.action, BindAction::SwitchMode(_)) {
            desc_label.add_css_class("mode-desc-mode");
        }
        entry.append(&desc_label);

        row.append(&entry);
    }

    container.append(&row);

    window.set_default_size(-1, -1);
    window.set_child(Some(&container));
}
