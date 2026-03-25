# Session Handoff: Modes & Scratchpad Picker

**Created:** 2026-03-25
**Context usage at handoff:** ~20%
**Reason for handoff:** User requested fresh session for implementation (planning consumed extensive context through 6 design iterations)
**Session type:** Interactive brainstorming → planning

## Session Configuration (MUST complete if applicable)

- **Commit preference:** Yes (include commit steps in plan)
- **Question style:** Not set (was interactive brainstorming, not deep work)
- **Scope boundaries:** Implementing the modes + scratchpad picker feature for niri-tools
- **Planned workflow:** brainstorming → writing-plans → subagent-driven-development (or executing-plans)
- **Current position in workflow:** Planning complete. Ready for implementation.

## User Preferences Discovered (MUST complete)

- User is highly opinionated about config format design. Went through 6 iterations (v1-v6) before landing on final design. The config format in the spec is final and should not be changed without explicit user approval.
- User values niri config consistency. The KDL format closely mirrors niri's `binds { Key { action; } }` pattern.
- User prefers elegance over convenience. They rejected auto-population features (scratchpad-mode, scratchpads {} inside modes) in favor of explicit binds because the simpler abstraction was cleaner.
- User cares about the scratchpad picker being a separate UI paradigm from modes (not crammed into the mode system). This was a key design breakthrough.
- User wants Super/Mod to be ignored by the mode overlay when matching keys (eliminates the need for Mod4+ aliases).
- User specifically mentioned the `frizbee` crate for fuzzy matching in the scratchpad picker.
- For the scratchpad picker: bare typing = fuzzy search, Mod+shortcut = instant toggle. This was a specific user decision.
- User prefers `ui { }` as the top-level visual settings block (not `mode-overlay`), with `modes { }` and `scratchpads { }` sub-blocks.
- User wants `notifications "all"` instead of `settings { notify "all"; watch true }`. Config watching is always on (like niri).

## Task Overview (MUST complete)

**Original goal:** The user asked to read `docs/plans/which-key.md` and plan a deep work session to implement a wlr-which-key replacement integrated into niri-tools. The session focused on refining the config format and UX design before writing the implementation plan.

**Approach decided:** Two GTK4 layer-shell UIs managed by the niri-tools daemon:
1. **Mode overlay** -- a which-key-style horizontal bar for key→action dispatch
2. **Scratchpad picker** -- a fuzzy-searchable vertical list for browsing/toggling scratchpads with live state indicators

GTK4 owns the main thread. The existing tokio event loop moves to a background `OnceLock<Runtime>`. This follows the whisper-overlay pattern at `/home/boo/proj/whisper-overlay/worktree/main/`.

## Progress Summary (MUST complete)

### Completed
- [x] Explored current niri-tools daemon architecture
- [x] Explored wlr-which-key codebase (predecessor being replaced)
- [x] Explored whisper-overlay (GTK4+tokio reference)
- [x] Read niri config documentation (binds, layout, recent-windows, introduction)
- [x] Designed config format through 6 iterations (v1→v6)
- [x] Wrote design spec: `docs/specs/2026-03-25-modes-and-scratchpad-picker-design.md`
- [x] Spec passed review
- [x] Wrote implementation plan: `docs/plans/modes-and-scratchpad-picker.md`
- [x] Plan passed review (4 issues found and fixed)

### In Progress
Nothing in progress. All planning work is complete.

### Remaining
- [ ] Implement Phase 1: Protocol, Config & CLI (Tasks 1.1-1.5) -- pure data model, no GTK
- [ ] Implement Phase 2: GTK Foundation & Mode Overlay (Tasks 2.1-2.4) -- the riskiest phase
- [ ] Implement Phase 3: Scratchpad Picker (Tasks 3.1-3.2)
- [ ] Implement Phase 4: Style Inheritance (Task 4.1)
- [ ] Implement Phase 5: Polish (Tasks 5.1-5.4)

## Files Being Modified (MUST complete)

| File | Status | Notes |
|------|--------|-------|
| `docs/specs/2026-03-25-modes-and-scratchpad-picker-design.md` | Created | Complete design spec |
| `docs/plans/modes-and-scratchpad-picker.md` | Created | Complete implementation plan |
| `docs/plans/which-key.md` | Existing | Original design doc (predecessor to the spec, now superseded) |
| `crates/niri-tools-common/src/protocol.rs` | Planned | Add ModeShow/ModeHide/ModeToggle/ScratchpadPick commands |
| `crates/niri-tools-common/src/config.rs` | Planned | Split into config/ sub-modules, add mode/ui types |
| `crates/niri-tools-common/src/config_parser.rs` | Planned | Parse mode, ui, notifications KDL nodes |
| `crates/niri-tools-daemon/src/main.rs` | Planned | Major refactor: GTK main loop + tokio background |
| `crates/niri-tools-daemon/src/server.rs` | Planned | Handle new commands, bridge to GTK thread |
| `crates/niri-tools-daemon/src/state.rs` | Planned | Add mode_configs, ui_config |
| `crates/niri-tools-daemon/src/ui/mod.rs` | Planned | New: UiManager |
| `crates/niri-tools-daemon/src/ui/mode_overlay.rs` | Planned | New: mode overlay window |
| `crates/niri-tools-daemon/src/ui/scratchpad_picker.rs` | Planned | New: scratchpad picker |
| `crates/niri-tools-daemon/src/ui/css.rs` | Planned | New: CSS generation |
| `crates/niri-tools-daemon/src/mode.rs` | Planned | New: ModeState (mode stack, navigation) |
| `crates/niri-tools/src/main.rs` | Planned | Add Mode and ScratchpadPick CLI subcommands |
| `crates/niri-tools-daemon/Cargo.toml` | Planned | Add gtk4, gtk4-layer-shell, gdk4-wayland, frizbee |
| `flake.nix` | Planned | Add GTK4 build inputs |

## Key Technical Decisions (MUST complete)

1. **GTK main thread, tokio background:** GTK4 must own the main thread on Wayland. The current `#[tokio::main]` must be replaced with plain `fn main()` calling `app.run()`. The tokio runtime runs as `OnceLock<Runtime>` on background threads. — This follows the whisper-overlay pattern and is the only reliable approach for GTK4 on Wayland.

2. **Two separate UIs, not one:** The mode overlay (horizontal bar, key dispatch) and scratchpad picker (vertical list, fuzzy search, live state) are fundamentally different UX paradigms. The user explicitly rejected cramming scratchpads into the mode system. — Modes are for "I know what I want, press the key." The picker is for "show me what's available."

3. **Super/Mod ignored by mode overlay:** When the overlay has exclusive keyboard grab, it strips Super/Mod from incoming key events before matching. This eliminates the need for `Mod4+` aliases (the biggest source of config noise in wlr-which-key). Ctrl/Shift/Alt are still respected.

4. **Scratchpad picker: bare typing = fuzzy search, Mod+key = shortcut:** No ambiguity in key disambiguation. Normal typing always goes to the search buffer. Holding Mod and pressing a shortcut key fires instantly.

5. **`key`/`desc` on scratchpad definitions are picker metadata only:** They have no effect on mode binds. Mode binds are always explicit. This was a deliberate decision after rejecting auto-population approaches.

6. **Niri action pass-through:** Any unrecognized action name in a mode bind is forwarded to `niri msg action <name> <args>`. Future-proof -- new niri actions work without niri-tools updates.

7. **Config format uses `binds { }` wrapper inside modes:** Matches niri's `recent-windows { binds { } }` pattern. Provides clear separation between mode-level settings (like `keep-open`) and the binds themselves.

8. **Always watch config:** The old `settings { watch true }` is removed. Config is always watched, matching niri's behavior. The `notifications` level is a top-level node. Old `settings` block is kept for backward compatibility.

9. **`frizbee` crate for fuzzy matching** in the scratchpad picker. User specifically mentioned this.

## Context and Constraints (MUST complete)

- The existing codebase has pre-existing LSP errors in `scratchpad.rs` about `auto_adopt` vs `auto_match`. These are unrelated to this feature. Don't get distracted by them.
- The wlr-which-key project at `/home/boo/proj/wlr-which-key/worktree/b0o` has important reference code, especially the key-release-before-hide fix in `src/main.rs`.
- The whisper-overlay project at `/home/boo/proj/whisper-overlay/worktree/main` has the GTK4+tokio pattern to follow.
- The user's wlr-which-key config at `~/.config/wlr-which-key/config.yaml` shows real-world usage patterns (horizontal bar, keep-open brightness, Mod4+ aliases, etc.).
- Tests are inline `#[cfg(test)] mod tests`. No separate test files. Mock `NiriClient` and `Notifier` are defined in test modules.
- The existing test helper for config parsing writes KDL to a tempfile and calls `load_config`. The plan notes this -- use `load_from_str` helper pattern.

## Open Questions (SHOULD complete)

- [ ] **Conditional GTK:** Should the daemon always init GTK even if no modes are configured? The plan doesn't resolve this. Simplest approach: always init GTK. If someone only uses scratchpads, the GTK overhead (~200ms startup) is the tradeoff. Decide during Task 2.1.
- [ ] **GTK4 crate versions:** The plan specifies `gtk4 = "0.9"`, `gtk4-layer-shell = "0.4"`, `gdk4-wayland = "0.9"`. Verify these are compatible and available. Check crates.io during Task 2.1.
- [ ] **`scratchpad-adopt` / `scratchpad-disown`:** These are listed as "Not implemented" in AGENTS.md Python reference table. The plan includes them as mode bind actions. They may need to be implemented as part of this work or deferred.

## Test Status (MUST complete)

- **Tests passing:** Not verified in this session (no implementation was done)
- **New tests added:** None yet (all in the plan)
- **Tests still needed:** See plan Tasks 1.1-1.4 for config/protocol tests, Task 2.4 for ModeState tests, Task 5.4 for comprehensive tests
- **How to run tests:** `cargo test`

## Next Steps (MUST complete)

When resuming, the agent MUST:

1. Read the implementation plan at `docs/plans/modes-and-scratchpad-picker.md`
2. Read the design spec at `docs/specs/2026-03-25-modes-and-scratchpad-picker-design.md`
3. Start executing Phase 1, Task 1.1: Add IPC commands to protocol (`crates/niri-tools-common/src/protocol.rs`)
4. Follow the plan task-by-task using the `subagent-driven-development` or `executing-plans` skill
5. Each task has explicit steps with code, test commands, and expected output

## Related Files (SHOULD complete)

- **Design spec:** `docs/specs/2026-03-25-modes-and-scratchpad-picker-design.md`
- **Implementation plan:** `docs/plans/modes-and-scratchpad-picker.md`
- **Original design doc (superseded):** `docs/plans/which-key.md`
- **AGENTS.md:** Project overview, crate structure, conventions
- **Reference: wlr-which-key:** `/home/boo/proj/wlr-which-key/worktree/b0o`
- **Reference: whisper-overlay:** `/home/boo/proj/whisper-overlay/worktree/main`
- **User's wlr-which-key config:** `~/.config/wlr-which-key/config.yaml`
- **User's niri config:** `~/.config/niri/config.kdl`
- **User's niri-tools config:** `~/.config/niri/niri-tools.kdl`
