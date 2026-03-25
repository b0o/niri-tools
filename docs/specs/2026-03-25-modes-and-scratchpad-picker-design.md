# Design: Modes & Scratchpad Picker for niri-tools

Successor to `wlr-which-key`. Two GTK4 layer-shell UIs managed by the
niri-tools daemon:

1. **Mode overlay** -- a which-key-style horizontal key-hint bar for
   dispatching actions via single keypresses.
2. **Scratchpad picker** -- a searchable vertical list for browsing and
   toggling scratchpads with live state indicators.

Both are pre-initialized at daemon startup and shown/hidden via IPC --
no process spawn, no font init on each invocation.

## Reference projects

| Project | Path | Role |
|---------|------|------|
| wlr-which-key | `/home/boo/proj/wlr-which-key/worktree/b0o` | Predecessor being replaced |
| whisper-overlay | `/home/boo/proj/whisper-overlay/worktree/main` | GTK4 + tokio + layer-shell reference |
| niri | niri config docs | Config style reference |

## Config format

### Top-level structure

```kdl
notifications "all"

ui { /* visual settings */ }

scratchpad "name" { /* window management */ }

mode "name" { /* keybinding set */ }
```

Four top-level node types. Two semantic primitives (scratchpad, mode),
one visual settings block (ui), one daemon setting (notifications).

### `notifications`

Daemon notification level. Replaces the old `settings { notify "..." }`
block.

```kdl
notifications "all"     // all | warning | error | none
```

The old `settings { watch true }` is removed -- config is always watched,
matching niri's behavior.

### `ui { }`

Visual settings for both UIs. Global defaults at the top level, with
per-UI overrides in `modes { }` and `scratchpads { }`.

```kdl
ui {
    // Global defaults -- inherited by both UIs
    font "Pragmasevka Nerd Font 12"
    background-color "#2F2A4C"
    color "#DFD9FB"
    corner-radius 2

    // Mode overlay specific
    modes {
        anchor "bottom"
        separator "  "
        margin-bottom -33
        padding 4
        column-padding 50
        min-width 1000
        // font "..."            // override global
        // background-color "..."
    }

    // Scratchpad picker specific
    scratchpads {
        anchor "center"
        padding 12
        // width 500
        // corner-radius 8       // override global
    }
}
```

#### Style resolution

Properties resolve with layered fallback:

```
ui.modes / ui.scratchpads  >  ui (global)  >  niri config  >  built-in defaults
```

The daemon reads relevant style properties from niri's `config.kdl` at
startup (e.g., `layout.border.active-color` for accent color). The user
can override any property at any level.

#### Built-in defaults

| Property | Default | Notes |
|----------|---------|-------|
| font | `monospace 12` | Pango font description |
| background-color | `#282828ff` | Dark neutral |
| color | `#fbf1c7ff` | Light text |
| accent-color | from niri `border.active-color`, or `#8ec07c` | State indicators, submode labels |
| separator | ` -> ` | Between key and description (modes) |
| anchor | `center` | Screen position |
| margin-* | `0` | All edges |
| corner-radius | `8` | Rounded corners |
| padding | same as corner-radius | Inner padding |
| column-padding | same as padding | Between columns (modes) |
| border-width | `0` | No border by default |
| min-width | none | Auto-sized to content |

### `scratchpad "name" { }`

Window management definition. Unchanged from the existing schema except
for two new optional fields: `key` and `desc`.

```kdl
scratchpad "term" {
    key "t"                         // optional: shortcut in picker
    desc "Terminal"                 // optional: display name (defaults to name)
    app-id "com.mitchellh.ghostty"
    command "ghostty"
    auto-adopt true                 // optional, default false
    size width="60%" height="60%"
    position x="50%" y="35%"
    output "DP-2" {
        position x="50%" y="35%"
    }
}
```

`key` is the shortcut key used in the scratchpad picker UI. It is fired
via Mod+key while the picker is open. Scratchpads without `key` are
accessible via fuzzy search and CLI but have no shortcut.

`desc` is the display name shown in the scratchpad picker. Defaults to
the scratchpad name if omitted.

These fields have no effect on mode binds -- they are picker-specific
metadata.

### `mode "name" { }`

A named set of keybindings shown in the mode overlay. Modes are flat and
reference each other by name.

```kdl
mode "root" {
    binds {
        "`"     "Lock"          { spawn-sh "sleep 0.2 && dms ipc lock lock"; }
        Space   "Launcher"      { spawn-sh "rofi -show drun -modi drun"; }
        o       "Open"          { switch-mode "open"; }
        s       "Scratchpads"   { scratchpad-pick; }
        b       "Brightness"    { switch-mode "brightness"; }
        n       "Notifications" { switch-mode "notifications"; }
        S       "Screenshot"    { switch-mode "screenshot"; }
        z       "Z"             { switch-mode "z"; }
    }
}
```

#### Mode-level settings

Settings are direct children of the mode block, before `binds { }`.

| Setting | Type | Description |
|---------|------|-------------|
| `keep-open` | flag | All entries stay open by default. Individual entries can override with `close;`. |

```kdl
mode "brightness" {
    keep-open
    binds {
        j "-5" { spawn-sh "brightness -5"; }
        k "+5" { spawn-sh "brightness +5"; }
    }
}
```

#### Bind syntax

```
key "description" { [options;] action [args]; }
```

- **key**: node name. Single letters (`o`, `b`, `S`) are bare identifiers.
  Numbers (`"1"`), special chars (`"?"`), and modifier combos (`"Ctrl+j"`)
  are quoted strings.
- **description**: first argument. Displayed in the overlay.
- **options**: child flag nodes before the action.
- **action**: child node with optional arguments.

#### Bind options

| Option | Description |
|--------|-------------|
| `keep-open;` | Stay in current mode after executing (overrides mode default) |
| `close;` | Close mode after executing (overrides mode-level `keep-open`) |
| `alias "key";` | Alternate key that triggers the same action (not displayed). Multiple `alias` nodes allowed. |

Options come before the action for readability.

#### Actions

**Shell commands:**
| Action | Description |
|--------|-------------|
| `spawn-sh "cmd"` | Run via `sh -c` (fire-and-forget) |
| `spawn "prog" "arg1" "arg2"` | Direct exec, no shell |

**Mode navigation:**
| Action | Description |
|--------|-------------|
| `switch-mode "name"` | Push onto mode stack, switch overlay |
| `scratchpad-pick` | Open the scratchpad picker UI |

**Scratchpad actions (internal, zero overhead):**
| Action | Description |
|--------|-------------|
| `scratchpad-toggle ["name"]` | Toggle scratchpad (no name = most recent) |
| `scratchpad-hide` | Hide visible scratchpad |
| `scratchpad-float ["name"]` | Float a scratchpad |
| `scratchpad-tile ["name"]` | Tile a scratchpad |
| `scratchpad-toggle-float` | Toggle float/tile |
| `scratchpad-adopt` | Adopt focused window as scratchpad |
| `scratchpad-disown` | Unregister a scratchpad |

**Niri actions (pass-through):**

Any unrecognized action name is forwarded to niri as
`niri msg action <name> <args>`. This enables direct use of niri
actions without shelling out:

```kdl
mode "resize" {
    binds {
        e "Expand" { expand-column-to-available-width; }
        "5" "50%"  { set-window-width "50%"; }
    }
}
```

Pass-through is future-proof: new niri actions work immediately without
an niri-tools update.

**Multi-step operations** use shell composition:

```kdl
d "Theme" { spawn-sh "niri msg action do-screen-transition && dms ipc theme toggle"; }
```

One action per bind. The shell is the action compositor.

## Mode overlay UX

### Layout

Horizontal bar anchored to a screen edge (default: bottom). Each entry
is a column showing `key separator description`:

```
 ` Lock    Space Launcher    o Open    s Scratchpads    b Brightness    n Notifications
```

### Keyboard behavior

**Super/Mod is ignored.** The overlay strips Super from incoming key
events before matching. Users open the mode with `Mod+Space` and may
still hold Mod -- the overlay should match `d` regardless of whether
Super is held. Ctrl, Shift, and Alt are real modifiers and are respected.

This eliminates the need for `Mod4+` aliases (the biggest source of
config noise in wlr-which-key).

**Back navigation:**
- Escape: close the overlay
- Backspace: go to previous mode (mode stack)
- Ctrl+[ and Ctrl+g: close (vim/emacs convention)

**Key-release-before-hide:** On a closing action, execute the command
and hide all child widgets immediately, but keep the window mapped with
keyboard grab until the triggering keycode is released. This prevents
spurious key-release events from reaching the next focused window.
Same fix as wlr-which-key.

### Show/hide flow

**Show:**
1. Daemon receives `ModeShow { mode }` via IPC
2. Set active mode (or root if none specified), push onto mode stack
3. Rebuild widget content for the mode
4. `window.present()`

**Hide:**
1. Daemon receives `ModeHide` via IPC (or Escape pressed)
2. `window.set_visible(false)`
3. Clear mode stack

The window is created once at startup and never destroyed. `app.hold()`
keeps the process alive.

### Live state rendering

When an entry's action is `scratchpad-toggle "name"`, the renderer
queries `DaemonState` for that scratchpad's status and applies a CSS
class:

| State | CSS class | Visual |
|-------|-----------|--------|
| Visible (tiled) | `.state-visible` | Accent color |
| Visible (floating) | `.state-floating` | Accent color + different indicator |
| Hidden | (none) | Normal |
| Not spawned | `.state-unspawned` | Dimmed |

This is automatic -- no config needed. The renderer detects
scratchpad-related actions and shows state.

## Scratchpad picker UX

### Layout

Vertical list centered on screen. Each row shows an optional shortcut
hint, display name, and state indicator:

```
  [t] Terminal             *
  [d] Discord
  [e] Matrix               *
  [m] Music
  [f] Figma                ~
  [ ] Parsec

  > _
```

`*` = visible, `~` = floating, dimmed = not spawned. Scratchpads without
`key` show `[ ]` or no bracket.

### Interaction model

**Fuzzy search (default).** Normal typing populates a search buffer. The
list filters in real time using fuzzy matching (frizbee crate). Enter
toggles the top match. Arrow keys navigate.

**Shortcut keys.** Mod+key fires the shortcut instantly. If the picker
is open and the user presses `Mod+t`, it toggles the scratchpad with
`key "t"` and dismisses the picker. This is the "I know what I want"
fast path.

**Disambiguation:** bare keypresses go to fuzzy search. Mod+key goes to
shortcuts. No ambiguity.

**Escape** dismisses the picker.

### Invocation

- CLI: `niri-tools scratchpad pick`
- Mode action: `scratchpad-pick;`
- niri keybinding: `Mod+Ctrl+S { spawn "niri-tools" "scratchpad" "pick"; }`

## Architecture

### Component overview

```
niri-tools (CLI)            niri-tools-daemon
  |                           |
  |-- IPC: ModeShow    -->    |-- Mode overlay (GTK4 layer-shell window)
  |-- IPC: ModeHide    -->    |   (horizontal bar, key hints, key dispatch)
  |-- IPC: ScratchpadPick ->  |-- Scratchpad picker (GTK4 layer-shell window)
  |                           |   (vertical list, fuzzy search, state indicators)
  |                           |-- Keyboard handling (GTK EventControllerKey)
  |                           |-- Command execution (shell / niri IPC / internal)
```

### GTK + tokio integration

**Option A (recommended): GTK main loop as primary, tokio on a
background thread.**

The daemon's `main()` calls `app.run()` which owns the main thread. The
existing tokio event loop (socket listener, niri event stream, signal
handlers) runs on a dedicated `tokio::Runtime` spawned via
`OnceLock<Runtime>` (same pattern as whisper-overlay).

Bridge:
- tokio -> GTK: `glib::spawn_future_local()` receiving from
  `tokio::sync::mpsc`
- GTK -> tokio: `runtime().spawn()`

This is a significant refactor of the daemon. The current
`#[tokio::main]` with `tokio::select!` and `&mut self` must be
restructured:
1. The socket listener, niri event stream, and signal handlers move to
   tokio background tasks.
2. Commands from the socket are forwarded to the GTK thread via a glib
   channel.
3. `DaemonState` lives on the GTK thread. Commands are dispatched there.

### Two GTK windows

Both are `ApplicationWindow` with `gtk4-layer-shell`:

1. **Mode overlay:**
   - `Layer::Overlay`
   - `KeyboardMode::Exclusive`
   - Namespace: `niri-tools-mode`
   - Anchor/margins from `ui.modes` config

2. **Scratchpad picker:**
   - `Layer::Overlay`
   - `KeyboardMode::Exclusive`
   - Namespace: `niri-tools-scratchpad-picker`
   - Anchor/margins from `ui.scratchpads` config

Both created at startup (hidden). `app.hold()` keeps the process alive.
Show/hide via `window.present()` / `window.set_visible(false)`.

### CSS

CSS is generated from resolved config settings at startup and applied
via `CssProvider` at `STYLE_PROVIDER_PRIORITY_APPLICATION`. CSS classes
are used for state-aware rendering (`.state-visible`, `.state-floating`,
etc.).

### IPC protocol changes

Add to `Command`:

```rust
pub enum Command {
    // ... existing variants ...
    ModeShow { mode: Option<String> },
    ModeHide,
    ModeToggle { mode: Option<String> },
    ScratchpadPick,
}
```

### CLI changes

New subcommand group:

```
niri-tools mode show [name]     # show overlay (default: root mode)
niri-tools mode hide            # dismiss overlay
niri-tools mode toggle [name]   # toggle visibility
niri-tools scratchpad pick      # open scratchpad picker
```

### Niri keybinding integration

```kdl
// In niri's config.kdl
binds {
    Mod+Space  { spawn "niri-tools" "mode" "show"; }
    Mod+B      { spawn "niri-tools" "mode" "show" "brightness"; }
    Mod+R      { spawn "niri-tools" "mode" "show" "resize"; }
    Mod+Ctrl+S { spawn "niri-tools" "scratchpad" "pick"; }
}
```

## Implementation phases

### Phase 1: Foundation
1. Add `gtk4`, `gtk4-layer-shell`, `gdk4-wayland` dependencies
2. Restructure daemon: GTK main loop + tokio background thread
3. Add `mode` and `ui` KDL config parsing
4. Add `ModeShow`/`ModeHide`/`ModeToggle` to `Command` enum
5. Add CLI subcommands

### Phase 2: Mode overlay
6. Create GTK layer-shell window at daemon startup (hidden)
7. Build the key-hint widget tree from config
8. Implement show/hide via IPC
9. Generate CSS from resolved config
10. Implement `EventControllerKey` for key dispatch
11. Implement mode switching with back-navigation (mode stack)
12. Implement key-release-before-hide
13. Implement Super-ignore for modifier stripping
14. Implement command execution (spawn-sh, spawn, niri action pass-through)
15. Implement `keep-open` / `close` behavior

### Phase 3: Scratchpad picker
16. Create second GTK layer-shell window (hidden)
17. Build the searchable list widget
18. Integrate frizbee for fuzzy matching
19. Implement shortcut key dispatch (Mod+key)
20. Implement live state rendering from `DaemonState`
21. Implement `scratchpad-pick` action and IPC command

### Phase 4: Style inheritance
22. Parse niri config.kdl for style properties
23. Implement layered style resolution

### Phase 5: Polish
24. Config hot-reload for mode/UI changes
25. Validation: error on invalid mode references, duplicate keys
26. Multi-monitor: show on focused output
27. Tests for config parsing, mode navigation

## Open questions

1. **Conditional GTK** -- should the daemon always init GTK, even if no
   modes are configured? If someone only uses scratchpads, pulling in
   GTK is heavy. Options: always init (simpler), feature flag
   (compile-time), or lazy init (tricky since GTK must own main thread
   from the start).

2. **Animation** -- should the popup fade in/out? Best left to niri's
   layer-rules. The daemon sets the layer-shell namespace; niri can
   match on it for animations.

3. **Config migration** -- a `niri-tools migrate-which-key` command to
   convert wlr-which-key YAML to the new KDL format. Low priority.

4. **Niri config watching** -- should the daemon watch niri's config for
   style changes? Can reuse the existing file watcher. Low priority.

## Full config example

```kdl
notifications "all"

ui {
    font "Pragmasevka Nerd Font 12"
    background-color "#2F2A4C"
    color "#DFD9FB"
    corner-radius 2

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

scratchpad "term" {
    key "t"
    desc "Terminal"
    app-id "com.mitchellh.ghostty"
    command "ghostty"
    size width="60%" height="60%"
    output "DP-2" { position x="50%" y="35%"; }
}

scratchpad "web-discord" {
    key "d"
    desc "Discord"
    app-id "chrome-magkoliahgffibhgfkmoealggombgknl-alt"
    command "xdg-launch" "chrome-magkoliahgffibhgfkmoealggombgknl-alt"
    auto-adopt true
    size width="35%" height="65%"
}

scratchpad "web-matrix" {
    key "e"
    desc "Matrix"
    app-id "chrome-bcngdmpegpihnheapppgoniglphkpfhm-alt"
    command "xdg-launch" "chrome-bcngdmpegpihnheapppgoniglphkpfhm-alt"
    auto-adopt true
    size width="50%" height="90%"
}

scratchpad "parsec" {
    app-id "parsecd"
    command "xdg-launch" "parsecd"
    size width="2560" height="1440"
}

mode "root" {
    binds {
        "`"     "Lock"          { spawn-sh "sleep 0.2 && dms ipc lock lock"; }
        Space   "Launcher"      { spawn-sh "rofi -show drun -modi drun"; }
        o       "Open"          { switch-mode "open"; }
        s       "Scratchpads"   { scratchpad-pick; }
        b       "Brightness"    { switch-mode "brightness"; }
        n       "Notifications" { switch-mode "notifications"; }
        S       "Screenshot"    { switch-mode "screenshot"; }
        z       "Z"             { switch-mode "z"; }
    }
}

mode "scratchpads" {
    binds {
        t "Terminal"       { scratchpad-toggle "term"; }
        d "Discord"        { scratchpad-toggle "web-discord"; }
        e "Matrix"         { scratchpad-toggle "web-matrix"; }
        "+" "Adopt"        { scratchpad-adopt; }
        "-" "Disown"       { scratchpad-disown; }
        y   "Toggle Float" { scratchpad-toggle-float; }
    }
}

mode "brightness" {
    keep-open
    binds {
        "?" "Query"    { alias "q"; spawn-sh "brightness -q"; }
        j   "-5"       { spawn-sh "brightness -5"; }
        k   "+5"       { spawn-sh "brightness +5"; }
        "Ctrl+j" "-1"  { spawn-sh "brightness -1"; }
        "Ctrl+k" "+1"  { spawn-sh "brightness +1"; }
        J   "-10"      { spawn-sh "brightness -10"; }
        K   "+10"      { spawn-sh "brightness +10"; }
        "1" "10"       { spawn-sh "brightness 10"; }
        "0" "100"      { spawn-sh "brightness 100"; }
    }
}

mode "resize" {
    binds {
        e   "Expand"  { expand-column-to-available-width; }
        "1" "10%"     { set-window-width "10%"; }
        "5" "50%"     { set-window-width "50%"; }
        "0" "100%"    { set-window-width "100%"; }
        h   "Height"  { switch-mode "resize-height"; }
    }
}

mode "resize-height" {
    binds {
        "1" "10%"    { set-window-height "10%"; }
        "5" "50%"    { set-window-height "50%"; }
        "0" "100%"   { set-window-height "100%"; }
        w   "Width"  { switch-mode "resize"; }
    }
}

mode "notifications" {
    binds {
        d "Dismiss"     { keep-open; spawn-sh "makoctl dismiss"; }
        D "Dismiss All" { spawn-sh "makoctl dismiss --all"; }
        p "Previous"    { keep-open; spawn-sh "makoctl restore"; }
        a "Action"      { spawn-sh "makoctl menu -- rofi -dmenu -p 'Choose Action: '"; }
    }
}

mode "workspace" {
    binds {
        n "Rename" {
            spawn-sh r#"
                name=$(rofi -dmenu -p "Workspace Name" -i -l 0)
                if [[ -n "$name" ]]; then
                    niri msg action set-workspace-name "$name"
                fi
            "#
        }
    }
}

mode "screenshot" {
    binds {
        Print "Window or Region" {
            alias "r"
            spawn-sh "$XDG_CONFIG_HOME/niri/bin/screenshot capture --still --pointer"
        }
        S "Pick" {
            alias "s"
            spawn-sh "$XDG_CONFIG_HOME/niri/bin/screenshot pick"
        }
    }
}
```
