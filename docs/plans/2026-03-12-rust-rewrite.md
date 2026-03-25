# niri-tools Rust Rewrite

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rewrite niri-tools from Python to Rust for minimal-latency scratchpad toggling, full unit testability via trait-based dependency injection, and KDL configuration.

**Architecture:** Client/daemon over Unix socket. The client is a thin CLI binary that serializes a command via bincode and sends it over a Unix socket. The daemon is a tokio-based async event loop that listens to niri's event stream, manages scratchpad windows, watches config files, and serves client commands. All external interactions (niri CLI, notifications, rofi prompts) are abstracted behind traits for testability.

**Tech Stack:** Rust 2024, tokio (async runtime), clap (CLI), serde + bincode (IPC), knuffel (KDL config parsing), notify (file watching)

---

## Project Context

### What is niri-tools?

A companion tool for the [niri](https://github.com/YaLTeR/niri) Wayland compositor. It provides a scratchpad system - floating overlay windows that can be toggled on/off with a keybinding. The daemon maintains state about which windows are scratchpads, listens to niri's event stream for window lifecycle events, and executes niri IPC commands to show/hide/position windows.

### Why rewrite in Rust?

**Primary motivation: latency.** The critical path is: user presses keybinding → client binary starts → sends command over socket → daemon processes → niri action executes. In Python, the client startup alone adds ~50-100ms. A compiled Rust binary starts in <1ms.

**Secondary motivations:**
- Type safety and compile-time guarantees
- Full unit testability via trait-based DI (Python version has tight coupling to subprocess calls)
- Binary distribution (no Python runtime dependency)
- Memory safety for a long-running daemon

### What's being ported?

1:1 feature port of the Python version, **excluding the urgency handler** (deferred to later). Specifically:

- Client CLI with subcommands (daemon start/stop/restart/status, scratchpad toggle/hide/float/tile/toggle-float)
- Daemon with Unix socket server, niri event stream listener, config file watcher
- Scratchpad manager (toggle, smart-toggle, show/hide, float/tile)
- State management (in-memory niri state, scratchpad mappings, disk persistence across daemon restarts)
- Config loading from KDL with recursive include support and hot-reload
- Notification system (notify-send / dms fallback)

**Deferred (along with urgency handler):**
- Rofi-dependent features: adopt, disown, menu, close-with-confirmation
- Prompter trait (deferred until rofi features are added)

### What's changing?

| Aspect | Python | Rust |
|--------|--------|------|
| Config format | YAML (`scratchpads.yaml`) | KDL (`scratchpads.kdl`) |
| IPC wire format | JSON over Unix socket | bincode over Unix socket |
| Config location | `~/.config/niri/scratchpads.yaml` | `~/.config/niri/scratchpads.kdl` |
| Async runtime | asyncio | tokio |
| File watching | watchfiles (inotify) | notify crate (inotify/kqueue) |
| Testability | Difficult (subprocess coupling) | Full unit tests via trait mocks |
| Urgency handler | Included | Deferred |
| Per-output config | Position only | Position AND size overrides |

---

## Key Design Decisions

### 1. Trait-based Dependency Injection

All external interactions are abstracted behind async traits:

```rust
#[async_trait]
pub trait NiriClient: Send + Sync {
    async fn run_action(&self, action: &str, args: &[&str]) -> Result<()>;
    async fn get_windows(&self) -> Result<Vec<WindowData>>;
    async fn get_workspaces(&self) -> Result<Vec<WorkspaceData>>;
    async fn get_outputs(&self) -> Result<OutputMap>;
    async fn get_focused_output(&self) -> Result<String>;
    async fn subscribe_events(&self) -> Result<Pin<Box<dyn Stream<Item = Result<NiriEvent>> + Send>>>;
}

#[async_trait]
pub trait Notifier: Send + Sync {
    fn notify_error(&self, title: &str, message: &str);
    fn notify_warning(&self, title: &str, message: &str);
    fn notify_info(&self, title: &str, message: &str);
}

// Prompter trait deferred until rofi-dependent features (adopt/menu/close) are added
```

**Why:** The Python version calls `subprocess.run(["niri", "msg", ...])` directly in business logic, making it impossible to unit test without a running niri compositor. With traits, tests inject mock implementations that return canned data and record calls for assertion. The `ScratchpadManager` and `DaemonState` become fully testable in isolation.

**Note:** The `Prompter` trait for rofi interactions is deferred until adopt/menu/close features are added.

### 2. Bincode over Unix Socket

Commands and responses are serde-serializable enums, transmitted via bincode:

```rust
#[derive(Serialize, Deserialize)]
pub enum Command {
    Toggle { name: Option<String> },
    Hide,
    ToggleFloat { name: Option<String> },
    Float { name: Option<String> },
    Tile { name: Option<String> },
    DaemonStop,
    DaemonRestart,
    DaemonStatus,
    // Deferred: Adopt, Disown, Menu, Close (rofi-dependent)
}

#[derive(Serialize, Deserialize)]
pub enum Response {
    Ok,
    Status { pid: u32, socket: String, /* ... */ },
    Error(String),
}
```

**Wire format:** 4-byte length prefix (u32 LE) followed by bincode-encoded payload. Simple, fast, zero-copy deserialization.

**Why bincode over JSON:** Serialization is ~10x faster and payloads are smaller. For the latency-critical toggle path, every microsecond matters. Bincode also avoids string allocation overhead.

### 3. KDL Configuration

```kdl
settings {
    notify "all"
    watch true
}

include "./scratchpads.private.kdl"

scratchpad "term" {
    app-id "com.mitchellh.ghostty"
    command "ghostty"
    size width="60%" height="60%"
    position x="10%" y="35%"

    output "DP-2" {
        position x="50%" y="35%"
    }
}

scratchpad "dms-settings" {
    app-id "org.quickshell"
    title "^Settings$"
    command "dms" "ipc" "call" "settings" "open"
    size width="40%" height="60%"
    position x="10%" y="35%"

    output "DP-2" {
        position x="50%" y="35%"
    }
}
```

**Improvements over YAML version:**
- Per-output `size` overrides (not just position) via `output` blocks
- Default size/position at top level, per-output overrides in scoped blocks
- KDL's document model maps naturally to this hierarchical config
- `position default="center"` for explicit centering

**Include support:** `include` nodes are processed recursively with cycle detection, same semantics as the Python version (included values are defaults, main file overrides).

### 4. Crate Structure

```
crates/
├── niri-tools-common/     # Shared types, protocol, config, traits
│   └── src/lib.rs
├── niri-tools/            # Thin client binary
│   └── src/main.rs
└── niri-tools-daemon/     # Daemon binary
    └── src/
        ├── main.rs
        ├── server.rs       # Socket server + event loop orchestration
        ├── scratchpad.rs   # Scratchpad manager logic
        ├── state.rs        # In-memory daemon state
        ├── config.rs       # KDL config loading
        ├── niri.rs         # Real NiriClient implementation
        └── notify.rs       # Real Notifier implementation
```

**Common crate exports:** `Command`, `Response`, `ScratchpadConfig`, `DaemonSettings`, `WindowInfo`, `WorkspaceInfo`, `OutputInfo`, `NiriEvent`, socket path constants, the trait definitions (`NiriClient`, `Notifier`), and shared utility types.

---

## Key Indicators of Success

### Functional Parity
- [ ] Core scratchpad subcommands work identically to the Python version: toggle, smart-toggle, hide, float, tile, toggle-float
- [ ] Daemon lifecycle commands work: start, stop, restart, status
- [ ] Deferred features (not in scope): adopt, disown, menu, close (rofi-dependent)
- [ ] Client auto-starts daemon when not running (spawns via `niri msg action spawn`)
- [ ] Config hot-reload works (file watcher detects changes, reloads KDL)
- [ ] Config includes work recursively with cycle detection
- [ ] State persists across daemon restarts (scratchpad-to-window mappings)
- [ ] State reconciliation on startup (removes mappings for windows that no longer exist)
- [ ] Per-output position AND size overrides work
- [ ] Regex matching for app_id and title works (patterns starting with `/` or `^`)

### Latency
- [ ] Client binary startup is <5ms (vs ~50-100ms for Python)
- [ ] Full toggle round-trip (client start → daemon action → niri executes) is <10ms excluding niri's own processing time
- [ ] Bincode serialization/deserialization of Command/Response is <1μs

### Testability
- [ ] ScratchpadManager has unit tests for all operations using mock NiriClient
- [ ] DaemonState has unit tests for state management, persistence, reconciliation
- [ ] Config loader has unit tests for KDL parsing, includes, error handling
- [ ] Client has unit tests for CLI argument parsing and command construction
- [ ] Protocol (Command/Response) has round-trip serialization tests
- [ ] No test requires a running niri compositor or real Unix socket
- [ ] `cargo test` passes in CI without any special environment

### Code Quality
- [ ] No `unwrap()` in library/daemon code (proper error handling with `thiserror` or `anyhow`)
- [ ] All public types and traits are documented
- [ ] Clippy passes with no warnings
- [ ] `cargo build` produces two binaries: `niri-tools` and `niri-tools-daemon`
- [ ] Nix flake builds and produces both binaries

---

## Dependency Budget

| Crate | Purpose | Used in |
|-------|---------|---------|
| `tokio` | Async runtime (rt-multi-thread, net, process, fs, signal) | daemon |
| `clap` (derive) | CLI argument parsing | client, daemon |
| `serde` + `serde_json` | Serialization (json for niri IPC parsing) | common, daemon |
| `bincode` | Binary serialization for client↔daemon IPC | common |
| `kdl` | KDL document parsing | common or daemon |
| `notify` | Filesystem watching (inotify) | daemon |
| `regex` | App ID / title matching | daemon |
| `async-trait` | Async trait support | common |
| `thiserror` | Error type derivation | common |
| `anyhow` | Error context in binaries | client, daemon |
| `futures` | Stream utilities for event stream | daemon |
| `tokio-stream` | Async stream adapters | daemon |

**Not included (YAGNI):**
- `tracing` / `log` - use `eprintln!` like the Python version until needed
- `nix` (libc bindings) - only if Unix socket permissions need it
- `directories` - XDG paths are trivial to compute manually

---

## Implementation Order

This is the recommended sequence for TDD implementation. Each task should be implemented test-first.

### Phase 1: Protocol & Types (common crate)
1. Command and Response enums with serde derives
2. Bincode serialization round-trip tests
3. Length-prefixed wire format helpers (encode/decode)
4. Socket path constants and XDG runtime dir resolution
5. Config types (ScratchpadConfig, DaemonSettings, OutputOverride)
6. Window/Workspace/Output info types
7. NiriEvent enum
8. Trait definitions (NiriClient, Notifier)

### Phase 2: Config (KDL parsing)
1. KDL config parser - basic scratchpad loading
2. Settings parsing (notify level, watch flag)
3. Size/position parsing with percentage and pixel support
4. Per-output override merging
5. Regex pattern compilation for app_id/title
6. Include file resolution with cycle detection
7. Error handling (malformed KDL, missing files, bad values)

### Phase 3: Client Binary
1. Clap CLI structure with all subcommands
2. Socket connection and command sending
3. Daemon auto-start (spawn via niri, poll for socket)
4. Response handling and display (status output)

### Phase 4: Daemon State
1. DaemonState struct with window/workspace/output tracking
2. Scratchpad state management (register/unregister/mark visible/hidden)
3. Recency tracking and most-recent-hidden lookup
4. State persistence to disk (save/load with session validation)
5. State reconciliation with actual windows

### Phase 5: Scratchpad Manager
1. Toggle named scratchpad (spawn if missing, show/hide/focus)
2. Smart toggle (hide focused or show most recent)
3. Hide focused scratchpad
4. Show scratchpad on current monitor (configure + move + focus)
5. Window configuration (float, resize, position with per-output logic)
6. Position calculation (percentage to pixels conversion)
7. Float/tile/toggle-float operations

### Phase 6: Daemon Server
1. Unix socket server (accept connections, dispatch commands)
2. Niri event stream listener (parse JSON events, update state)
3. Event handling (window opened/closed/focus changed, workspace changes)
4. Config file watcher (detect changes, reload)
5. Daemon lifecycle (start, stop, restart, status)
6. Main event loop orchestration (socket + events + config watcher)

### Phase 7: Real Implementations
1. Real NiriClient (tokio::process::Command calls to niri CLI)
2. Real Notifier (notify-send with dms fallback)
3. Integration wiring in daemon main.rs

### Phase 8: Polish
1. Error messages and user-facing output
2. Clippy + formatting pass
3. Nix flake updates for new dependencies
4. End-to-end manual testing with real niri
