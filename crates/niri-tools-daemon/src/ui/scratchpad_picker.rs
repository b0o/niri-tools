use std::cell::RefCell;
use std::rc::Rc;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
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
    /// Keycode that triggered a close action. The picker hides on release.
    pub exit_on_key_release: Option<u32>,
}

impl PickerState {
    pub fn new(daemon_tx: tokio::sync::mpsc::Sender<Command>) -> Self {
        Self {
            entries: Vec::new(),
            search_buffer: String::new(),
            selected_index: 0,
            filtered_indices: Vec::new(),
            daemon_tx,
            exit_on_key_release: None,
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

    // Set a placeholder child so GTK has something to measure during
    // the initial layout pass triggered by init_layer_shell().
    let placeholder = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    placeholder.set_size_request(1, 1);
    window.set_child(Some(&placeholder));

    tracing::info!("scratchpad picker window created (hidden)");
    window
}

/// Rebuild the picker list widget from the current state.
pub fn rebuild_picker_list(window: &ApplicationWindow, state: &Rc<RefCell<PickerState>>) {
    let s = state.borrow();

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    container.add_css_class("picker-container");

    // Search bar (always visible -- shows "/" prompt when empty)
    let search_text = if s.search_buffer.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", s.search_buffer)
    };
    let search_label = Label::new(Some(&search_text));
    search_label.add_css_class("picker-search");
    search_label.set_halign(gtk4::Align::Start);
    if s.search_buffer.is_empty() {
        search_label.add_css_class("picker-search-empty");
    }
    container.append(&search_label);

    // Filtered entries
    for (display_idx, &entry_idx) in s.filtered_indices.iter().enumerate() {
        let entry = &s.entries[entry_idx];
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        row.add_css_class("picker-row");

        if display_idx == s.selected_index {
            row.add_css_class("picker-row-selected");
        }

        // State indicator (left side)
        let state_char = match entry.state {
            PickerEntryState::Visible => "●",
            PickerEntryState::Floating => "◆",
            PickerEntryState::Hidden => "○",
            PickerEntryState::Unspawned => " ",
        };
        let state_label = Label::new(Some(state_char));
        state_label.add_css_class("picker-state");
        match entry.state {
            PickerEntryState::Visible => state_label.add_css_class("state-visible"),
            PickerEntryState::Floating => state_label.add_css_class("state-floating"),
            PickerEntryState::Unspawned => state_label.add_css_class("state-unspawned"),
            _ => {}
        }
        row.append(&state_label);

        // Key shortcut
        if let Some(ref key) = entry.key {
            let key_label = Label::new(Some(key));
            key_label.add_css_class("picker-key");
            row.append(&key_label);
        } else {
            let spacer = Label::new(Some(" "));
            spacer.add_css_class("picker-key");
            row.append(&spacer);
        }

        // Name
        let display_name = entry.desc.as_deref().unwrap_or(&entry.name);
        let name_label = Label::new(Some(display_name));
        name_label.add_css_class("picker-name");
        name_label.set_hexpand(true);
        name_label.set_halign(gtk4::Align::Start);
        row.append(&name_label);

        container.append(&row);
    }

    if s.filtered_indices.is_empty() && !s.entries.is_empty() {
        let empty_label = Label::new(Some("  no matches"));
        empty_label.add_css_class("state-unspawned");
        container.append(&empty_label);
    }

    // Reset the window's default size so GTK re-measures from the new content
    // rather than trying to fit it into the old allocation.
    window.set_default_size(-1, -1);
    window.set_child(Some(&container));
}

/// Attach keyboard handler to the picker window.
pub fn attach_picker_keyboard(window: &ApplicationWindow, state: &Rc<RefCell<PickerState>>) {
    let key_controller = gtk4::EventControllerKey::new();

    {
        let state = state.clone();
        let window = window.clone();
        key_controller.connect_key_pressed(move |_, keyval, keycode, modifiers| {
            handle_picker_key(&window, &state, keyval, keycode, modifiers)
        });
    }

    {
        let state = state.clone();
        let window = window.clone();
        key_controller.connect_key_released(move |_, _keyval, keycode, _modifiers| {
            let mut s = state.borrow_mut();
            if s.exit_on_key_release == Some(keycode) {
                s.exit_on_key_release = None;
                drop(s);
                window.set_visible(false);
            }
        });
    }

    window.add_controller(key_controller);
}

fn handle_picker_key(
    window: &ApplicationWindow,
    state: &Rc<RefCell<PickerState>>,
    keyval: gtk4::gdk::Key,
    keycode: u32,
    modifiers: gtk4::gdk::ModifierType,
) -> gtk4::glib::Propagation {
    let key_name = keyval.name().map(|s| s.to_string()).unwrap_or_default();

    // If waiting for a key release to exit, ignore new presses
    {
        let s = state.borrow();
        if s.exit_on_key_release.is_some() {
            return gtk4::glib::Propagation::Stop;
        }
    }

    // Escape: clear search first, then close
    if key_name == "Escape" {
        let mut s = state.borrow_mut();
        if !s.search_buffer.is_empty() {
            s.search_buffer.clear();
            s.update_filtered();
            drop(s);
            rebuild_picker_list(window, state);
        } else {
            s.exit_on_key_release = Some(keycode);
        }
        return gtk4::glib::Propagation::Stop;
    }

    // Ctrl+[ and Ctrl+g also close (vim convention)
    let has_ctrl = modifiers.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
    if has_ctrl && (key_name == "bracketleft" || key_name == "g") {
        let mut s = state.borrow_mut();
        s.exit_on_key_release = Some(keycode);
        return gtk4::glib::Propagation::Stop;
    }

    // Mod+key: instant toggle via shortcut
    let has_super = modifiers.contains(gtk4::gdk::ModifierType::META_MASK)
        || modifiers.contains(gtk4::gdk::ModifierType::SUPER_MASK);
    if has_super {
        let mut s = state.borrow_mut();
        if let Some(entry) = s.entries.iter().find(|e| {
            e.key
                .as_ref()
                .is_some_and(|k| k.eq_ignore_ascii_case(&key_name))
        }) {
            let name = entry.name.clone();
            let _ = s.daemon_tx.try_send(Command::Toggle { name: Some(name) });
            s.exit_on_key_release = Some(keycode);
        }
        return gtk4::glib::Propagation::Stop;
    }

    // Enter: toggle selected and close
    if key_name == "Return" || key_name == "KP_Enter" {
        let mut s = state.borrow_mut();
        if let Some(entry) = s.selected_entry() {
            let name = entry.name.clone();
            let _ = s.daemon_tx.try_send(Command::Toggle { name: Some(name) });
            s.exit_on_key_release = Some(keycode);
        }
        return gtk4::glib::Propagation::Stop;
    }

    // Up/Ctrl+k/Ctrl+p: navigate up
    if key_name == "Up" || (has_ctrl && key_name == "k") || (has_ctrl && key_name == "p") {
        let mut s = state.borrow_mut();
        if s.selected_index > 0 {
            s.selected_index -= 1;
        }
        drop(s);
        rebuild_picker_list(window, state);
        return gtk4::glib::Propagation::Stop;
    }
    // Down/Ctrl+j/Ctrl+n: navigate down
    if key_name == "Down" || (has_ctrl && key_name == "j") || (has_ctrl && key_name == "n") {
        let mut s = state.borrow_mut();
        if s.selected_index + 1 < s.filtered_indices.len() {
            s.selected_index += 1;
        }
        drop(s);
        rebuild_picker_list(window, state);
        return gtk4::glib::Propagation::Stop;
    }

    // Tab: toggle selected without closing (for toggling multiple)
    if key_name == "Tab" {
        let s = state.borrow();
        if let Some(entry) = s.selected_entry() {
            let name = entry.name.clone();
            let _ = s.daemon_tx.try_send(Command::Toggle { name: Some(name) });
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entries() -> Vec<PickerEntry> {
        vec![
            PickerEntry {
                name: "term".to_string(),
                key: Some("t".to_string()),
                desc: Some("Terminal".to_string()),
                state: PickerEntryState::Hidden,
            },
            PickerEntry {
                name: "browser".to_string(),
                key: Some("b".to_string()),
                desc: Some("Web Browser".to_string()),
                state: PickerEntryState::Visible,
            },
            PickerEntry {
                name: "music-tidal".to_string(),
                key: None,
                desc: Some("Tidal Music".to_string()),
                state: PickerEntryState::Unspawned,
            },
            PickerEntry {
                name: "volume".to_string(),
                key: Some("v".to_string()),
                desc: None,
                state: PickerEntryState::Floating,
            },
        ]
    }

    fn make_picker() -> PickerState {
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut ps = PickerState::new(tx);
        ps.set_entries(make_entries());
        ps
    }

    #[test]
    fn set_entries_shows_all() {
        let ps = make_picker();
        assert_eq!(ps.filtered_indices.len(), 4);
        assert_eq!(ps.selected_index, 0);
        assert!(ps.search_buffer.is_empty());
    }

    #[test]
    fn fuzzy_filter_narrows_results() {
        let mut ps = make_picker();
        ps.search_buffer = "term".to_string();
        ps.update_filtered();
        assert_eq!(ps.filtered_indices.len(), 1);
        assert_eq!(ps.entries[ps.filtered_indices[0]].name, "term");
    }

    #[test]
    fn fuzzy_filter_matches_desc() {
        let mut ps = make_picker();
        ps.search_buffer = "tidal".to_string();
        ps.update_filtered();
        assert_eq!(ps.filtered_indices.len(), 1);
        assert_eq!(ps.entries[ps.filtered_indices[0]].name, "music-tidal");
    }

    #[test]
    fn fuzzy_filter_no_match() {
        let mut ps = make_picker();
        ps.search_buffer = "zzzzz".to_string();
        ps.update_filtered();
        assert!(ps.filtered_indices.is_empty());
    }

    #[test]
    fn selected_entry_returns_correct_item() {
        let ps = make_picker();
        let entry = ps.selected_entry().unwrap();
        assert_eq!(entry.name, "term");
    }

    #[test]
    fn selected_entry_after_navigation() {
        let mut ps = make_picker();
        ps.selected_index = 2;
        let entry = ps.selected_entry().unwrap();
        assert_eq!(entry.name, "music-tidal");
    }

    #[test]
    fn selected_index_clamped_on_filter() {
        let mut ps = make_picker();
        ps.selected_index = 3; // last item
        ps.search_buffer = "term".to_string();
        ps.update_filtered();
        // Should clamp to 0 since only 1 result
        assert_eq!(ps.selected_index, 0);
    }

    #[test]
    fn clear_search_shows_all() {
        let mut ps = make_picker();
        ps.search_buffer = "term".to_string();
        ps.update_filtered();
        assert_eq!(ps.filtered_indices.len(), 1);

        ps.search_buffer.clear();
        ps.update_filtered();
        assert_eq!(ps.filtered_indices.len(), 4);
    }

    #[test]
    fn display_name_uses_desc_then_name() {
        let ps = make_picker();
        // "volume" has no desc, so name should be used for matching
        let volume = &ps.entries[3];
        assert!(volume.desc.is_none());
        assert_eq!(volume.name, "volume");
    }
}
