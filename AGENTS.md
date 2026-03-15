# AGENTS.md

## Project Overview

**niri-tools** is a scratchpad window manager for the [niri](https://github.com/YaLTeR/niri) Wayland compositor. Niri lacks built-in scratchpad support; this project fills that gap with a daemon + CLI client architecture that enables toggling floating windows on/off screen via keybindings.

This is a **Rust rewrite** of the original Python implementation. The Python version lives at `~/dotfiles/config/niri/niri_tools/` and serves as the reference for features, behavior, and edge cases. When implementing new features or debugging behavior questions, consult the Python source.

## Architecture

```
niri-tools (CLI)              niri-tools-daemon
┌────────────────┐            ┌──────────────────────────┐
│ Parses CLI args│            │ DaemonServer             │
│ Connects to    │───Unix─────│  - Unix socket listener  │
│ daemon socket  │  socket    │  - niri event stream     │
│ Sends Command  │  (bincode) │  - Config file watcher   │
│ Reads Response │            │                          │
└────────────────┘            │ ScratchpadManager        │
                              │  - toggle/hide/float/tile│
                              │  - spawn, position, size │
                              │                          │
                              │ DaemonState              │
                              │  - windows, workspaces   │
                              │  - outputs, scratchpads  │
                              │  - JSON persistence      │
                              └──────────┬───────────────┘
                                         │
                                    niri msg action/query
                                         │
                                         ▼
                                   niri (Wayland WM)
```

## Crate Structure

```
crates/
  niri-tools/           CLI client binary
  niri-tools-daemon/    Async daemon binary (tokio)
  niri-tools-common/    Shared library (types, protocol, config, traits)
```

Run `cat crates/*/Cargo.toml` to see current dependencies for each crate.

### niri-tools-common (shared library)

| File | Responsibility |
|---|---|
| `types.rs` | Core data types: `WindowInfo`, `WorkspaceInfo`, `OutputInfo`, `NiriEvent` |
| `protocol.rs` | IPC protocol: `Command`/`Response` enums, length-prefixed bincode wire format |
| `config.rs` | Config structs: `ScratchpadConfig`, `SizeConfig`, `PositionConfig`, `OutputOverride`, `DaemonSettings` |
| `config_parser.rs` | KDL config file parser with include resolution and cycle detection |
| `traits.rs` | `NiriClient` and `Notifier` traits (for dependency injection in tests) |
| `error.rs` | `NiriToolsError` enum |
| `paths.rs` | XDG-compliant path helpers: socket, config, state file |

### niri-tools-daemon (daemon binary)

| File | Responsibility |
|---|---|
| `main.rs` | Entry point: tracing setup, creates real implementations, starts server |
| `server.rs` | `DaemonServer`: socket listener, event loop (`tokio::select!`), command dispatch, config reload |
| `state.rs` | `DaemonState`: in-memory state, JSON persistence, reconciliation |
| `scratchpad.rs` | `ScratchpadManager`: toggle/hide/float/tile/spawn logic, position/size calculations, window matching |
| `events.rs` | Event parsing (niri JSON -> `NiriEvent`), event application to state |
| `niri.rs` | `RealNiriClient`: shells out to `niri msg` for actions/queries, subscribes to event stream |
| `notify.rs` | `RealNotifier`: sends notifications via `dms` (preferred) or `notify-send` (fallback) |

### niri-tools (CLI binary)

| File | Responsibility |
|---|---|
| `main.rs` | Clap CLI parsing, Unix socket client, auto-starts daemon if not running |

## Key Design Decisions

- **Trait-based dependency injection**: `NiriClient` and `Notifier` are traits (`crates/niri-tools-common/src/traits.rs`). The daemon uses real implementations; tests use mocks. This is the primary testing strategy.
- **IPC wire format**: Length-prefixed bincode over a Unix domain socket. See `protocol.rs` for `encode_message`/`decode_message`.
- **Config format**: KDL (not YAML). The Python version uses YAML. Config lives at `~/.config/niri/scratchpads.kdl`.
- **State persistence**: Scratchpad-to-window mappings are saved to `$XDG_RUNTIME_DIR/niri-tools-state.json` so the daemon can recover after restarts.
- **Auto-start**: The CLI spawns the daemon via `niri msg action spawn` if the socket is not available (except for daemon management commands like `stop`/`status`).

## Commands

Run `cargo run --bin niri-tools -- --help` to see the current CLI interface.

Current commands:

```
niri-tools daemon start|stop|restart|status
niri-tools scratchpad toggle [name]
niri-tools scratchpad hide
niri-tools scratchpad toggle-float [name]
niri-tools scratchpad float [name]
niri-tools scratchpad tile [name]
niri-tools smart-focus --id <window-id>
```

See `Command` enum in `crates/niri-tools-common/src/protocol.rs` for the canonical list of supported IPC commands.

## Python Reference Implementation

The original Python version at `~/dotfiles/config/niri/niri_tools/` has features **not yet ported** to Rust. Consult these files when implementing:

| Feature | Python file | Status in Rust |
|---|---|---|
| adopt (register existing window as scratchpad) | `daemon/scratchpad.py` | Not implemented |
| disown (unregister a scratchpad) | `daemon/scratchpad.py` | Not implemented |
| menu (rofi-based scratchpad picker) | `daemon/scratchpad.py` | Not implemented |
| close (with rofi confirmation) | `daemon/scratchpad.py` | Not implemented |
| urgency handling | `daemon/urgency.py` | Not implemented |

Run `diff <(grep 'def ' ~/dotfiles/config/niri/niri_tools/daemon/scratchpad.py) <(grep 'fn ' crates/niri-tools-daemon/src/scratchpad.rs)` to compare available functions.

## Build & Development

### Prerequisites

- Rust stable (see `rust-toolchain.toml` for exact components)
- Nix (optional, for reproducible builds via `flake.nix`)

### Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test                     # Run all tests
cargo clippy --all-features    # Lint (CI runs with -Dwarnings)
nix build                     # Nix release build
nix develop                   # Enter dev shell (includes dprint)
```

### Formatting

Non-Rust files are formatted with `dprint` (see `dprint.json` for config). Rust code uses `rustfmt`.

### CI

CI runs on every push (see `.github/workflows/lint.yml`):
- `nix flake check`
- Clippy with `-Dwarnings`

Releases are built on version tags (`v*.*.*`) via `.github/workflows/release.yml`.

## Conventions

### Commit Style

Conventional commits: `type(scope): description`

Types: `feat`, `fix`, `chore`, `refactor`, `test`, `docs`

Scopes: `client`, `daemon`, `common`, `config`, `nix`

Run `git log --oneline -20` to see recent examples.

### Code Style

- 2-space indentation for non-Rust files (`.editorconfig`)
- Rust edition 2024, MSRV 1.85
- Tests are inline (`#[cfg(test)] mod tests` blocks), not in separate files
- Mocks for `NiriClient` and `Notifier` are defined within test modules

### File Paths

All runtime paths are XDG-compliant. See `crates/niri-tools-common/src/paths.rs` for the canonical path logic:
- Socket: `$NIRI_TOOLS_SOCKET` or `$XDG_RUNTIME_DIR/niri-tools.sock`
- Config: `$XDG_CONFIG_HOME/niri/scratchpads.kdl`
- State: `$XDG_RUNTIME_DIR/niri-tools-state.json`

## Testing

**Prefer test-driven development (TDD) when implementing features and bug fixes.** Write the failing test first, verify it fails for the right reason, then write minimal code to make it pass. This applies to new commands, scratchpad logic, event handling, and protocol changes.

All tests are inline unit tests. Run with `cargo test`.

Tests use mock implementations of `NiriClient` and `Notifier` traits. Look at existing test modules in `scratchpad.rs`, `server.rs`, and `events.rs` for patterns on how to write new tests.

To see test coverage by file:
```bash
cargo test 2>&1 | grep 'test result'          # Summary
cargo test -- --list 2>/dev/null | grep '::'   # List all test names
```

## Maintaining This File

**If you change something documented here, update this file.** Specifically:

- Adding a new crate or source file: update the Crate Structure tables
- Adding/removing a CLI command or IPC command: update the Commands section
- Porting a feature from the Python reference: update the Python Reference Implementation table
- Changing build commands, CI, or tooling: update the Build & Development section
- Changing conventions (commit style, file paths, etc.): update the Conventions section
- Changing the architecture (new IPC mechanism, new daemon components): update the Architecture diagram
