# Modes & Scratchpad Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two GTK4 layer-shell UIs to the niri-tools daemon: a which-key-style mode overlay for action dispatch, and a fuzzy-searchable scratchpad picker with live state.

**Architecture:** GTK4 owns the main thread (`app.run()`). The existing tokio event loop (socket listener, niri event stream) moves to a background `tokio::Runtime` via `OnceLock`. Commands bridge from tokio to GTK via `glib::spawn_future_local()`. Two layer-shell windows are pre-initialized at startup and shown/hidden via IPC.

**Tech Stack:** Rust, GTK4, gtk4-layer-shell, gdk4-wayland, frizbee (fuzzy matching), KDL config, tokio, bincode IPC.

**Spec:** `docs/specs/2026-03-25-modes-and-scratchpad-picker-design.md`

---

## File Structure

### New files

| File | Responsibility |
|------|----------------|
| `crates/niri-tools-common/src/config/mod.rs` | Re-exports; replaces current flat `config.rs` |
| `crates/niri-tools-common/src/config/scratchpad.rs` | `ScratchpadConfig`, `SizeConfig`, `PositionConfig`, `OutputOverride` (moved from `config.rs`) |
| `crates/niri-tools-common/src/config/mode.rs` | `ModeConfig`, `BindConfig`, `BindAction`, `BindOption` |
| `crates/niri-tools-common/src/config/ui.rs` | `UiConfig`, `ModesUiConfig`, `ScratchpadsUiConfig` |
| `crates/niri-tools-common/src/config/settings.rs` | `DaemonSettings`, `NotifyLevel` (moved from `config.rs`) |
| `crates/niri-tools-daemon/src/ui/mod.rs` | `UiManager`: owns both GTK windows, bridges IPC to GTK |
| `crates/niri-tools-daemon/src/ui/mode_overlay.rs` | Mode overlay window: layer-shell setup, widget tree, key dispatch |
| `crates/niri-tools-daemon/src/ui/scratchpad_picker.rs` | Scratchpad picker window: list, fuzzy search, state indicators |
| `crates/niri-tools-daemon/src/ui/css.rs` | CSS generation from resolved `UiConfig` |
| `crates/niri-tools-daemon/src/mode.rs` | `ModeState`: mode stack, navigation, bind lookup |

### Modified files

| File | Changes |
|------|---------|
| `crates/niri-tools-common/src/lib.rs` | Update `config` module path |
| `crates/niri-tools-common/src/config_parser.rs` | Add `"mode"`, `"ui"`, `"notifications"` KDL node handlers; parse `key`/`desc` on scratchpads |
| `crates/niri-tools-common/src/protocol.rs` | Add `ModeShow`, `ModeHide`, `ModeToggle`, `ScratchpadPick` to `Command` |
| `crates/niri-tools-common/Cargo.toml` | No changes expected |
| `crates/niri-tools-daemon/Cargo.toml` | Add `gtk4`, `gtk4-layer-shell`, `gdk4-wayland`, `frizbee` deps |
| `crates/niri-tools-daemon/src/main.rs` | GTK main loop + tokio restructure |
| `crates/niri-tools-daemon/src/server.rs` | Handle new commands; channel-based dispatch to GTK thread |
| `crates/niri-tools-daemon/src/state.rs` | Add `mode_configs`, `ui_config` to `DaemonState` |
| `crates/niri-tools/src/main.rs` | Add `Mode` and `ScratchpadPick` CLI subcommands |
| `flake.nix` | Add GTK4/layer-shell to build inputs |

---

## Phase 1: Protocol, Config & CLI (no GTK)

Everything in this phase is pure data model work. No GTK dependency. Tests can run headless.

### Task 1.1: Add IPC commands to protocol

**Files:**
- Modify: `crates/niri-tools-common/src/protocol.rs`

- [ ] **Step 1: Add new Command variants**

Add to the `Command` enum:

```rust
pub enum Command {
    // ... existing variants ...
    ModeShow { mode: Option<String> },
    ModeHide,
    ModeToggle { mode: Option<String> },
    ScratchpadPick,
}
```

- [ ] **Step 2: Add PartialEq derive to Command and Response if not present**

Needed for test assertions. Add `#[derive(PartialEq)]` to both enums.

- [ ] **Step 3: Write round-trip serialization test**

Add to the existing `#[cfg(test)]` module:

```rust
#[test]
fn test_mode_commands_roundtrip() {
    let commands = vec![
        Command::ModeShow { mode: Some("root".to_string()) },
        Command::ModeShow { mode: None },
        Command::ModeHide,
        Command::ModeToggle { mode: Some("brightness".to_string()) },
        Command::ScratchpadPick,
    ];
    for cmd in commands {
        let encoded = encode_message(&cmd).unwrap();
        let decoded: Command = decode_message(&encoded).unwrap();
        assert_eq!(cmd, decoded);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p niri-tools-common`
Expected: All pass, including the new round-trip test.

- [ ] **Step 4: Commit**

```
feat(common): add Mode and ScratchpadPick IPC commands
```

### Task 1.2: Add CLI subcommands

**Files:**
- Modify: `crates/niri-tools/src/main.rs`

- [ ] **Step 1: Add Mode subcommand to clap**

Add a new variant to `Commands`:

```rust
#[derive(Subcommand)]
enum Commands {
    // ... existing ...
    Mode { #[command(subcommand)] command: ModeCommand },
}

#[derive(Subcommand)]
enum ModeCommand {
    Show { name: Option<String> },
    Hide,
    Toggle { name: Option<String> },
}
```

- [ ] **Step 2: Add `pick` to ScratchpadCommand**

```rust
#[derive(Subcommand)]
enum ScratchpadCommand {
    // ... existing ...
    Pick,
}
```

- [ ] **Step 3: Update `build_command` mapping**

Add match arms:

```rust
Commands::Mode { command } => Some(match command {
    ModeCommand::Show { name } => Command::ModeShow { mode: name },
    ModeCommand::Hide => Command::ModeHide,
    ModeCommand::Toggle { name } => Command::ModeToggle { mode: name },
}),
// In ScratchpadCommand match:
ScratchpadCommand::Pick => Some(Command::ScratchpadPick),
```

- [ ] **Step 4: Verify CLI parses correctly**

Run: `cargo run --bin niri-tools -- mode show --help`
Run: `cargo run --bin niri-tools -- scratchpad pick --help`
Expected: Help text displays correctly.

- [ ] **Step 5: Commit**

```
feat(client): add mode and scratchpad pick CLI subcommands
```

### Task 1.3: Split config module into sub-modules

**Files:**
- Create: `crates/niri-tools-common/src/config/mod.rs`
- Create: `crates/niri-tools-common/src/config/scratchpad.rs`
- Create: `crates/niri-tools-common/src/config/settings.rs`
- Create: `crates/niri-tools-common/src/config/mode.rs`
- Create: `crates/niri-tools-common/src/config/ui.rs`
- Remove: `crates/niri-tools-common/src/config.rs`
- Modify: `crates/niri-tools-common/src/lib.rs`

- [ ] **Step 1: Create `config/` directory and move existing types**

Move `ScratchpadConfig`, `SizeConfig`, `PositionConfig`, `OutputOverride` to `config/scratchpad.rs`.
Move `DaemonSettings`, `NotifyLevel` to `config/settings.rs`.
Create `config/mod.rs` that re-exports everything (public API unchanged).

- [ ] **Step 2: Verify all existing code still compiles**

Run: `cargo build`
Expected: Success with no errors.

- [ ] **Step 3: Add `key` and `desc` fields to `ScratchpadConfig`**

In `config/scratchpad.rs`, add the two new fields to the existing struct.
Keep all existing fields intact (`name`, `command`, `app_id`, `title`,
`auto_match`, `size`, `position`, `output_overrides`). Just add:

```rust
pub key: Option<String>,              // NEW: shortcut key in picker
pub desc: Option<String>,             // NEW: display name in picker
```

Initialize both to `None` in all existing construction sites.

- [ ] **Step 4: Create `config/mode.rs` with mode config types**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeConfig {
    pub name: String,
    pub keep_open: bool,
    pub binds: Vec<BindConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindConfig {
    pub key: String,
    pub description: String,
    pub options: Vec<BindOption>,
    pub action: BindAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BindOption {
    KeepOpen,
    Close,
    Alias(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BindAction {
    SpawnSh(String),
    Spawn(Vec<String>),
    SwitchMode(String),
    ScratchpadPick,
    ScratchpadToggle(Option<String>),
    ScratchpadHide,
    ScratchpadFloat(Option<String>),
    ScratchpadTile(Option<String>),
    ScratchpadToggleFloat,
    ScratchpadAdopt,
    ScratchpadDisown,
    /// Pass-through niri action: name + args
    NiriAction { name: String, args: Vec<String> },
}
```

- [ ] **Step 5: Create `config/ui.rs` with UI config types**

```rust
#[derive(Debug, Clone, Default)]
pub struct UiConfig {
    pub font: Option<String>,
    pub background_color: Option<String>,
    pub color: Option<String>,
    pub corner_radius: Option<f64>,
    pub modes: ModesUiConfig,
    pub scratchpads: ScratchpadsUiConfig,
}

#[derive(Debug, Clone, Default)]
pub struct ModesUiConfig {
    pub font: Option<String>,
    pub background_color: Option<String>,
    pub color: Option<String>,
    pub corner_radius: Option<f64>,
    pub anchor: Option<String>,
    pub separator: Option<String>,
    pub margin_top: Option<i32>,
    pub margin_right: Option<i32>,
    pub margin_bottom: Option<i32>,
    pub margin_left: Option<i32>,
    pub padding: Option<f64>,
    pub column_padding: Option<f64>,
    pub min_width: Option<f64>,
    pub border_width: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct ScratchpadsUiConfig {
    pub font: Option<String>,
    pub background_color: Option<String>,
    pub color: Option<String>,
    pub corner_radius: Option<f64>,
    pub anchor: Option<String>,
    pub padding: Option<f64>,
}
```

- [ ] **Step 6: Update `config/mod.rs` to re-export everything**

```rust
pub mod mode;
pub mod scratchpad;
pub mod settings;
pub mod ui;

pub use mode::*;
pub use scratchpad::*;
pub use settings::*;
pub use ui::*;
```

- [ ] **Step 7: Build and run existing tests**

Run: `cargo test`
Expected: All existing tests pass. No public API changes to existing types.

- [ ] **Step 8: Commit**

```
refactor(common): split config module into sub-modules, add mode/ui types
```

### Task 1.4: Parse mode and UI config from KDL

**Files:**
- Modify: `crates/niri-tools-common/src/config_parser.rs`

- [ ] **Step 1: Add `modes` and `ui_config` to `LoadedConfig`**

```rust
pub struct LoadedConfig {
    pub settings: DaemonSettings,
    pub scratchpads: HashMap<String, ScratchpadConfig>,
    pub modes: HashMap<String, ModeConfig>,         // NEW
    pub ui_config: UiConfig,                         // NEW
    pub config_files: Vec<PathBuf>,
    pub warnings: Vec<String>,
}
```

- [ ] **Step 2: Create a `load_from_str` test helper if not already available**

The existing tests use a helper that writes KDL to a tempfile and calls
`load_config`. Check the existing test module for this pattern and reuse
it. If it's private to another test module, create a shared one or
duplicate it. The helper should look like:

```rust
fn load_from_str(kdl: &str) -> LoadedConfig {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.kdl");
    std::fs::write(&path, kdl).unwrap();
    load_config(Some(&path)).unwrap()
}
```

- [ ] **Step 3: Write failing test for `key`/`desc` on scratchpads**

```rust
#[test]
fn test_parse_scratchpad_with_key_and_desc() {
    let config = load_from_str(r#"
        scratchpad "term" {
            key "t"
            desc "Terminal"
            app-id "com.mitchellh.ghostty"
            command "ghostty"
        }
    "#);
    let sp = &config.scratchpads["term"];
    assert_eq!(sp.key.as_deref(), Some("t"));
    assert_eq!(sp.desc.as_deref(), Some("Terminal"));
}
```

- [ ] **Step 4: Run test, verify it fails**

Run: `cargo test -p niri-tools-common test_parse_scratchpad_with_key_and_desc`
Expected: FAIL (key/desc not parsed yet).

- [ ] **Step 4: Implement `key`/`desc` parsing in `parse_scratchpad`**

In the scratchpad parser, add handling for `"key"` and `"desc"` child nodes:

```rust
"key" => {
    if let Some(val) = child.entries().first().and_then(|e| e.value().as_string()) {
        config.key = Some(val.to_string());
    }
}
"desc" => {
    if let Some(val) = child.entries().first().and_then(|e| e.value().as_string()) {
        config.desc = Some(val.to_string());
    }
}
```

- [ ] **Step 5: Run test, verify it passes**

Run: `cargo test -p niri-tools-common test_parse_scratchpad_with_key_and_desc`
Expected: PASS.

- [ ] **Step 7: Write failing test for `notifications` top-level node**

```rust
#[test]
fn test_parse_notifications() {
    let config = load_from_str(r#"notifications "warning""#);
    assert!(matches!(config.settings.notify_level, NotifyLevel::Warning));
}
```

- [ ] **Step 8: Implement `notifications` parsing in `parse_document`**

Add a new match arm. Keep the existing `"settings"` arm for backward
compatibility -- both `notifications "all"` and `settings { notify "all" }`
should work. The new `notifications` node takes precedence if both are
present.

```rust
"notifications" => {
    if let Some(level_str) = node.entries().first().and_then(|e| e.value().as_string()) {
        config.settings.notify_level = match level_str {
            "none" => NotifyLevel::None,
            "error" => NotifyLevel::Error,
            "warning" => NotifyLevel::Warning,
            "all" => NotifyLevel::All,
            other => {
                config.warnings.push(format!("Unknown notification level: {other}"));
                NotifyLevel::All
            }
        };
    }
}
```

- [ ] **Step 8: Run test, verify it passes**

- [ ] **Step 10: Write failing test for `ui` config parsing**

```rust
#[test]
fn test_parse_ui_config() {
    let kdl = r#"
        ui {
            font "Mono 12"
            background-color "#282828"
            color "#fbf1c7"
            corner-radius 4
            modes {
                anchor "bottom"
                separator "  "
                margin-bottom -33
                padding 4
                column-padding 50
                min-width 1000
            }
            scratchpads {
                anchor "center"
                padding 12
            }
        }
    "#;
    let config = load_from_str(kdl);
    assert_eq!(config.ui_config.font.as_deref(), Some("Mono 12"));
    assert_eq!(config.ui_config.modes.anchor.as_deref(), Some("bottom"));
    assert_eq!(config.ui_config.modes.separator.as_deref(), Some("  "));
    assert_eq!(config.ui_config.modes.margin_bottom, Some(-33));
    assert_eq!(config.ui_config.scratchpads.anchor.as_deref(), Some("center"));
    assert_eq!(config.ui_config.scratchpads.padding, Some(12.0));
}
```

- [ ] **Step 10: Implement `parse_ui` function**

Add `"ui"` arm to `parse_document` and implement `parse_ui`, `parse_modes_ui`, `parse_scratchpads_ui` helper functions. Each reads KDL child nodes by name and extracts string/number values.

- [ ] **Step 11: Run test, verify it passes**

- [ ] **Step 13: Write failing test for `mode` config parsing**

```rust
#[test]
fn test_parse_mode_config() {
    let kdl = r#"
        mode "root" {
            binds {
                Space "Launcher" { spawn-sh "rofi -show drun"; }
                o "Open" { switch-mode "open"; }
                b "Brightness" { switch-mode "brightness"; }
            }
        }
        mode "brightness" {
            keep-open
            binds {
                j "-5" { keep-open; spawn-sh "brightness -5"; }
                k "+5" { spawn-sh "brightness +5"; }
                "?" "Query" { alias "q"; spawn-sh "brightness -q"; }
            }
        }
    "#;
    let config = load_from_str(kdl);

    // Root mode
    let root = &config.modes["root"];
    assert_eq!(root.binds.len(), 3);
    assert_eq!(root.binds[0].key, "Space");
    assert_eq!(root.binds[0].description, "Launcher");
    assert!(matches!(root.binds[0].action, BindAction::SpawnSh(ref s) if s == "rofi -show drun"));
    assert!(matches!(root.binds[1].action, BindAction::SwitchMode(ref s) if s == "open"));
    assert!(!root.keep_open);

    // Brightness mode
    let bright = &config.modes["brightness"];
    assert!(bright.keep_open);
    assert_eq!(bright.binds.len(), 3);
    assert_eq!(bright.binds[0].key, "j");
    assert!(bright.binds[0].options.contains(&BindOption::KeepOpen));
    assert_eq!(bright.binds[2].key, "?");
    assert!(bright.binds[2].options.iter().any(|o| matches!(o, BindOption::Alias(s) if s == "q")));
}
```

- [ ] **Step 13: Implement `parse_mode` function**

Add `"mode"` arm to `parse_document`. Implement:
- `parse_mode(node, config)` -- reads mode name (arg 0), checks for `keep-open` flag node, finds `binds` child
- `parse_binds(binds_node)` -> `Vec<BindConfig>` -- iterates children, each is a bind: node name = key, first arg = description, children = options + action
- `parse_bind_children(node)` -> `(Vec<BindOption>, BindAction)` -- dispatches action node name to `BindAction` variants

Action dispatch in `parse_bind_children`:
```
"spawn-sh"             -> BindAction::SpawnSh(first arg)
"spawn"                -> BindAction::Spawn(all args)
"switch-mode"          -> BindAction::SwitchMode(first arg)
"scratchpad-pick"      -> BindAction::ScratchpadPick
"scratchpad-toggle"    -> BindAction::ScratchpadToggle(optional first arg)
"scratchpad-hide"      -> BindAction::ScratchpadHide
"scratchpad-float"     -> BindAction::ScratchpadFloat(optional first arg)
"scratchpad-tile"      -> BindAction::ScratchpadTile(optional first arg)
"scratchpad-toggle-float" -> BindAction::ScratchpadToggleFloat
"scratchpad-adopt"     -> BindAction::ScratchpadAdopt
"scratchpad-disown"    -> BindAction::ScratchpadDisown
"keep-open"            -> option, not action
"close"                -> option, not action
"alias"                -> option, not action
_                      -> BindAction::NiriAction { name, args }
```

- [ ] **Step 14: Run tests, verify they pass**

Run: `cargo test -p niri-tools-common`
Expected: All pass.

- [ ] **Step 16: Write test for niri action pass-through**

```rust
#[test]
fn test_parse_niri_action_passthrough() {
    let kdl = r#"
        mode "resize" {
            binds {
                "5" "50%" { set-window-width "50%"; }
                e "Expand" { expand-column-to-available-width; }
            }
        }
    "#;
    let config = load_from_str(kdl);
    let resize = &config.modes["resize"];
    assert!(matches!(&resize.binds[0].action,
        BindAction::NiriAction { name, args } if name == "set-window-width" && args == &["50%"]));
    assert!(matches!(&resize.binds[1].action,
        BindAction::NiriAction { name, args } if name == "expand-column-to-available-width" && args.is_empty()));
}
```

- [ ] **Step 16: Run test, verify it passes (should pass from step 13 implementation)**

- [ ] **Step 18: Write validation test for duplicate mode names**

```rust
#[test]
fn test_duplicate_mode_name_overrides() {
    let kdl = r#"
        mode "root" { binds { a "A" { spawn-sh "echo a"; } } }
        mode "root" { binds { b "B" { spawn-sh "echo b"; } } }
    "#;
    let config = load_from_str(kdl);
    // Second definition overrides first (same behavior as scratchpads)
    assert_eq!(config.modes["root"].binds[0].key, "b");
}
```

- [ ] **Step 18: Run all tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 19: Commit**

```
feat(common): parse mode, ui, and notifications config from KDL
```

### Task 1.5: Add mode/ui config to DaemonState and server dispatch

**Files:**
- Modify: `crates/niri-tools-daemon/src/state.rs`
- Modify: `crates/niri-tools-daemon/src/server.rs`

- [ ] **Step 1: Add mode and UI config to `DaemonState`**

```rust
pub struct DaemonState {
    // ... existing fields ...
    pub mode_configs: HashMap<String, ModeConfig>,
    pub ui_config: UiConfig,
}
```

- [ ] **Step 2: Update `reload_config` in `server.rs` to load modes and ui**

In the existing `reload_config` method, after loading scratchpad configs:

```rust
self.state.mode_configs = loaded.modes;
self.state.ui_config = loaded.ui_config;
```

- [ ] **Step 3: Add placeholder dispatch for new commands**

In `dispatch_command`:

```rust
Command::ModeShow { mode } => {
    tracing::info!("Mode show: {:?}", mode);
    // TODO: dispatch to UI manager in Phase 2
    Response::Ok
}
Command::ModeHide => {
    tracing::info!("Mode hide");
    Response::Ok
}
Command::ModeToggle { mode } => {
    tracing::info!("Mode toggle: {:?}", mode);
    Response::Ok
}
Command::ScratchpadPick => {
    tracing::info!("Scratchpad pick");
    Response::Ok
}
```

- [ ] **Step 4: Verify it compiles and existing tests pass**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 5: Commit**

```
feat(daemon): wire mode/ui config into state and add placeholder command dispatch
```

---

## Phase 2: GTK Foundation & Mode Overlay

This phase introduces the GTK4 dependency and restructures the daemon's main loop. This is the riskiest phase -- validate with a minimal proof-of-concept before building the full UI.

### Task 2.1: Add GTK4 dependencies and restructure daemon entry point

**Files:**
- Modify: `crates/niri-tools-daemon/Cargo.toml`
- Modify: `crates/niri-tools-daemon/src/main.rs`
- Modify: `flake.nix` (add GTK4 build inputs)

- [ ] **Step 1: Add dependencies to daemon Cargo.toml**

```toml
gtk4 = "0.9"
gtk4-layer-shell = "0.4"
gdk4-wayland = "0.9"
```

Check crates.io for latest compatible versions. The `gtk4` crate version must match `gtk4-layer-shell` and `gdk4-wayland`.

- [ ] **Step 2: Update `flake.nix` build inputs**

Add `gtk4`, `gtk4-layer-shell`, `gdk4-wayland` (or their pkg-config equivalents) to the nix build inputs so CI and nix builds work.

- [ ] **Step 3: Restructure `main.rs` -- GTK main thread, tokio background**

This is the most significant refactor. The current `#[tokio::main]` and
`DaemonServer.start()` must be split: GTK owns the main thread, tokio
runs on a background runtime.

**Sub-steps:**

a) Create the `OnceLock<Runtime>` pattern:
```rust
use std::sync::OnceLock;
use tokio::runtime::Runtime;

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("Failed to create tokio runtime"))
}
```

b) Change `main()` from `#[tokio::main] async fn main()` to plain
`fn main()`. Create a `gtk4::Application`, call `app.run_with_args(&[])`.

c) In `connect_activate`: call `app.hold()`, then spawn the daemon's
tokio event loop on the background runtime via `runtime().spawn(...)`.

d) **Refactor `DaemonServer`**: the existing `run_loop` reads commands
synchronously from the socket. It needs a new code path where UI-related
commands (`ModeShow`, `ModeHide`, `ModeToggle`, `ScratchpadPick`) are
forwarded to the GTK thread via a `glib::Sender` channel, while
non-UI commands (`Toggle`, `Hide`, `DaemonStop`, etc.) are handled
directly on the tokio thread as before.

Create a `glib::MainContext::channel()` pair. The tokio side sends
UI commands through it. The GTK side receives and dispatches them to the
`UiManager`.

e) Ensure scratchpad functionality continues working: the niri event
stream, socket listener, and scratchpad operations all run on tokio
exactly as before. Only UI commands are forwarded to GTK.

**Reference:** whisper-overlay at `/home/boo/proj/whisper-overlay/worktree/main/src/app.rs` and `src/main.rs` for the working GTK4+tokio pattern (especially `glib::spawn_future_local()` for the bridge).

**Testing:** After this refactor, all existing `cargo test` should still pass (tests don't use GTK). Manual testing: start the daemon, run scratchpad commands, verify they still work.

- [ ] **Step 4: Verify it builds (may not fully work without Wayland display)**

Run: `cargo build -p niri-tools-daemon`
Expected: Compiles successfully.

- [ ] **Step 5: Commit**

```
feat(daemon): restructure to GTK4 main loop with tokio background thread
```

### Task 2.2: Create minimal layer-shell mode overlay window

**Files:**
- Create: `crates/niri-tools-daemon/src/ui/mod.rs`
- Create: `crates/niri-tools-daemon/src/ui/mode_overlay.rs`
- Modify: `crates/niri-tools-daemon/src/main.rs` (wire up)

- [ ] **Step 1: Create `ui/mod.rs` with `UiManager` struct**

The `UiManager` owns both GTK windows and provides show/hide methods callable from the GTK main thread.

- [ ] **Step 2: Create `ui/mode_overlay.rs` with layer-shell window**

Create an `ApplicationWindow` with:
- `init_layer_shell()`
- `set_layer(Layer::Overlay)`
- `set_keyboard_mode(KeyboardMode::Exclusive)`
- `set_namespace(Some("niri-tools-mode"))`
- Anchor and margins from `ModesUiConfig`
- Start hidden (do NOT call `present()` during init)

Reference: whisper-overlay `src/app.rs:508-528` for the init sequence.

- [ ] **Step 3: Wire `UiManager` into `main.rs` activate handler**

Create the `UiManager` in the `connect_activate` callback. The mode overlay window is created but hidden.

- [ ] **Step 4: Add a temporary test: show the overlay via CLI**

Wire the `ModeShow` command to call `window.present()` on the GTK thread. The overlay should appear as an empty window. Test manually:

```bash
cargo run --bin niri-tools-daemon &
cargo run --bin niri-tools -- mode show
```

Expected: An empty overlay window appears on the focused output.

- [ ] **Step 5: Commit**

```
feat(daemon): create minimal layer-shell mode overlay window
```

### Task 2.3: Build mode overlay widget tree and CSS

**Files:**
- Modify: `crates/niri-tools-daemon/src/ui/mode_overlay.rs`
- Create: `crates/niri-tools-daemon/src/ui/css.rs`

- [ ] **Step 1: Implement CSS generation from `UiConfig`**

Generate CSS string from resolved config values:

```css
window { background-color: transparent; }
.mode-container { background-color: #2F2A4C; border-radius: 2px; padding: 4px; }
.mode-key { font-family: "Pragmasevka Nerd Font"; font-size: 12pt; color: #DFD9FB; }
.mode-sep { color: #DFD9FB; }
.mode-desc { color: #DFD9FB; }
.mode-desc-mode { color: #8ec07c; }  /* accent for switch-mode entries */
.state-visible { color: #8ec07c; }
.state-floating { color: #8ec07c; }
.state-unspawned { opacity: 0.5; }
```

- [ ] **Step 2: Build widget tree for a mode**

```
ApplicationWindow
  Box.mode-container (horizontal)
    Box (column, vertical) -- for each bind:
      Box (horizontal)
        Label.mode-key "key"
        Label.mode-sep "separator"
        Label.mode-desc "description"
```

Create a `rebuild_mode(&self, mode: &ModeConfig)` method that clears the container and rebuilds labels from the mode's binds.

- [ ] **Step 3: Test manually**

With a config that has a `mode "root"`, run the daemon and `niri-tools mode show`. The overlay should display the key hints.

- [ ] **Step 4: Commit**

```
feat(daemon): render mode overlay with key hints from config
```

### Task 2.4: Keyboard handling and action dispatch

**Files:**
- Create: `crates/niri-tools-daemon/src/mode.rs`
- Modify: `crates/niri-tools-daemon/src/ui/mode_overlay.rs`

- [ ] **Step 1: Create `mode.rs` with `ModeState`**

```rust
pub struct ModeState {
    mode_stack: Vec<String>,
    modes: HashMap<String, ModeConfig>,
}

impl ModeState {
    pub fn new(modes: HashMap<String, ModeConfig>) -> Self
    pub fn current_mode(&self) -> Option<&ModeConfig>
    pub fn push_mode(&mut self, name: &str) -> bool
    pub fn pop_mode(&mut self) -> bool  // returns false if stack is empty
    pub fn clear(&mut self)
    pub fn lookup_bind(&self, key: &str) -> Option<&BindConfig>
    // Strip Super/Mod from the key name before lookup
}
```

- [ ] **Step 2: Write tests for `ModeState`**

Test push/pop/clear, bind lookup, and Super-stripping behavior.

- [ ] **Step 3: Attach `EventControllerKey` to the overlay window**

```rust
let key_controller = EventControllerKey::new();
key_controller.connect_key_pressed(move |_, keyval, keycode, modifiers| {
    // 1. Convert keyval + modifiers to key string
    // 2. Strip Super/Mod from modifiers
    // 3. Look up bind in current mode
    // 4. If found: execute action
    // 5. If Escape, Ctrl+[, or Ctrl+g: hide
    // 6. If Backspace: pop mode
    Propagation::Stop
});
window.add_controller(key_controller);
```

- [ ] **Step 4: Implement action execution**

For each `BindAction` variant:
- `SpawnSh(cmd)` -- `std::process::Command::new("sh").arg("-c").arg(cmd)` with `pre_exec(|| { libc::daemon(1, 0); Ok(()) })`
- `Spawn(args)` -- `std::process::Command::new(&args[0]).args(&args[1..])` with daemon
- `SwitchMode(name)` -- push mode stack, rebuild overlay
- `NiriAction { name, args }` -- dispatch to niri client via tokio: `runtime().spawn(niri.run_action(name, args))`
- `ScratchpadToggle(name)` etc. -- dispatch to daemon via channel
- `ScratchpadPick` -- show scratchpad picker (Phase 3)

- [ ] **Step 5: Implement key-release-before-hide**

On a closing action:
1. Execute the command
2. Hide all child widgets (or set opacity to 0)
3. Record the keycode in `exit_on_key_release`
4. In `connect_key_released`, if keycode matches, hide the window

Reference: wlr-which-key `src/main.rs:389-433`.

- [ ] **Step 6: Implement mode-level `keep-open` and per-bind `close`**

After executing an action:
- If `BindOption::Close` is present, OR if mode `keep_open` is false and `BindOption::KeepOpen` is NOT present: begin close sequence (key-release-before-hide)
- Otherwise: stay in current mode

- [ ] **Step 7: Test manually with full config**

Create a config with root mode, brightness mode (keep-open), and resize mode (niri actions). Test:
1. Mode show → overlay appears
2. Press key → action fires, overlay closes
3. Brightness mode → keep-open works, Escape closes
4. Mode switching → mode stack navigation, Backspace goes back

- [ ] **Step 8: Commit**

```
feat(daemon): implement keyboard handling and action dispatch for mode overlay
```

---

## Phase 3: Scratchpad Picker

### Task 3.1: Add frizbee dependency and create picker window

**Files:**
- Modify: `crates/niri-tools-daemon/Cargo.toml`
- Create: `crates/niri-tools-daemon/src/ui/scratchpad_picker.rs`
- Modify: `crates/niri-tools-daemon/src/ui/mod.rs`

- [ ] **Step 1: Add `frizbee` to dependencies**

```toml
frizbee = "0.2"  # check crates.io for latest
```

- [ ] **Step 2: Create scratchpad picker layer-shell window**

Similar to mode overlay but:
- Anchor: center (from `ui.scratchpads.anchor`)
- Different namespace: `niri-tools-scratchpad-picker`
- Widget tree: vertical list + search entry

- [ ] **Step 3: Build the list widget from scratchpad configs**

Each row:
```
Box (horizontal)
  Label.picker-key "[t]"     // or "[ ]" if no key
  Label.picker-name "Terminal"
  Label.picker-state "●"     // or empty
```

Populate from `DaemonState.scratchpad_configs` + `DaemonState.scratchpads` (for state).

- [ ] **Step 4: Commit**

```
feat(daemon): create scratchpad picker layer-shell window
```

### Task 3.2: Implement fuzzy search and shortcut dispatch

**Files:**
- Modify: `crates/niri-tools-daemon/src/ui/scratchpad_picker.rs`

- [ ] **Step 1: Add search entry and fuzzy filtering**

Attach an `EventControllerKey` to the picker window. On key press:
1. Check if `Mod` is held and key matches a scratchpad shortcut → toggle immediately
2. Otherwise, append to search buffer, filter list with `frizbee::match_list()`

- [ ] **Step 2: Implement Enter to toggle selected, Escape to dismiss**

- [ ] **Step 3: Implement live state CSS classes**

Query `DaemonState` scratchpad states and apply `.state-visible`, `.state-floating`, `.state-unspawned` CSS classes to list rows.

- [ ] **Step 4: Wire `ScratchpadPick` command to show the picker**

- [ ] **Step 5: Test manually**

- [ ] **Step 6: Commit**

```
feat(daemon): implement fuzzy search and shortcut dispatch for scratchpad picker
```

---

## Phase 4: Style Inheritance

### Task 4.1: Parse niri config for style properties

**Files:**
- New function in `crates/niri-tools-common/src/config_parser.rs` or a new file

- [ ] **Step 1: Read `~/.config/niri/config.kdl` and extract**

- `layout.border.active-color` → accent color
- `layout.border.width` → border width

- [ ] **Step 2: Implement layered resolution**

`ui.modes > ui (global) > niri config > built-in defaults`

- [ ] **Step 3: Commit**

```
feat(common): parse niri config for style inheritance
```

---

## Phase 5: Polish

### Task 5.1: Config hot-reload for modes and UI

- [ ] Extend existing file watcher to reload mode/UI config
- [ ] Rebuild overlay widget tree on config change
- [ ] Regenerate CSS on config change
- [ ] Commit

### Task 5.2: Validation

- [ ] Warn on invalid mode references in `switch-mode`
- [ ] Warn on duplicate keys within a mode's binds
- [ ] Warn on scratchpad `key` conflicts
- [ ] Commit

### Task 5.3: Multi-monitor support

- [ ] Set layer-shell output to focused output before `present()`
- [ ] The daemon already tracks `focused_output` from niri events
- [ ] Commit

### Task 5.4: Comprehensive tests

- [ ] Config parsing edge cases (empty modes, missing binds block, unknown actions)
- [ ] `ModeState` navigation tests (push/pop/clear/lookup)
- [ ] Round-trip serialization for all new Command variants
- [ ] Commit
