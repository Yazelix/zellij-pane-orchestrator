# Zellij Pane Orchestrator

Standalone Zellij plugin for pane orchestration. The plugin originated in Yazelix, but core pane behavior is usable without installing Yazelix.

The plugin requires Zellij with named tiled swap-layout selection, available
in upstream commit `81f56e1` and the earlier Nova Zellij `796a30c4` API baseline.

## Build

```bash
cargo test
cargo build --target wasm32-wasip1 --profile release
nix build .#yazelix_zellij_pane_orchestrator
```

The public artifact is:

```text
target/wasm32-wasip1/release/yazelix_zellij_pane_orchestrator.wasm
```

The Nix package artifact for Yazelix runtime integration is:

```text
share/yazelix_zellij_pane_orchestrator/yazelix_pane_orchestrator.wasm
```

## Minimal Zellij config

```kdl
plugins {
    yazelix-zellij-pane-orchestrator location="file:/absolute/path/to/yazelix_zellij_pane_orchestrator.wasm" {
        screen_saver_enabled false
    }
}

keybinds {
    normal {
        bind "Alt y" {
            MessagePlugin "yazelix-zellij-pane-orchestrator" {
                name "toggle_sidebar"
            }
        }
    }
}
```

`toggle_sidebar` applies the matching named tiled swap layout for a terminal or
tiled plugin pane named `sidebar`. Layout order does not affect selection, and a
visible floating pane remains visible and focused while the tiled layout changes.

`toggle_bottom_hints` hides or restores one tiled plugin pane named
`bottom_hints` in every supported tab of the current session. Layout providers
must supply
`single_open`, `single_closed`, `columns_open`, and `columns_closed`, each with
an otherwise matching `_no_hints` variant whose minimum pane count is one
lower. The hidden variants omit the hint pane; the visible variants place it
in a fixed full-width bottom row. Sidebar and content-layout commands preserve
the hint choice. New tabs inherit session visibility; fresh sessions use the
provider's visible default. Each client's active tab receives the matching
layout while preserving its focus and input mode. Background tabs receive
restoration geometry when activated. Native tab client identities elect one
handler for broadcast messages, including mirrored sessions. Native suppression
of the oldest hint pane is the session visibility source. Unknown layouts, absent or ambiguous hint
panes, and legacy agent-layout variants leave the session unchanged.
When a tab closes, the orchestrator closes that tab's known hint pane: native
Zellij preserves suppressed background panes by moving them into a surviving
tab. Other background panes retain that native behavior.
With a popup visible, native pane geometry settles the tiled hint layout without
repeated redraws and preserves work columns when tab info reports floating `BASE`.
CLI callers supply a nonempty payload (for example `-- toggle`); the final
empty CLI pipe message is ignored so one request toggles once.
Hiding captures native frame offsets and restores framing on those tiled panes
after native auto-layout runs. This prevents a work pane from retaining the
missing hint slot's borderless style, while leaving borderless work panes alone.

## Standalone pipe API

These commands are intended to work without Yazelix runtime paths:

- `move_focus_left_or_tab`
- `move_focus_right_or_tab`
- `move_focus_down`
- `move_focus_up`
- `move_pane_down`
- `move_pane_up`
- `next_family`
- `previous_family`
- `content_layout_target` (returns the exact next named swap layout to a CLI
  request, or applies it for a keybinding; entering columns requires two visible
  tiled work panes)
- `toggle_sidebar`
- `toggle_bottom_hints`
- `hide_sidebar`
- `get_active_tab_session_state`
- `open_terminal_in_cwd`
- `open_workspace_terminal`

Vertical pane moves are circular within the focused work-pane column. Requests
received before Zellij reports the prior move's pane order are applied in order.

Yazelix integration commands depend on Yazelix-managed editor/sidebar/workspace conventions:

- `open_file`
- `set_managed_editor_cwd`
- `retarget_workspace`
- `close_startup_picker_tab`
- `complete_startup_picker_handoff`
- `toggle_workspace_popup`
- `reload_runtime_config`

`retarget_workspace` accepts an optional `workspace_source` of `explicit` or
`bootstrap`; callers normally omit it, while coordinators can preserve the
previous provenance when rolling back a failed multi-step retarget.
`close_startup_picker_tab` accepts the terminal pane id from `ZELLIJ_PANE_ID`
and closes its stable tab only while that named picker has no same-tab editor.
`complete_startup_picker_handoff` closes that picker pane only after an editor
is visible in the same stable tab.
`toggle_workspace_popup` requires a configured `popup_plugin_url`, accepts a
popup id as its payload, and forwards that id with the active tab's canonical
workspace root to the loaded popup instance matching that URL.
The plugin does not track agent activity or decorate tab names. The v2 active-tab
response retains an empty `extensions.ai_pane_activity` list for wire compatibility.
`managed_agent_command_marker` identifies the agent popup for focus navigation,
including when its terminal title changes. Agent activity and usage belong to
the consuming runtime's chosen tools, not this pane plugin.
`quit_on_last_terminal_close true` exits the session when a terminal pane closes
and leaves only plugin panes; it is disabled by default.

Editor command-mode integration is Neovim-only. Helix buffer opens and cwd sync are owned by the Yazelix Helix action bridge; direct Helix `open_file`, `set_managed_editor_cwd`, or `retarget_workspace` editor requests are rejected instead of sending `:open` or `:cd` text into the terminal.

Debug commands are maintainer-only and not part of the ordinary standalone API:

- `maintainer_debug_editor_state`
- `debug_write_literal`
- `debug_send_escape`

## Standalone contract

Core behavior must not require `YAZELIX_RUNTIME_DIR`, `YAZELIX_SESSION_CONFIG_PATH`, `yzx_control`, or Yazelix-managed config paths. Yazelix consumes this plugin as a first-party integration, but those integration paths are extensions on top of the standalone Zellij plugin contract.
