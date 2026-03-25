# Smart Focus Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `smart-focus` command that focuses a window by ID, with scratchpad-aware behavior (showing scratchpads, hiding focused scratchpads, focusing workspaces).

**Architecture:** New top-level CLI subcommand `smart-focus --id <u64>` that sends a `SmartFocus { id }` command over IPC. The daemon dispatches to `ScratchpadManager::smart_focus()`, which inspects window and scratchpad state to decide the correct action.

**Tech Stack:** Rust, clap (CLI), bincode (IPC), tokio (async daemon)

---

### Task 1: Add `SmartFocus` variant to the IPC protocol

**Files:**
- Modify: `crates/niri-tools-common/src/protocol.rs`

**Step 1: Add the Command variant**

Add `SmartFocus { id: u64 }` to the `Command` enum in `protocol.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Command {
    Toggle { name: Option<String> },
    Hide,
    ToggleFloat { name: Option<String> },
    Float { name: Option<String> },
    Tile { name: Option<String> },
    SmartFocus { id: u64 },
    DaemonStop,
    DaemonRestart,
    DaemonStatus,
}
```

**Step 2: Add a serialization round-trip test**

Add a test alongside the existing protocol tests (search for `#[cfg(test)]` in `protocol.rs`). Follow the pattern of the existing round-trip tests:

```rust
#[test]
fn smart_focus_command_roundtrip() {
    let cmd = Command::SmartFocus { id: 42 };
    let encoded = encode_message(&cmd).unwrap();
    let decoded: Command = decode_message(&encoded[4..]).unwrap();
    assert_eq!(decoded, cmd);
}
```

**Step 3: Run tests to verify**

Run: `cargo test -p niri-tools-common`
Expected: All tests pass, including the new round-trip test.

**Step 4: Commit**

```
feat(common): add SmartFocus command variant to IPC protocol
```

---

### Task 2: Add `smart-focus` CLI subcommand

**Files:**
- Modify: `crates/niri-tools/src/main.rs`

**Step 1: Add SmartFocus to the Commands enum**

Add a new top-level variant to `Commands` (alongside `Daemon` and `Scratchpad`):

```rust
#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    /// Manage the daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Manage scratchpad windows
    Scratchpad {
        #[command(subcommand)]
        command: ScratchpadCommand,
    },
    /// Focus a window by ID with scratchpad-aware behavior
    SmartFocus {
        /// Window ID to focus
        #[arg(long)]
        id: u64,
    },
}
```

**Step 2: Add SmartFocus arm to build_command()**

```rust
Commands::SmartFocus { id } => Some(Command::SmartFocus { id: *id }),
```

**Step 3: SmartFocus should auto-start the daemon**

In `requires_running_daemon()` (or the equivalent logic in `main()`), ensure `SmartFocus` is treated as an operational command (not a daemon management command). Check the existing `is_daemon_management_command()` function — `SmartFocus` should NOT be considered daemon management.

The existing code at `crates/niri-tools/src/main.rs` uses a function called `is_daemon_management_command()` — read it to understand how it works, then ensure `SmartFocus` falls through correctly (it should, since it only checks for `DaemonCommand::Stop/Status`).

**Step 4: Add CLI parsing tests**

Follow the existing test pattern:

```rust
#[test]
fn parse_smart_focus() {
    let cli = Cli::try_parse_from(["niri-tools", "smart-focus", "--id", "42"]).unwrap();
    assert_eq!(cli.command, Commands::SmartFocus { id: 42 });
}

#[test]
fn parse_smart_focus_missing_id_is_error() {
    assert!(Cli::try_parse_from(["niri-tools", "smart-focus"]).is_err());
}

#[test]
fn build_command_smart_focus() {
    let cli = Cli::try_parse_from(["niri-tools", "smart-focus", "--id", "99"]).unwrap();
    assert_eq!(build_command(&cli), Some(Command::SmartFocus { id: 99 }));
}
```

**Step 5: Run tests**

Run: `cargo test -p niri-tools`
Expected: All tests pass.

**Step 6: Commit**

```
feat(client): add smart-focus top-level CLI subcommand
```

---

### Task 3: Implement `smart_focus()` in `ScratchpadManager`

**Files:**
- Modify: `crates/niri-tools-daemon/src/scratchpad.rs`

**Step 1: Write the failing tests**

Add these tests in the `mod tests` block of `scratchpad.rs`, following existing patterns using `setup_state`, `make_window`, `MockNiriClient`, etc.:

```rust
// -- smart_focus tests --

#[tokio::test]
async fn smart_focus_already_focused_is_noop() {
    let mut state = setup_state();
    let window = make_window(42, "firefox", true, false, Some(1));
    state.windows.insert(42, window);
    state.focused_window_id = Some(42);

    let niri = MockNiriClient::new();
    {
        let mut mgr = ScratchpadManager::new(&mut state, &niri);
        mgr.smart_focus(42).await.unwrap();
    }

    let actions = niri.get_actions();
    assert!(actions.is_empty());
}

#[tokio::test]
async fn smart_focus_nonexistent_window_returns_error() {
    let mut state = setup_state();
    let niri = MockNiriClient::new();
    {
        let mut mgr = ScratchpadManager::new(&mut state, &niri);
        let result = mgr.smart_focus(999).await;
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn smart_focus_scratchpad_window_shows_it() {
    let mut state = setup_state_with_config("term");

    // Scratchpad window on scratchpad workspace (hidden)
    let window = make_window(42, "ghostty", false, true, Some(2));
    state.windows.insert(42, window);
    state.register_scratchpad_window("term", 42);
    state.mark_scratchpad_hidden("term");
    state.focused_window_id = Some(99);

    let niri = MockNiriClient::new();
    {
        let mut mgr = ScratchpadManager::new(&mut state, &niri);
        mgr.smart_focus(42).await.unwrap();
    }

    let actions = niri.get_actions();
    assert!(actions.iter().any(|(a, _)| a == "move-window-to-floating"));
    assert!(actions.iter().any(|(a, _)| a == "move-window-to-monitor"));
    assert!(actions.iter().any(|(a, args)| {
        a == "focus-window" && args.contains(&"42".to_string())
    }));
    assert!(state.scratchpads.get("term").unwrap().visible);
}

#[tokio::test]
async fn smart_focus_regular_window_focuses_it() {
    let mut state = setup_state();
    let target = make_window(42, "firefox", false, false, Some(1));
    state.windows.insert(42, target);
    state.focused_window_id = Some(99);

    let niri = MockNiriClient::new();
    {
        let mut mgr = ScratchpadManager::new(&mut state, &niri);
        mgr.smart_focus(42).await.unwrap();
    }

    let actions = niri.get_actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].0, "focus-window");
    assert!(actions[0].1.contains(&"42".to_string()));
}

#[tokio::test]
async fn smart_focus_regular_window_hides_focused_scratchpad() {
    let mut state = setup_state_with_config("term");

    // Focused scratchpad (floating)
    let sp_window = make_window(10, "ghostty", true, true, Some(1));
    state.windows.insert(10, sp_window);
    state.register_scratchpad_window("term", 10);
    state.mark_scratchpad_visible("term");
    state.focused_window_id = Some(10);

    // Target regular window on a different workspace
    let target = make_window(42, "firefox", false, false, Some(3));
    state.windows.insert(42, target);
    state.workspaces.insert(3, make_workspace(3, "eDP-1", false, Some("other")));

    let niri = MockNiriClient::new();
    {
        let mut mgr = ScratchpadManager::new(&mut state, &niri);
        mgr.smart_focus(42).await.unwrap();
    }

    let actions = niri.get_actions();
    // Should hide the scratchpad first
    assert!(actions.iter().any(|(a, args)| {
        a == "move-window-to-workspace"
            && args.contains(&SCRATCHPAD_WORKSPACE.to_string())
    }));
    // Then focus the target
    assert!(actions.iter().any(|(a, args)| {
        a == "focus-window" && args.contains(&"42".to_string())
    }));
    assert!(!state.scratchpads.get("term").unwrap().visible);
}
```

**Step 2: Run tests to confirm they fail**

Run: `cargo test -p niri-tools-daemon smart_focus`
Expected: Compile error — `smart_focus` method doesn't exist yet.

**Step 3: Implement `smart_focus()`**

Add this method to `impl ScratchpadManager`:

```rust
/// Focus a specific window by ID with scratchpad-aware behavior.
///
/// - If already focused: no-op
/// - If window doesn't exist: return error
/// - If window is a scratchpad: show it (move to current monitor + focus)
/// - If window is regular:
///   - Hide the currently focused scratchpad (if any)
///   - Focus the target window (niri handles workspace switching)
pub async fn smart_focus(&mut self, window_id: u64) -> niri_tools_common::Result<()> {
    // Already focused — nothing to do
    if self.state.focused_window_id == Some(window_id) {
        return Ok(());
    }

    // Check the window exists
    if !self.state.windows.contains_key(&window_id) {
        return Err(niri_tools_common::NiriToolsError::Other(
            format!("Window {window_id} not found"),
        ));
    }

    // Check if target is a scratchpad
    if let Some(name) = self.state.get_scratchpad_for_window(window_id) {
        let name = name.to_string();
        let config = self
            .state
            .scratchpad_configs
            .get(&name)
            .cloned()
            .ok_or_else(|| {
                niri_tools_common::NiriToolsError::Other(format!(
                    "No config for scratchpad '{name}'"
                ))
            })?;
        self.show_scratchpad(&name, window_id, &config).await?;
        return Ok(());
    }

    // Regular window: hide focused scratchpad if any
    if let Some(focused_id) = self.state.focused_window_id {
        if let Some(sp_name) = self.state.get_scratchpad_for_window(focused_id) {
            let sp_name = sp_name.to_string();
            let focused_win = self.state.windows.get(&focused_id).cloned();
            if focused_win.as_ref().is_some_and(|w| w.is_floating) {
                self.hide_scratchpad(&sp_name, focused_id).await?;
            }
        }
    }

    // Focus the target window
    self.focus_window(window_id).await?;

    Ok(())
}
```

**Step 4: Run tests to confirm they pass**

Run: `cargo test -p niri-tools-daemon smart_focus`
Expected: All 5 new tests pass.

**Step 5: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

**Step 6: Commit**

```
feat(daemon): implement smart_focus in ScratchpadManager
```

---

### Task 4: Wire up dispatch in `DaemonServer`

**Files:**
- Modify: `crates/niri-tools-daemon/src/server.rs`

**Step 1: Add SmartFocus dispatch arm**

In `dispatch_command()`, add a new match arm after the existing ones (before the closing brace). Follow the same pattern as other commands:

```rust
Command::SmartFocus { id } => {
    let mut mgr = ScratchpadManager::new(&mut self.state, self.niri.as_ref());
    match mgr.smart_focus(id).await {
        Ok(()) => Response::Ok,
        Err(e) => {
            self.notifier.notify_warning("Smart Focus", &e.to_string());
            Response::Error(e.to_string())
        }
    }
}
```

Note: Unlike other commands, `smart_focus` errors should trigger a warning notification since the user spec says "show a warning notification" when the window doesn't exist.

**Step 2: Add dispatch test**

Follow the pattern from `dispatch_toggle_with_no_config_returns_error`:

```rust
#[tokio::test]
async fn dispatch_smart_focus_nonexistent_window_returns_error() {
    let mut server = make_server();
    let response = server
        .dispatch_command(Command::SmartFocus { id: 99999 })
        .await;
    match response {
        Response::Error(msg) => {
            assert!(msg.contains("99999"));
        }
        other => panic!("Expected Error, got {other:?}"),
    }
}
```

**Step 3: Run tests**

Run: `cargo test -p niri-tools-daemon dispatch`
Expected: All dispatch tests pass.

**Step 4: Run full test suite + clippy**

Run: `cargo test && cargo clippy --all-features`
Expected: All pass with no warnings.

**Step 5: Commit**

```
feat(daemon): wire up SmartFocus command dispatch with warning notification
```

---

### Task 5: Update AGENTS.md

**Files:**
- Modify: `AGENTS.md`

**Step 1: Update the Commands section**

Add `smart-focus` to the current commands list:

```
niri-tools smart-focus --id <window-id>
```

**Step 2: Commit all together**

```
docs: add smart-focus command to AGENTS.md
```
