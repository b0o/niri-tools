use std::cell::RefCell;
use std::rc::Rc;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Label};
use gtk4_layer_shell::LayerShell;
use niri_tools_common::config::UiConfig;
use niri_tools_common::protocol::Command;

use super::{PickerEntry, PickerEntryState};

/// Shared state for the scratchpad picker.
pub struct PickerState {
    pub entries: Vec<PickerEntry>,
    pub search_buffer: String,
    pub selected_index: usize,
    pub filtered_indices: Vec<usize>,
    pub daemon_tx: tokio::sync::mpsc::Sender<Command>,
}

impl PickerState {
    pub fn new(daemon_tx: tokio::sync::mpsc::Sender<Command>) -> Self {
        Self {
            entries: Vec::new(),
            search_buffer: String::new(),
            selected_index: 0,
            filtered_indices: Vec::new(),
            daemon_tx,
        }
    }

    pub fn set_entries(&mut self, entries: Vec<PickerEntry>) {
        self.entries = entries;
        self.search_buffer.clear();
        self.selected_index = 0;
        self.update_filtered();
    }

    /// Update filtered indices based on current search buffer.
    pub fn update_filtered(&mut self) {
        if self.search_buffer.is_empty() {
            self.filtered_indices = (0..self.entries.len()).collect();
        } else {
            let matcher = SkimMatcherV2::default();
            let mut scored: Vec<(usize, i64)> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(idx, entry)| {
                    let display = entry.desc.as_deref().unwrap_or(&entry.name);
                    matcher
                        .fuzzy_match(display, &self.search_buffer)
                        .map(|score| (idx, score))
                })
                .collect();
            // Sort by score descending
            scored.sort_by(|a, b| b.1.cmp(&a.1));
            self.filtered_indices = scored.into_iter().map(|(idx, _)| idx).collect();
        }

        // Clamp selection
        if self.filtered_indices.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = self.filtered_indices.len() - 1;
        }
    }

    /// Get the currently selected entry, if any.
    pub fn selected_entry(&self) -> Option<&PickerEntry> {
        self.filtered_indices
            .get(self.selected_index)
            .and_then(|&idx| self.entries.get(idx))
    }
}

/// Create the scratchpad picker layer-shell window.
pub fn create_picker_window(app: &gtk4::Application, ui_config: &UiConfig) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("niri-tools-scratchpad-picker")
        .resizable(false)
        .decorated(false)
        .default_width(400)
        .build();

    window.init_layer_shell();
    window.set_layer(gtk4_layer_shell::Layer::Overlay);
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::Exclusive);
    window.set_namespace(Some("niri-tools-scratchpad-picker"));

    // Anchor from config (default: center = no anchors)
    let anchor = ui_config.scratchpads.anchor.as_deref().unwrap_or("center");
    match anchor {
        "top" => window.set_anchor(gtk4_layer_shell::Edge::Top, true),
        "bottom" => window.set_anchor(gtk4_layer_shell::Edge::Bottom, true),
        "center" => {} // No anchors = centered
        _ => {}
    }

    tracing::info!("scratchpad picker window created (hidden)");
    window
}

/// Rebuild the picker list widget from the current state.
pub fn rebuild_picker_list(window: &ApplicationWindow, state: &Rc<RefCell<PickerState>>) {
    let s = state.borrow();

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    container.add_css_class("mode-container"); // Reuse mode container style

    // Search indicator
    if !s.search_buffer.is_empty() {
        let search_label = Label::new(Some(&format!("/{}", s.search_buffer)));
        search_label.add_css_class("mode-desc");
        search_label.set_halign(gtk4::Align::Start);
        container.append(&search_label);

        let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        container.append(&sep);
    }

    // Filtered entries
    for (display_idx, &entry_idx) in s.filtered_indices.iter().enumerate() {
        let entry = &s.entries[entry_idx];
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);

        // Key shortcut
        let key_text = entry
            .key
            .as_ref()
            .map(|k| format!("[{}]", k))
            .unwrap_or_else(|| "[ ]".to_string());
        let key_label = Label::new(Some(&key_text));
        key_label.add_css_class("mode-key");
        row.append(&key_label);

        // Name/description
        let display_name = entry.desc.as_deref().unwrap_or(&entry.name);
        let name_label = Label::new(Some(display_name));
        name_label.add_css_class("mode-desc");
        row.append(&name_label);

        // State indicator
        let (state_text, state_class) = match entry.state {
            PickerEntryState::Visible => ("●", "state-visible"),
            PickerEntryState::Floating => ("◆", "state-floating"),
            PickerEntryState::Hidden => ("○", "mode-desc"),
            PickerEntryState::Unspawned => ("·", "state-unspawned"),
        };
        let state_label = Label::new(Some(state_text));
        state_label.add_css_class(state_class);
        row.append(&state_label);

        // Highlight selected row
        if display_idx == s.selected_index {
            row.add_css_class("mode-desc-mode");
        }

        container.append(&row);
    }

    if s.filtered_indices.is_empty() && !s.entries.is_empty() {
        let empty_label = Label::new(Some("No matches"));
        empty_label.add_css_class("state-unspawned");
        container.append(&empty_label);
    }

    window.set_child(Some(&container));
}

/// Attach keyboard handler to the picker window.
pub fn attach_picker_keyboard(window: &ApplicationWindow, state: &Rc<RefCell<PickerState>>) {
    let key_controller = gtk4::EventControllerKey::new();

    {
        let state = state.clone();
        let window = window.clone();
        key_controller.connect_key_pressed(move |_, keyval, _keycode, modifiers| {
            handle_picker_key(&window, &state, keyval, modifiers)
        });
    }

    window.add_controller(key_controller);
}

fn handle_picker_key(
    window: &ApplicationWindow,
    state: &Rc<RefCell<PickerState>>,
    keyval: gtk4::gdk::Key,
    modifiers: gtk4::gdk::ModifierType,
) -> gtk4::glib::Propagation {
    let key_name = keyval.name().map(|s| s.to_string()).unwrap_or_default();

    // Escape: close
    if key_name == "Escape" {
        window.set_visible(false);
        return gtk4::glib::Propagation::Stop;
    }

    // Mod+key: instant toggle via shortcut
    let has_super = modifiers.contains(gtk4::gdk::ModifierType::META_MASK)
        || modifiers.contains(gtk4::gdk::ModifierType::SUPER_MASK);
    if has_super {
        let s = state.borrow_mut();
        if let Some(entry) = s.entries.iter().find(|e| {
            e.key
                .as_ref()
                .is_some_and(|k| k.eq_ignore_ascii_case(&key_name))
        }) {
            let name = entry.name.clone();
            let _ = s.daemon_tx.try_send(Command::Toggle { name: Some(name) });
            drop(s);
            window.set_visible(false);
        }
        return gtk4::glib::Propagation::Stop;
    }

    // Enter: toggle selected
    if key_name == "Return" || key_name == "KP_Enter" {
        let s = state.borrow();
        if let Some(entry) = s.selected_entry() {
            let name = entry.name.clone();
            let _ = s.daemon_tx.try_send(Command::Toggle { name: Some(name) });
            drop(s);
            window.set_visible(false);
        }
        return gtk4::glib::Propagation::Stop;
    }

    // Up/Down: navigate
    if key_name == "Up"
        || key_name == "k" && modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK)
    {
        let mut s = state.borrow_mut();
        if s.selected_index > 0 {
            s.selected_index -= 1;
        }
        drop(s);
        rebuild_picker_list(window, state);
        return gtk4::glib::Propagation::Stop;
    }
    if key_name == "Down"
        || key_name == "j" && modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK)
    {
        let mut s = state.borrow_mut();
        if s.selected_index + 1 < s.filtered_indices.len() {
            s.selected_index += 1;
        }
        drop(s);
        rebuild_picker_list(window, state);
        return gtk4::glib::Propagation::Stop;
    }

    // Backspace: delete from search buffer
    if key_name == "BackSpace" {
        let mut s = state.borrow_mut();
        s.search_buffer.pop();
        s.update_filtered();
        drop(s);
        rebuild_picker_list(window, state);
        return gtk4::glib::Propagation::Stop;
    }

    // Regular character: add to search buffer
    if let Some(ch) = keyval.to_unicode() {
        if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' {
            let mut s = state.borrow_mut();
            s.search_buffer.push(ch);
            s.update_filtered();
            drop(s);
            rebuild_picker_list(window, state);
        }
    }

    gtk4::glib::Propagation::Stop
}
