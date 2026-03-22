# niri-tools

A collection of tools for the [niri](https://github.com/YaLTeR/niri) Wayland compositor.

Currently provides scratchpad window management. A background daemon tracks window state via niri's event stream and a CLI client sends commands over a Unix socket. You define scratchpad windows in a config file, then bind the CLI commands to keyboard shortcuts in your niri config to toggle them on and off screen.

## How it works

The daemon (`niri-tools-daemon`) subscribes to niri's event stream and maintains a live mirror of all windows, workspaces, and outputs. When you run a command like `niri-tools scratchpad toggle term`, the CLI connects to the daemon over a Unix socket and tells it to show or hide the window matching that scratchpad definition.

Hidden scratchpad windows are moved to a dedicated off-screen workspace. Showing a scratchpad moves it to the current monitor as a floating window, sized and positioned according to your config.

The daemon auto-starts when you run any scratchpad command.

## Installation

### From source

Requires Rust 1.85+.

```sh
cargo build --release
```

Binaries are at `target/release/niri-tools` and `target/release/niri-tools-daemon`. Place both somewhere on your `$PATH`.

### With Nix

```sh
nix build
```

Or add the flake as an input to your system configuration.

## Configuration

Create `~/.config/niri/niri-tools.kdl`:

```kdl
settings {
  notify "all"
  watch true
}

scratchpad "term" {
  app-id "com.mitchellh.ghostty"
  command "ghostty"
  size width="60%" height="60%"
  position x="50%" y="50%"
}

scratchpad "browser" {
  app-id "firefox"
  command "firefox"
  size width="80%" height="80%"
  position x="50%" y="50%"
}

scratchpad "btop" {
  app-id "com.mitchellh.ghostty"
  command "ghostty" "-e" "btop"
  title "/btop/"
  size width="70%" height="70%"
  position x="50%" y="50%"
}
```

### Scratchpad options

| Field      | Description                                                                                                                  |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `app-id`   | Wayland app ID to match. Prefix with `/` for regex (e.g. `/google-chrome.*`).                                                |
| `title`    | Window title to match. Prefix with `/` or `^` for regex.                                                                     |
| `command`  | Command to spawn the window. Each argument is a separate quoted string (e.g. `command "ghostty" "-e" "btop"`). |
| `size`     | Window size. Accepts percentages (`"60%"`) or pixels (`"800"`).                                                              |
| `position` | Window position. `"0%"` = top/left edge, `"50%"` = centered, `"100%"` = bottom/right edge. The window stays fully on screen. |

### Per-output overrides

Scratchpads can have different sizes or positions on different monitors:

```kdl
scratchpad "term" {
  app-id "com.mitchellh.ghostty"
  command "ghostty"
  size width="60%" height="60%"
  position x="50%" y="50%"

  output "DP-2" {
    position x="50%" y="35%"
  }
  output "eDP-1" {
    size width="80%" height="80%"
  }
}
```

### Settings

| Setting  | Values                                    | Default | Description                                        |
| -------- | ----------------------------------------- | ------- | -------------------------------------------------- |
| `notify` | `"none"`, `"error"`, `"warning"`, `"all"` | `"all"` | Desktop notification verbosity.                    |
| `watch`  | `true`, `false`                           | `true`  | Reload config automatically when the file changes. |

### Includes

Split config across files:

```kdl
include "browsers.kdl"
```

Paths are relative to the including file. The main file's values take precedence over included ones.

## Commands

### Scratchpad

```
niri-tools scratchpad toggle [name]
```

Toggle a named scratchpad. If no name is given, performs a smart toggle: hides the focused scratchpad, or shows the most recently hidden one.

```
niri-tools scratchpad hide
```

Hide the currently focused floating window (moves it to the scratchpad workspace).

```
niri-tools scratchpad toggle-float [name]
niri-tools scratchpad float [name]
niri-tools scratchpad tile [name]
```

Switch a scratchpad between floating and tiled layout. If no name is given, acts on the focused scratchpad.

### Smart focus

```
niri-tools smart-focus --id <window-id>
```

Focus a window by ID with scratchpad-aware behavior:

- If the window is already focused, does nothing.
- If the window does not exist, shows a warning notification.
- If the window is a scratchpad, shows it on the current monitor.
- If the window is a regular window, focuses it (hiding any focused scratchpad first).

### Daemon

```
niri-tools daemon start
niri-tools daemon stop
niri-tools daemon restart
niri-tools daemon status
```

The daemon starts automatically when needed. These commands are for manual control.

## Niri keybinding example

```kdl
binds {
  Mod+Grave { spawn "niri-tools" "scratchpad" "toggle"; }
  Mod+T { spawn "niri-tools" "scratchpad" "toggle" "term"; }
  Mod+B { spawn "niri-tools" "scratchpad" "toggle" "browser"; }
}
```

## File paths

| Path                                     | Description                                                   |
| ---------------------------------------- | ------------------------------------------------------------- |
| `$XDG_CONFIG_HOME/niri/niri-tools.kdl`  | Configuration file (default `~/.config/niri/niri-tools.kdl`) |
| `$XDG_RUNTIME_DIR/niri-tools.sock`       | Daemon socket                                                 |
| `$XDG_RUNTIME_DIR/niri-tools-state.json` | Persisted state (survives daemon restarts)                    |

The socket path can be overridden with the `$NIRI_TOOLS_SOCKET` environment variable.

## License

MIT
