# Zellij Pane Orchestrator

Standalone Zellij plugin for tab-local pane orchestration. The plugin originated in Yazelix, but core pane behavior is usable without installing Yazelix.

This revision targets Nova Zellij `796a30c4`, which exposes exact named tiled
swap-layout selection to plugins. Stock Zellij does not provide that operation.

## Build

```bash
cargo test --lib
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

## Standalone pipe API

These commands are intended to work without Yazelix runtime paths:

- `move_focus_left_or_tab`
- `move_focus_right_or_tab`
- `next_family`
- `previous_family`
- `toggle_sidebar`
- `hide_sidebar`
- `get_active_tab_session_state`
- `open_terminal_in_cwd`
- `open_workspace_terminal`

Yazelix integration commands depend on Yazelix-managed editor/sidebar/workspace conventions:

- `open_file`
- `set_managed_editor_cwd`
- `retarget_workspace`
- `toggle_workspace_popup`
- `reload_runtime_config`

`retarget_workspace` accepts an optional `workspace_source` of `explicit` or
`bootstrap`; callers normally omit it, while coordinators can preserve the
previous provenance when rolling back a failed multi-step retarget.
`toggle_workspace_popup` requires a configured `popup_plugin_url`, accepts a
popup id as its payload, and forwards that id with the active tab's canonical
workspace root to the loaded popup instance matching that URL.
The plugin does not track agent activity or decorate tab names. The v2 active-tab
response retains an empty `extensions.ai_pane_activity` list for wire compatibility.
`managed_agent_command_marker` identifies the agent popup for focus navigation,
including when its terminal title changes. Agent activity and usage belong to
the consuming runtime's chosen tools, not this pane plugin.

Editor command-mode integration is Neovim-only. Helix buffer opens and cwd sync are owned by the Yazelix Helix action bridge; direct Helix `open_file`, `set_managed_editor_cwd`, or `retarget_workspace` editor requests are rejected instead of sending `:open` or `:cd` text into the terminal.

Debug commands are maintainer-only and not part of the ordinary standalone API:

- `maintainer_debug_editor_state`
- `debug_write_literal`
- `debug_send_escape`

## Standalone contract

Core behavior must not require `YAZELIX_RUNTIME_DIR`, `YAZELIX_SESSION_CONFIG_PATH`, `yzx_control`, or Yazelix-managed config paths. Yazelix consumes this plugin as a first-party integration, but those integration paths are extensions on top of the standalone Zellij plugin contract.
