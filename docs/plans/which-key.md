# Design: Submodes for niri-tools

Successor to `wlr-which-key`. Displays a submode popup (key => action/submode
menu) as a layer-shell overlay, managed by the niri-tools daemon for instant
show/hide.

## Reference projects

### wlr-which-key (predecessor, being replaced)

Root: `/home/boo/proj/wlr-which-key/worktree/b0o`

| File | Description |
|------|-------------|
| `src/main.rs` | Entry point, Wayland event loop, `State` struct, keyboard handling, key-release-before-hide fix |
| `src/menu.rs` | Menu model: pages, columns, key dispatch, action types (`Quit`, `Exec`, `Submenu`), rendering |
| `src/config.rs` | Config loading, top-level `Config` struct, `pub use` re-exports |
| `src/config/entry.rs` | `Entry` enum (`Cmd`/`Recursive`), `RawEntry` deserialization, `TryFrom` validation |
| `src/config/anchor.rs` | `ConfigAnchor` enum (screen position), conversion to layer-shell anchor |
| `src/config/compat.rs` | Legacy config format support with `From` conversion |
| `src/config/font.rs` | `Font` newtype around `pango::FontDescription` |
| `src/config/namespace.rs` | `Namespace` newtype around `CString` |
| `src/color.rs` | `Color` type with hex parsing, cairo integration, custom serde visitor |
| `src/key.rs` | `Key`/`SingleKey`/`ModifierState` types, key parsing, modifier matching |
| `src/text.rs` | `ComputedText` -- pango text layout and cairo rendering |

User config: `~/.config/wlr-which-key/config.yaml`

### whisper-overlay (gtk4-layer-shell reference)

Root: `/home/boo/proj/whisper-overlay/worktree/main`

| File | Description |
|------|-------------|
| `src/app.rs` | GTK4 Application setup, layer-shell config, window creation, show/hide lifecycle, CSS loading, `EventControllerKey` |
| `src/main.rs` | Entry point, tokio `OnceLock<Runtime>` pattern for async bridging |
| `src/style.css` | CSS styling: transparent window, rounded container, text styles |

Key patterns to reference:
- `app.hold()` to keep process alive when window is hidden (`src/app.rs:416`)
- Layer-shell init sequence (`src/app.rs:508-513`)
- Empty Wayland input region for click-through (`src/app.rs:517-523`)
- Show/hide via `window.present()` / `window.set_visible(false)` (`src/app.rs:678-687`)

## Goals

- **Instant popup** -- the daemon keeps GTK and the menu pre-initialized;
  show/hide is a single IPC message, no process spawn or font init.
- **GTK4 + gtk4-layer-shell** -- delegate all rendering, text layout, and
  Wayland surface management to GTK. No manual pango/cairo/wlr-layer-shell.
- **KDL config** -- extend the existing `niri-tools.kdl` config format.
- **Flat mode references** -- submodes reference each other by identifier
  instead of nesting, enabling reuse and keeping the config readable.
- **Inherit niri styles** -- where possible, read font/color/style settings
  from the user's niri config so the popup matches the compositor theme
  out of the box, with per-property overrides available.
- **Feature-scoped** -- only implement features actually in use (see
  "Dropped features" below).

## Architecture

### Component overview

```
niri-tools (CLI)            niri-tools-daemon
  |                           |
  |-- IPC: SubmodeShow  -->   |-- SubmodeState (pre-built menu model)
  |-- IPC: SubmodeHide  -->   |-- GTK4 ApplicationWindow + layer-shell
  |                           |   (created once, shown/hidden)
  |                           |-- Keyboard handling (GTK key events)
  |                           |-- Command execution (on action)
```

### Daemon side

The daemon already runs a tokio event loop with a Unix socket listener. The
submode feature adds:

1. **A GTK4 `Application`** initialized at daemon startup. This is the big
   architectural question -- the daemon currently has no GUI. GTK4 requires
   running its own main loop. Two viable approaches:

   **Option A: GTK main loop as primary, tokio on a thread.**
   The daemon's `main()` calls `app.run()` which owns the main thread. The
   existing tokio event loop (socket listener, niri event stream) runs on a
   dedicated `tokio::Runtime` spawned on a background thread, bridged to GTK
   via `glib::spawn_future_local` / `glib::MainContext::channel`. This is the
   same pattern used in whisper-overlay.

   **Option B: tokio main loop as primary, GTK on a thread.**
   Keep the current tokio `main()`. Spawn GTK on a dedicated thread. Bridge
   via channels. Less conventional -- GTK generally expects to own the main
   thread on Wayland.

   **Recommendation: Option A.** GTK on main thread is the well-trodden path.
   The tokio runtime for niri IPC and event stream moves to a background
   thread. The socket listener dispatches submode commands to the GTK thread
   via a glib channel.

2. **`SubmodeState`** -- a struct holding:
   - The parsed menu model (modes, bindings, labels)
   - The current mode stack (for back-navigation)
   - The GTK window + widgets (created once, shown/hidden)

3. **Window lifecycle:**
   - `SubmodeShow { mode }` -- set the active mode, populate widgets, call
     `window.present()`. The window is an `ApplicationWindow` with
     `gtk4-layer-shell` configured for `Layer::Overlay` with exclusive keyboard
     interactivity.
   - `SubmodeHide` -- `window.set_visible(false)`. The window stays alive
     in memory.
   - Key press handling via GTK's `EventControllerKey` on the window. On a
     closing action, hide content + wait for key release (same fix as
     wlr-which-key) before hiding.

4. **Command execution** -- actions that run shell commands use `std::process::Command`
   with the existing `daemon(1, 0)` double-fork pattern (fire-and-forget).

### CLI side

New subcommand group:

```
niri-tools submode show [mode]    # show popup, optionally at a specific mode
niri-tools submode hide           # dismiss popup
niri-tools submode toggle [mode]  # toggle visibility
```

These map to new `Command` variants sent over the existing IPC socket.
Auto-start behavior (spawn daemon if not running) applies as with scratchpad
commands.

### niri keybinding integration

In `~/.config/niri/config.kdl`:

```kdl
binds {
    Mod+Space { spawn "niri-tools" "submode" "show"; }
    Mod+B     { spawn "niri-tools" "submode" "show" "brightness"; }
    Mod+R     { spawn "niri-tools" "submode" "show" "resize"; }
}
```

Each invocation is just a socket message -- the CLI connects, writes the
command, and exits. The daemon (already running) shows the pre-built window
within milliseconds.

## Config format

Extend `niri-tools.kdl` with a `submode` section. Modes are top-level,
referenced by string identifier. Style settings are optional -- defaults are
inherited from the user's niri config where possible (see "Style resolution"
below).

```kdl
submode {
    settings {
        // All style properties are optional. Unset values are resolved from
        // the niri config or fall back to sensible defaults.
        // font "monospace 12"
        // background "#282828"
        // color "#fbf1c7"
        separator "  "
        anchor "bottom"
        margin-bottom -33
        corner-radius 2
        padding 4
        column-padding 50
        min-width 1000
    }

    // Root mode (shown by default when no mode is specified)
    mode "root" {
        entry "`"     desc="Lock"           cmd="sleep 0.2 && dms ipc lock lock"
        entry "Space" desc="Launcher"       cmd="rofi -show drun -modi drun"
        entry "o"     desc="Open"           mode="open"
        entry "s"     desc="Systemd"        mode="systemd"
        entry "b"     desc="Brightness"     mode="brightness"
        entry "n"     desc="Notifications"  mode="notifications"
        entry "S"     desc="Screenshot"     mode="screenshot"
        entry "z"     desc="Z"              mode="z"
    }

    mode "brightness" {
        entry "?" desc="Query"  cmd="ddcutil getvcp 10 | notify"  alias="q"
        entry "j" desc="-5"     cmd="ddcutil setvcp 10 - 5"       keep-open
        entry "k" desc="+5"     cmd="ddcutil setvcp 10 + 5"       keep-open
        entry "1" desc="10"     cmd="ddcutil setvcp 10 10"
        entry "0" desc="100"    cmd="ddcutil setvcp 10 100"
    }

    mode "resize" {
        entry "1" desc="10%"  cmd="niri msg action set-window-width 10%"
        entry "2" desc="20%"  cmd="niri msg action set-window-width 20%"
        entry "h" desc="Resize Height" mode="resize-height"
    }

    mode "resize-height" {
        entry "1" desc="10%" cmd="niri msg action set-window-height 10%"
        entry "w" desc="Resize Width" mode="resize"
    }

    mode "notifications" {
        entry "d" desc="Dismiss"     cmd="swaync-client -d" keep-open
        entry "D" desc="Dismiss All" cmd="swaync-client -C"
        entry "p" desc="Previous"    cmd="swaync-client --show-notification" keep-open
        entry "a" desc="Action"      cmd="swaync-client --latest-notification-action"
    }
}
```

### Config design decisions

**Flat modes, not nested.** Each mode has a unique string identifier. An entry
with `mode="..."` switches to that mode. This enables:
- Bidirectional navigation (resize <-> resize-height) without duplication
- Reuse of a mode from multiple parents
- Simpler config for deeply nested menus

**Aliases.** `alias="q"` or `alias="q" alias="Mod4+?"` (multiple alias
properties) for alternate key bindings to the same action. Aliases can trigger
the action but are not displayed.

**`keep-open` flag.** Entry stays in the current mode after executing the
command (for brightness, notifications, etc.).

**Back navigation.** Escape closes the popup. Backspace goes to the previous
mode (the daemon tracks a mode stack). Ctrl+[ and Ctrl+g also close (vim/emacs
convention).

**Dropped features** (not in use in wlr-which-key):
- `rows_per_column` -- the existing config sets this to 1 (horizontal-only
  layout). The new implementation uses a single-row horizontal layout. If
  multi-row is needed later, add `rows-per-column` to settings.
- `inhibit_compositor_keyboard_shortcuts` -- niri's layer-shell exclusive
  keyboard interactivity handles this. Not needed.
- `auto_kbd_layout` -- not in use. Can be added later if needed.
- `hide` on entries -- was used for hidden aliases (e.g., `Mod4+o` hidden
  duplicate of `o`). The new `alias` mechanism replaces this entirely.

## Style resolution

Style properties (font, colors, border, etc.) are resolved with a layered
fallback:

```
niri-tools.kdl submode settings  >  niri config  >  built-in defaults
```

### Reading from niri config

niri's `config.kdl` defines visual properties that the submode popup can
inherit:

```kdl
// In niri's config.kdl:
layout {
    border { width 2; active-color "#8ec07c"; }
    focus-ring { /* ... */ }
    default-column-width { proportion 0.5; }
}
```

**Approach 1: Parse niri's config.kdl directly.**
The niri config path is known (`$XDG_CONFIG_HOME/niri/config.kdl`). Parse it
with the `kdl` crate to extract relevant style properties. This is
straightforward since we already have a KDL parser. Relevant properties:
- `layout.border.active-color` -> popup border/accent color
- `layout.border.width` -> popup border width
- Font: niri doesn't expose a global font setting, so this would remain
  a submode-specific config or use a system default.

**Approach 2: niri IPC.**
`niri msg -j` exposes runtime state but does not currently expose config
style properties (colors, fonts). If niri adds a `config` IPC endpoint in
the future, this would be the preferred approach. For now, direct config
parsing is more complete.

**Approach 3: Hybrid.**
Parse niri config for style properties at daemon startup. If niri later
exposes style info via IPC, prefer that. Either way, the user can always
override any property in `niri-tools.kdl`.

**Recommendation: Approach 1 (parse niri config) initially**, with the
architecture designed so the style source is pluggable. The daemon already
loads config at startup; reading a few extra properties from a sibling KDL
file is low cost.

### Built-in defaults

When neither the niri-tools config nor the niri config provide a value:

| Property | Default | Notes |
|----------|---------|-------|
| font | System monospace, 12pt | `monospace 12` pango description |
| background | `#282828ff` | Dark neutral |
| color | `#fbf1c7ff` | Light text |
| accent | from niri `border.active-color`, or `#8ec07c` | Used for submode labels |
| separator | ` -> ` | Between key and description |
| anchor | `center` | Screen position |
| margin-* | `0` | All edges |
| corner-radius | `8` | Rounded corners |
| padding | same as corner-radius | Inner padding |
| column-padding | same as padding | Between columns |
| border-width | `0` | No border by default |
| min-width | none | Auto-sized to content |

## GTK window design

### Layer-shell configuration

```rust
window.init_layer_shell();
window.set_layer(Layer::Overlay);
window.set_keyboard_mode(KeyboardMode::Exclusive);
window.set_namespace(Some("niri-tools-submode"));
// Anchor and margins from resolved config settings
```

Exclusive keyboard mode is essential -- the popup must capture all key input.

### Widget structure

```
ApplicationWindow (layer-shell, transparent background)
  Box (horizontal, CSS: background, rounded corners, padding)
    Box (column 0, vertical)
      Box (horizontal: key_label + separator + desc_label)
      Box (horizontal: key_label + separator + desc_label)
      ...
    Box (column 1, vertical) ...
```

All labels are `gtk::Label`. Layout is handled by GTK's box model. Styling
via CSS (loaded once at startup, same pattern as whisper-overlay):

```css
window { background-color: transparent; }
.submode-container {
    /* background, border-radius, padding, border generated from config */
}
.submode-key { /* font, color from config */ }
.submode-sep { /* color from config */ }
.submode-desc { /* color from config */ }
.submode-desc-mode { /* accent color for submode entries */ }
```

CSS is generated from the resolved config settings at startup. The user can
also provide a custom CSS file for full control.

### Keyboard handling

Attach an `EventControllerKey` to the window:

```rust
let key_controller = EventControllerKey::new();
key_controller.connect_key_pressed(move |_, keyval, keycode, modifiers| {
    // Look up action in current mode
    // If closing action: exec command, hide content, record keycode
    // If mode switch: push mode stack, rebuild UI
    // If keep-open: exec command, stay
    Propagation::Stop
});
key_controller.connect_key_released(move |_, keyval, keycode, modifiers| {
    // If waiting for this specific key release: hide window, clear state
});
window.add_controller(key_controller);
```

The key-release-before-hide fix from wlr-which-key carries over: on a closing
action, immediately exec the command and hide all child widgets (or set opacity
to 0), but keep the window mapped with keyboard grab until the specific
triggering keycode is released. Other key releases are ignored.

### Show/hide flow

**Show:**
1. Daemon receives `SubmodeShow { mode }` via IPC
2. Set active mode (or root if none specified), push onto mode stack
3. Rebuild widget content for the mode (swap out labels)
4. `window.present()`

**Hide:**
1. Daemon receives `SubmodeHide` via IPC (or Escape pressed)
2. `window.set_visible(false)`
3. Clear mode stack

The window object is never destroyed -- `app.hold()` keeps the process alive.
Rebuilding the label contents on show is cheap since the font and layout
engine are already loaded.

## IPC protocol changes

Add to `Command`:

```rust
pub enum Command {
    // ... existing variants ...
    SubmodeShow { mode: Option<String> },
    SubmodeHide,
    SubmodeToggle { mode: Option<String> },
}
```

`Response::Ok` suffices initially. A `SubmodeStatus { visible, mode }` variant
can be added later if needed.

## Implementation plan

### Phase 1: Foundation
1. Add `gtk4`, `gtk4-layer-shell`, `gdk4-wayland` dependencies to daemon crate
2. Restructure daemon main loop: GTK main thread + tokio background thread
3. Add `submode` KDL config parsing to `config_parser.rs`
4. Add `SubmodeShow`/`SubmodeHide`/`SubmodeToggle` to `Command` enum
5. Add CLI subcommands

### Phase 2: Window and rendering
6. Create the GTK layer-shell window at daemon startup (hidden)
7. Build the menu widget tree from config
8. Implement show/hide via IPC commands
9. Generate CSS from resolved config settings

### Phase 3: Keyboard handling
10. Implement `EventControllerKey` for key dispatch
11. Implement mode switching with back-navigation (mode stack)
12. Implement key-release-before-hide behavior (track keycode)
13. Implement command execution (fire-and-forget shell commands)
14. Implement `keep-open` mode

### Phase 4: Style inheritance
15. Parse niri config.kdl for style properties (border color, etc.)
16. Implement layered style resolution (submode config > niri config > defaults)

### Phase 5: Polish
17. Alias support
18. Config hot-reload (extend existing `DaemonRestart` or file watcher)
19. Validation: error on invalid mode references, duplicate mode IDs
20. Tests for config parsing, mode navigation, command construction

## Open questions

1. **GTK + tokio integration** -- the daemon currently uses `#[tokio::main]`.
   Switching to GTK main loop is a significant refactor of the daemon's
   entry point and event loop. Need to ensure scratchpad functionality
   (niri event stream, socket listener) continues working correctly on the
   tokio background thread. Alternatively, use `gtk4::gio::spawn_blocking`
   or `glib::MainContext::spawn_local` to bridge.

2. **Multiple monitors** -- should the popup appear on the focused monitor?
   Layer-shell can target a specific output. The daemon already tracks
   `focused_output` from the niri event stream and can set the output on
   the layer surface before presenting.

3. **Animation** -- should the popup fade in/out? GTK CSS transitions could
   handle opacity, or it can be left to the compositor (niri supports layer
   animations via layer-rules). Probably best left to the compositor.

4. **Config migration** -- provide a one-time migration script/command from
   `wlr-which-key` YAML to the new KDL format, or document the manual
   conversion.

5. **niri config changes** -- should the daemon watch the niri config for
   style changes? Could reuse the existing file watcher infrastructure
   (already watches `niri-tools.kdl` when `watch true` is set). Low priority
   since niri style changes are infrequent.
