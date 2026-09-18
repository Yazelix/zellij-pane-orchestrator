use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use yazelix_zellij_pane_orchestrator::active_tab_session_state::{
    build_active_tab_session_state_v2, ActiveTabReadState, ActiveTabSessionStateV2,
    SessionStatusExtensions, SessionTransientPane, SessionTransientPanes, SessionWorkspace,
};
use yazelix_zellij_pane_orchestrator::agent_focus_contract::{
    resolve_agent_focus_toggle, AgentFocusTogglePlan,
};
use yazelix_zellij_pane_orchestrator::editor_open_contract::{
    build_editor_change_directory_command, build_editor_command_sequence,
    normalize_open_file_targets, EditorCommandSequenceError,
};
use yazelix_zellij_pane_orchestrator::horizontal_focus_contract::{
    horizontal_role_for_pane, is_visible_popup_pane, resolve_horizontal_focus, HorizontalFocusPlan,
    HorizontalPaneSnapshot,
};
use yazelix_zellij_pane_orchestrator::layout_state_contract::{AgentState, SidebarState};
use yazelix_zellij_pane_orchestrator::pane_contract::{startup_picker_tab_id, FocusContextPolicy};
use yazelix_zellij_pane_orchestrator::sidebar_contract::{
    resolve_sidebar_hide, resolve_sidebar_visibility_toggle, sidebar_post_layout_focus_nudges,
    SidebarPostLayoutFocus, SidebarVisibilityAction,
};
use yazelix_zellij_pane_orchestrator::transient_pane_contract::{
    select_transient_pane, transient_pane_identity, TransientPaneKind, TransientPaneSnapshot,
};
use yazelix_zellij_pane_orchestrator::vertical_focus_contract::{
    resolve_vertical_focus, resolve_vertical_move, resolve_vertical_move_step, VerticalFocusPlan,
    VerticalMovePlan,
};
use yazelix_zellij_pane_orchestrator::workspace_popup_contract::{
    workspace_popup_destination_id, workspace_popup_payload,
};
use yazelix_zellij_pane_orchestrator::workspace_recovery_contract::recovered_workspace_root;
use zellij_tile::prelude::{
    apply_tiled_swap_layout, cli_pipe_output, close_pane_with_id, close_tab_with_id,
    focus_pane_with_id, get_pane_cwd, go_to_next_tab, go_to_previous_tab, move_focus,
    move_pane_with_pane_id_in_direction, open_command_pane, open_terminal, pipe_message_to_plugin,
    rename_pane_with_id, rename_tab, write_chars_to_pane_id, write_to_pane_id, CommandToRun,
    Direction, MessageToPlugin, PaneId, PipeMessage, PipeSource,
};

use crate::model::{ProjectedMove, Tab, Workspace, WorkspaceSource, AGENT_TITLE};
use crate::State;

pub(crate) use yazelix_zellij_pane_orchestrator::horizontal_focus_contract::HorizontalDirection;
pub(crate) use yazelix_zellij_pane_orchestrator::layout_state_contract::LayoutFamilyDirection;
pub(crate) use yazelix_zellij_pane_orchestrator::vertical_focus_contract::VerticalDirection;

const DENIED: &str = "permissions_denied";
const FOCUSED_AGENT: &str = "focused_agent";
const FOCUSED_EDITOR: &str = "focused_editor";
const INVALID: &str = "invalid_payload";
const MISSING: &str = "missing";
const NOT_READY: &str = "not_ready";
const OK: &str = "ok";
const UNSUPPORTED_EDITOR: &str = "unsupported_editor";
const UNKNOWN_LAYOUT: &str = "unknown_layout";
const COMMAND_DELAY: Duration = Duration::from_millis(35);

#[derive(Deserialize)]
struct OpenFileRequest {
    editor: String,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    file_paths: Vec<String>,
    working_dir: String,
}

#[derive(Deserialize)]
struct EditorCwdRequest {
    editor: String,
    working_dir: String,
}

#[derive(Deserialize)]
struct WorkspaceRetargetRequest {
    workspace_root: String,
    #[serde(default)]
    workspace_source: Option<WorkspaceSource>,
    cd_focused_pane: bool,
    editor: Option<String>,
}

#[derive(Serialize)]
struct WorkspaceRetargetResponse {
    status: &'static str,
    editor_status: &'static str,
}

#[derive(Deserialize)]
struct OpenTerminalRequest {
    cwd: String,
}

#[derive(Serialize)]
struct DebugState {
    permissions_granted: bool,
    active_tab_position: Option<usize>,
    active_swap_layout_name: Option<String>,
    workspace_root: Option<String>,
    workspace_root_source: Option<String>,
    editor_pane_id: Option<String>,
    sidebar_pane_id: Option<String>,
    agent_pane_id: Option<String>,
    sidebar_is_collapsed: Option<bool>,
    agent_is_collapsed: Option<bool>,
}

impl State {
    pub(crate) fn get_active_tab_session_state(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        match serde_json::to_string(&session_state(tab)) {
            Ok(payload) => self.respond(message, &payload),
            Err(_) => self.respond(message, INVALID),
        }
    }

    pub(crate) fn ready<'a>(&'a self, message: &PipeMessage) -> Option<&'a Tab> {
        if !self.permissions_granted {
            self.respond(message, DENIED);
            return None;
        }
        self.session.active().or_else(|| {
            self.respond(message, NOT_READY);
            None
        })
    }

    pub(crate) fn respond(&self, message: &PipeMessage, value: &str) {
        if let PipeSource::Cli(id) = &message.source {
            cli_pipe_output(id, value);
        }
    }

    pub(crate) fn move_horizontal_focus(
        &self,
        message: &PipeMessage,
        direction: HorizontalDirection,
    ) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let terminals = tab.terminal_panes();
        let sidebar = tab.sidebar().map(|pane| pane.id);
        let agent = tab.agent().map(|pane| pane.id);
        let panes = terminals
            .iter()
            .map(|pane| HorizontalPaneSnapshot {
                role: horizontal_role_for_pane(
                    &PaneId::Terminal(pane.id),
                    &pane.title,
                    sidebar.as_ref(),
                    agent.as_ref(),
                ),
                is_plugin: false,
                exited: false,
                is_focused: pane.is_focused,
                pane_x: pane.pane_x,
                pane_y: pane.pane_y,
                pane_columns: pane.pane_columns,
                pane_rows: pane.pane_rows,
            })
            .collect::<Vec<_>>();
        let layout = tab.layout();
        match resolve_horizontal_focus(
            &panes,
            direction,
            visible_popup(tab, self.managed_agent_command_marker.as_deref()),
            layout.is_some_and(|layout| layout.is_sidebar_closed()),
            layout.is_some_and(|layout| layout.agent_is_closed() == Some(true)),
        ) {
            HorizontalFocusPlan::FocusPane(index) => match terminals.get(index) {
                Some(pane) => {
                    focus_pane_with_id(PaneId::Terminal(pane.id), false, false);
                    self.respond(message, OK);
                }
                None => self.respond(message, MISSING),
            },
            HorizontalFocusPlan::PreviousTab => {
                go_to_previous_tab();
                self.respond(message, OK);
            }
            HorizontalFocusPlan::NextTab => {
                go_to_next_tab();
                self.respond(message, OK);
            }
            HorizontalFocusPlan::MissingFocusedPane => self.respond(message, MISSING),
        }
    }

    pub(crate) fn move_vertical_focus(&self, message: &PipeMessage, direction: VerticalDirection) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let terminals = tab.terminal_panes();
        let panes = tab.vertical_snapshots(&terminals);
        match resolve_vertical_focus(
            &panes,
            direction,
            tab.focus == FocusContextPolicy::Sidebar,
            visible_popup(tab, self.managed_agent_command_marker.as_deref()),
        ) {
            VerticalFocusPlan::FocusPane(index) => match terminals.get(index) {
                Some(pane) => {
                    focus_pane_with_id(PaneId::Terminal(pane.id), false, false);
                    self.respond(message, OK);
                }
                None => self.respond(message, MISSING),
            },
            VerticalFocusPlan::PreserveFocus => self.respond(message, OK),
            VerticalFocusPlan::MissingFocusedPane => self.respond(message, MISSING),
        }
    }

    pub(crate) fn move_vertical_pane(
        &mut self,
        message: &PipeMessage,
        direction: VerticalDirection,
    ) {
        let Some(tab_id) = self.ready(message).map(|tab| tab.id) else {
            return;
        };
        let marker = self.managed_agent_command_marker.clone();
        let result = match self
            .session
            .tab_mut(tab_id)
            .map(|tab| vertical_move(tab, direction, marker.as_deref()))
        {
            Some(VerticalMoveResult::Dispatch(pane, direction, repetitions)) => {
                dispatch_vertical_move(pane, direction, repetitions);
                OK
            }
            Some(VerticalMoveResult::Preserve) => OK,
            Some(VerticalMoveResult::Missing) | None => MISSING,
        };
        self.respond(message, result);
    }

    pub(crate) fn switch_layout_family(
        &self,
        message: &PipeMessage,
        direction: LayoutFamilyDirection,
    ) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let required_panes = if tab.sidebar().is_none() {
            2
        } else if tab.agent().is_some() {
            4
        } else {
            3
        };
        if tab.user_pane_count() < required_panes {
            self.respond(message, OK);
            return;
        }
        let Some(current) = tab.layout() else {
            self.respond(message, UNKNOWN_LAYOUT);
            return;
        };
        let target = current.with_next_family(direction);
        if target != current {
            apply_tiled_swap_layout(target.layout_name());
            if target.agent_state == AgentState::Open {
                self.move_agent_right(tab);
            }
        }
        self.respond(message, OK);
    }

    pub(crate) fn toggle_sidebar(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        if tab.sidebar().is_none() {
            self.respond(message, MISSING);
            return;
        }
        let Some(layout) = tab.layout() else {
            self.respond(message, UNKNOWN_LAYOUT);
            return;
        };
        let plan = resolve_sidebar_visibility_toggle(
            layout.is_sidebar_closed(),
            tab.focus,
            tab.editor().is_some(),
            tab.fallback_terminal().is_some(),
        );
        self.apply_sidebar(
            layout.with_sidebar_state(match plan.action {
                SidebarVisibilityAction::Open => SidebarState::Open,
                SidebarVisibilityAction::Close => SidebarState::Closed,
            }),
            plan.post_layout_focus,
        );
        self.respond(message, OK);
    }

    pub(crate) fn hide_sidebar(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        if tab.sidebar().is_none() {
            self.respond(message, MISSING);
            return;
        }
        let Some(layout) = tab.layout() else {
            self.respond(message, UNKNOWN_LAYOUT);
            return;
        };
        if let Some(focus) = resolve_sidebar_hide(
            layout.is_sidebar_closed(),
            tab.focus,
            tab.editor().is_some(),
            tab.fallback_terminal().is_some(),
        ) {
            self.apply_sidebar(layout.with_sidebar_state(SidebarState::Closed), focus);
        }
        self.respond(message, OK);
    }

    pub(crate) fn toggle_agent_sidebar(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let result = match tab.agent() {
            Some(agent) if tab.focused_terminal() == Some(agent.id) => self
                .set_agent_state(tab, AgentState::Closed)
                .map(|_| self.focus_non_agent(tab)),
            Some(agent) => self.open_existing_agent(tab, agent.id),
            None => self.create_agent(tab),
        };
        self.respond(message, result.map(|_| OK).unwrap_or_else(|error| error));
    }

    pub(crate) fn toggle_editor_right_sidebar_focus(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let agent = tab.agent();
        let plan = resolve_agent_focus_toggle(
            agent.is_some_and(|agent| tab.focused_terminal() == Some(agent.id)),
            agent.is_some(),
            tab.layout()
                .is_some_and(|layout| layout.agent_is_closed() == Some(true)),
            tab.editor().is_some(),
            tab.fallback_terminal().is_some(),
        );
        let response: Result<&str, &str> = match plan {
            AgentFocusTogglePlan::FocusEditor => tab
                .editor()
                .map(|pane| {
                    focus_pane_with_id(pane.id, false, false);
                    FOCUSED_EDITOR
                })
                .ok_or(MISSING),
            AgentFocusTogglePlan::FocusFallback => tab
                .fallback_terminal()
                .map(|pane| {
                    focus_pane_with_id(pane, false, false);
                    OK
                })
                .ok_or(MISSING),
            AgentFocusTogglePlan::FocusAgent => agent
                .map(|pane| {
                    focus_pane_with_id(pane.id, false, false);
                    FOCUSED_AGENT
                })
                .ok_or(MISSING),
            AgentFocusTogglePlan::OpenAndFocusAgent => agent
                .ok_or(MISSING)
                .and_then(|pane| self.open_existing_agent(tab, pane.id))
                .map(|_| FOCUSED_AGENT),
            AgentFocusTogglePlan::CreateAndFocusAgent => {
                self.create_agent(tab).map(|_| FOCUSED_AGENT)
            }
            AgentFocusTogglePlan::MissingTarget => Err(MISSING),
        };
        self.respond(message, response.unwrap_or_else(|error| error));
    }

    fn apply_sidebar(
        &self,
        target: yazelix_zellij_pane_orchestrator::layout_state_contract::LayoutVariant,
        focus: SidebarPostLayoutFocus,
    ) {
        apply_tiled_swap_layout(target.layout_name());
        for delay in sidebar_post_layout_focus_nudges(focus) {
            sleep(Duration::from_millis(*delay));
            move_focus(Direction::Right);
        }
    }

    fn set_agent_state(&self, tab: &Tab, state: AgentState) -> Result<(), &'static str> {
        let Some(current) = tab.layout() else {
            return Err(UNKNOWN_LAYOUT);
        };
        let target = current.with_agent_state(state);
        if target != current {
            apply_tiled_swap_layout(target.layout_name());
        }
        Ok(())
    }

    fn create_agent(&self, tab: &Tab) -> Result<(), &'static str> {
        let Some(config) = self.right_sidebar_command.as_ref() else {
            return Err(MISSING);
        };
        let pane = open_command_pane(
            CommandToRun {
                path: PathBuf::from(&config.command),
                args: config.args.clone(),
                cwd: None,
            },
            BTreeMap::new(),
        )
        .ok_or(MISSING)?;
        rename_pane_with_id(pane, AGENT_TITLE);
        self.open_existing_agent(tab, pane)
    }

    fn open_existing_agent(&self, tab: &Tab, pane: PaneId) -> Result<(), &'static str> {
        self.set_agent_state(tab, AgentState::Open)?;
        sleep(COMMAND_DELAY);
        move_pane_with_pane_id_in_direction(pane, Direction::Right);
        sleep(COMMAND_DELAY);
        focus_pane_with_id(pane, false, false);
        Ok(())
    }

    fn focus_non_agent(&self, tab: &Tab) {
        sleep(COMMAND_DELAY);
        if let Some(pane) = tab
            .editor()
            .map(|pane| pane.id)
            .or_else(|| tab.fallback_terminal())
        {
            focus_pane_with_id(pane, false, false);
        }
    }

    fn move_agent_right(&self, tab: &Tab) {
        if let Some(agent) = tab.agent() {
            sleep(COMMAND_DELAY);
            move_pane_with_pane_id_in_direction(agent.id, Direction::Right);
        }
    }
}

impl State {
    pub(crate) fn recover_workspaces(&mut self) {
        let candidates = self
            .session
            .tabs()
            .filter_map(|tab| {
                let workspace = tab.workspace.as_ref()?;
                if workspace.source != WorkspaceSource::Bootstrap {
                    return None;
                }
                Some((tab.id, workspace.root.clone(), tab.editor()?.id))
            })
            .collect::<Vec<_>>();
        for (tab_id, current_root, editor) in candidates {
            let Ok(editor_cwd) = get_pane_cwd(editor) else {
                continue;
            };
            let Some(root) = recovered_workspace_root(Path::new(&current_root), &editor_cwd) else {
                continue;
            };
            if let Some(tab) = self.session.tab_mut(tab_id) {
                tab.workspace = Some(Workspace {
                    root: root.display().to_string(),
                    source: WorkspaceSource::Explicit,
                });
            }
        }
    }

    pub(crate) fn open_file(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let Some(editor) = tab.editor() else {
            self.respond(message, MISSING);
            return;
        };
        let Some(request) = decode::<OpenFileRequest>(message) else {
            self.respond(message, INVALID);
            return;
        };
        let targets =
            normalize_open_file_targets(request.file_path.as_deref(), &request.file_paths);
        let sequence =
            match build_editor_command_sequence(&request.editor, &request.working_dir, &targets) {
                Ok(sequence) => sequence,
                Err(EditorCommandSequenceError::EmptyTargets) => {
                    self.respond(message, INVALID);
                    return;
                }
                Err(EditorCommandSequenceError::UnsupportedEditor) => {
                    self.respond(message, UNSUPPORTED_EDITOR);
                    return;
                }
            };
        focus_pane_with_id(editor.id, false, false);
        sleep(COMMAND_DELAY);
        send_editor_command(editor.id, &sequence.change_directory_command);
        for command in sequence.open_file_commands {
            send_editor_command(editor.id, &command);
        }
        self.respond(message, OK);
    }

    pub(crate) fn set_managed_editor_cwd(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let Some(editor) = tab.editor() else {
            self.respond(message, MISSING);
            return;
        };
        let Some(request) = decode::<EditorCwdRequest>(message) else {
            self.respond(message, INVALID);
            return;
        };
        let Some(command) =
            build_editor_change_directory_command(&request.editor, &request.working_dir)
        else {
            self.respond(message, UNSUPPORTED_EDITOR);
            return;
        };
        send_editor_command(editor.id, &command);
        self.respond(message, OK);
    }

    pub(crate) fn retarget_workspace(&mut self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let tab_id = tab.id;
        let tab_position = tab.position;
        let focused = tab.focused_terminal();
        let editor = tab.editor();
        let Some(request) = decode::<WorkspaceRetargetRequest>(message) else {
            self.respond(message, INVALID);
            return;
        };
        let root = request.workspace_root.trim();
        if root.is_empty() {
            self.respond(message, INVALID);
            return;
        }
        rename_tab(
            u32::try_from(tab_position + 1).expect("tab position should fit in u32"),
            tab_name(root),
        );
        if let Some(tab) = self.session.tab_mut(tab_id) {
            tab.workspace = Some(Workspace {
                root: root.to_string(),
                source: request
                    .workspace_source
                    .unwrap_or(WorkspaceSource::Explicit),
            });
        }
        if request.cd_focused_pane {
            let Some(pane) = focused else {
                self.respond(message, MISSING);
                return;
            };
            write_chars_to_pane_id(&shell_cd(root), pane);
            sleep(COMMAND_DELAY);
            write_to_pane_id(vec![13], pane);
        }
        let editor_status = request
            .editor
            .as_deref()
            .map(str::trim)
            .filter(|editor| !editor.is_empty())
            .map(|kind| {
                let Some(command) = build_editor_change_directory_command(kind, root) else {
                    return UNSUPPORTED_EDITOR;
                };
                let Some(editor) = editor else {
                    return MISSING;
                };
                send_editor_command(editor.id, &command);
                OK
            })
            .unwrap_or("skipped");
        match serde_json::to_string(&WorkspaceRetargetResponse {
            status: OK,
            editor_status,
        }) {
            Ok(response) => self.respond(message, &response),
            Err(_) => self.respond(message, INVALID),
        }
    }

    pub(crate) fn close_startup_picker_tab(&self, message: &PipeMessage) {
        let Some((_, tab_id)) = self.startup_picker(message) else {
            return;
        };
        self.respond(message, OK);
        if self
            .session
            .tab(tab_id)
            .is_none_or(|tab| tab.editor().is_none())
        {
            close_tab_with_id(tab_id as u64);
        }
    }

    pub(crate) fn complete_startup_picker_handoff(&self, message: &PipeMessage) {
        let Some((pane_id, tab_id)) = self.startup_picker(message) else {
            return;
        };
        if self
            .session
            .tab(tab_id)
            .is_none_or(|tab| tab.editor().is_none())
        {
            self.respond(message, MISSING);
            return;
        }
        self.respond(message, OK);
        close_pane_with_id(PaneId::Terminal(pane_id));
    }

    pub(crate) fn open_terminal_in_cwd(&self, message: &PipeMessage) {
        if self.ready(message).is_none() {
            return;
        }
        let Some(request) = decode::<OpenTerminalRequest>(message) else {
            self.respond(message, INVALID);
            return;
        };
        open_terminal(&request.cwd);
        self.respond(message, OK);
    }

    pub(crate) fn open_workspace_terminal(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let Some(workspace) = tab.workspace.as_ref() else {
            self.respond(message, MISSING);
            return;
        };
        open_terminal(&workspace.root);
        self.respond(message, OK);
    }

    pub(crate) fn toggle_workspace_popup(&self, message: &PipeMessage) {
        let Some(popup_id) = message.payload.as_deref() else {
            self.respond(message, INVALID);
            return;
        };
        let Some(tab) = self.ready(message) else {
            return;
        };
        let Some(workspace) = tab.workspace.as_ref() else {
            self.respond(message, MISSING);
            return;
        };
        let Some(payload) = workspace_popup_payload(popup_id, &workspace.root) else {
            self.respond(message, INVALID);
            return;
        };
        let Some(url) = self.popup_plugin_url.as_deref() else {
            self.respond(message, MISSING);
            return;
        };
        let Some(plugin_id) = workspace_popup_destination_id(
            url,
            self.session
                .panes()
                .into_iter()
                .map(|pane| (pane.id, pane.exited, pane.plugin_url.as_deref())),
        ) else {
            self.respond(message, MISSING);
            return;
        };
        pipe_message_to_plugin(
            MessageToPlugin::new("toggle")
                .with_destination_plugin_id(plugin_id)
                .with_payload(payload),
        );
        self.respond(message, OK);
    }

    pub(crate) fn maintainer_debug_editor_state(&self, message: &PipeMessage) {
        let tab = self.session.active();
        let workspace = tab.and_then(|tab| tab.workspace.as_ref());
        let layout = tab.and_then(Tab::layout);
        let state = DebugState {
            permissions_granted: self.permissions_granted,
            active_tab_position: tab.map(|tab| tab.position),
            active_swap_layout_name: tab.and_then(|tab| tab.swap_layout.clone()),
            workspace_root: workspace.map(|workspace| workspace.root.clone()),
            workspace_root_source: workspace.map(|workspace| {
                match workspace.source {
                    WorkspaceSource::Bootstrap => "bootstrap",
                    WorkspaceSource::Explicit => "explicit",
                }
                .to_string()
            }),
            editor_pane_id: pane_id(tab.and_then(Tab::editor).map(|pane| pane.id)),
            sidebar_pane_id: pane_id(tab.and_then(Tab::sidebar).map(|pane| pane.id)),
            agent_pane_id: pane_id(tab.and_then(Tab::agent).map(|pane| pane.id)),
            sidebar_is_collapsed: layout.map(|layout| layout.is_sidebar_closed()),
            agent_is_collapsed: layout.and_then(|layout| layout.agent_is_closed()),
        };
        match serde_json::to_string(&state) {
            Ok(payload) => self.respond(message, &payload),
            Err(_) => self.respond(message, INVALID),
        }
    }

    pub(crate) fn debug_write_literal(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let Some(editor) = tab.editor() else {
            self.respond(message, MISSING);
            return;
        };
        let Some(payload) = message.payload.as_deref() else {
            self.respond(message, INVALID);
            return;
        };
        focus_pane_with_id(editor.id, false, false);
        sleep(COMMAND_DELAY);
        write_chars_to_pane_id(payload, editor.id);
        self.respond(message, OK);
    }

    pub(crate) fn debug_send_escape(&self, message: &PipeMessage) {
        let Some(tab) = self.ready(message) else {
            return;
        };
        let Some(editor) = tab.editor() else {
            self.respond(message, MISSING);
            return;
        };
        focus_pane_with_id(editor.id, false, false);
        sleep(COMMAND_DELAY);
        write_to_pane_id(vec![27], editor.id);
        self.respond(message, OK);
    }

    fn startup_picker(&self, message: &PipeMessage) -> Option<(u32, usize)> {
        if !self.permissions_granted {
            self.respond(message, DENIED);
            return None;
        }
        let Some(pane_id) = message
            .payload
            .as_deref()
            .and_then(|payload| payload.trim().parse::<u32>().ok())
        else {
            self.respond(message, INVALID);
            return None;
        };
        let tab_id = startup_picker_tab_id(
            self.session.tabs().flat_map(|tab| {
                tab.terminals()
                    .map(move |pane| (tab.id, pane.id, pane.title.as_str()))
            }),
            pane_id,
        );
        match tab_id {
            Some(tab_id) => Some((pane_id, tab_id)),
            None => {
                self.respond(message, MISSING);
                None
            }
        }
    }
}

fn decode<T: for<'de> Deserialize<'de>>(message: &PipeMessage) -> Option<T> {
    serde_json::from_str(message.payload.as_deref()?).ok()
}

fn send_editor_command(pane: PaneId, command: &str) {
    write_to_pane_id(vec![27], pane);
    sleep(COMMAND_DELAY);
    write_chars_to_pane_id(command, pane);
    sleep(COMMAND_DELAY);
    write_to_pane_id(vec![13], pane);
    sleep(COMMAND_DELAY);
}

fn shell_cd(path: &str) -> String {
    format!(
        "cd \"{}\"",
        path.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "\\$")
            .replace('`', "\\`")
    )
}

pub(crate) fn tab_name(root: &str) -> String {
    let trimmed = root.trim_end_matches(std::path::MAIN_SEPARATOR);
    Path::new(if trimmed.is_empty() { root } else { trimmed })
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("unnamed")
        .to_string()
}

fn visible_popup(tab: &Tab, managed_agent_command_marker: Option<&str>) -> bool {
    tab.terminals().any(|pane| {
        is_visible_popup_pane(
            &pane.title,
            pane.terminal_command.as_deref(),
            managed_agent_command_marker,
            pane.is_floating,
            pane.is_suppressed,
            tab.floating_panes_visible,
        )
    })
}

enum VerticalMoveResult {
    Dispatch(PaneId, VerticalDirection, usize),
    Preserve,
    Missing,
}

fn vertical_move(
    tab: &mut Tab,
    direction: VerticalDirection,
    managed_agent_command_marker: Option<&str>,
) -> VerticalMoveResult {
    if let Some(projected) = tab.projected_move.as_mut() {
        let Some(position) = projected
            .order
            .iter()
            .position(|id| *id == projected.pane_id)
        else {
            return VerticalMoveResult::Missing;
        };
        let Some((target, native_direction, repetitions)) =
            resolve_vertical_move_step(position, projected.order.len(), direction)
        else {
            return VerticalMoveResult::Preserve;
        };
        let pane = projected.order.remove(position);
        projected.order.insert(target, pane);
        return VerticalMoveResult::Dispatch(projected.pane_id, native_direction, repetitions);
    }
    if visible_popup(tab, managed_agent_command_marker) {
        return VerticalMoveResult::Preserve;
    }

    let terminals = tab.terminal_panes();
    let panes = tab.vertical_snapshots(&terminals);
    let (pane_index, direction, repetitions, expected_pane_order) =
        match resolve_vertical_move(&panes, direction, false) {
            VerticalMovePlan::MovePane {
                pane_index,
                direction,
                repetitions,
                expected_pane_order,
            } => (pane_index, direction, repetitions, expected_pane_order),
            VerticalMovePlan::PreservePanes => return VerticalMoveResult::Preserve,
            VerticalMovePlan::MissingFocusedPane => return VerticalMoveResult::Missing,
        };
    let Some(target) = terminals.get(pane_index) else {
        return VerticalMoveResult::Missing;
    };
    let pane_id = PaneId::Terminal(target.id);
    let order = expected_pane_order
        .iter()
        .map(|index| terminals.get(*index).map(|pane| PaneId::Terminal(pane.id)))
        .collect::<Option<Vec<_>>>();
    let Some(order) = order else {
        return VerticalMoveResult::Missing;
    };
    tab.projected_move = Some(ProjectedMove { pane_id, order });
    VerticalMoveResult::Dispatch(pane_id, direction, repetitions)
}

fn dispatch_vertical_move(pane: PaneId, direction: VerticalDirection, repetitions: usize) {
    let direction = match direction {
        VerticalDirection::Up => Direction::Up,
        VerticalDirection::Down => Direction::Down,
    };
    for _ in 0..repetitions {
        move_pane_with_pane_id_in_direction(pane, direction);
    }
}

pub(crate) fn session_state(tab: &Tab) -> ActiveTabSessionStateV2 {
    let (explicit_workspace, bootstrap_workspace) = match tab.workspace.as_ref() {
        Some(workspace) => {
            let value = SessionWorkspace {
                root: workspace.root.clone(),
                source: match workspace.source {
                    WorkspaceSource::Bootstrap => "bootstrap",
                    WorkspaceSource::Explicit => "explicit",
                }
                .to_string(),
            };
            match workspace.source {
                WorkspaceSource::Bootstrap => (None, Some(value)),
                WorkspaceSource::Explicit => (Some(value), None),
            }
        }
        None => (None, None),
    };
    let layout = tab.layout();
    let read = ActiveTabReadState {
        active_swap_layout_name: tab.swap_layout.clone(),
        explicit_workspace,
        bootstrap_workspace,
        editor_pane_id: pane_id(tab.editor().map(|pane| pane.id)),
        sidebar_pane_id: pane_id(tab.sidebar().map(|pane| pane.id)),
        agent_pane_id: pane_id(tab.agent().map(|pane| pane.id)),
        sidebar_collapsed: layout.map(|layout| layout.is_sidebar_closed()),
        agent_collapsed: layout.and_then(|layout| layout.agent_is_closed()),
        focus_context: match tab.focus {
            FocusContextPolicy::Editor => "editor",
            FocusContextPolicy::Sidebar => "sidebar",
            FocusContextPolicy::Other => "other",
        }
        .to_string(),
        transient_panes: transient_panes(tab),
        extensions: SessionStatusExtensions::default(),
    };
    build_active_tab_session_state_v2(tab.position, read)
}

fn transient_panes(tab: &Tab) -> SessionTransientPanes {
    let panes = tab
        .terminals()
        .map(|pane| TransientPaneSnapshot {
            pane_id: PaneId::Terminal(pane.id),
            title: pane.title.as_str(),
            terminal_command: pane.terminal_command.as_deref(),
            is_plugin: false,
            exited: false,
            is_floating: pane.is_floating,
            is_focused: pane.is_focused,
        })
        .collect::<Vec<_>>();
    SessionTransientPanes {
        popup: transient_pane(&panes, TransientPaneKind::Popup),
        menu: transient_pane(&panes, TransientPaneKind::Menu),
    }
}

fn transient_pane(
    panes: &[TransientPaneSnapshot<'_, PaneId>],
    kind: TransientPaneKind,
) -> Option<SessionTransientPane> {
    let pane = select_transient_pane(panes, transient_pane_identity(kind))?;
    Some(SessionTransientPane {
        pane_id: pane_id(Some(pane.pane_id))?,
        is_focused: pane.is_focused,
    })
}

pub(crate) fn pane_id(id: Option<PaneId>) -> Option<String> {
    match id {
        Some(PaneId::Terminal(id)) => Some(format!("terminal:{id}")),
        Some(PaneId::Plugin(id)) => Some(format!("plugin:{id}")),
        None => None,
    }
}
