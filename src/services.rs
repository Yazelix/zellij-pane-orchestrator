use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use yazelix_zellij_pane_orchestrator::orchestrator_heartbeat_contract::{
    build_orchestrator_heartbeat_payload, OrchestratorHeartbeatPayload,
};
use yazelix_zellij_pane_orchestrator::pane_contract::SessionExitState;
use yazelix_zellij_pane_orchestrator::runtime_config_contract::{
    decode_runtime_config_reload, RuntimeConfigReloadError,
};
use yazelix_zellij_pane_orchestrator::screen_saver_contract::{
    resolve_screen_saver_timer_plan, ScreenSaverConfig, ScreenSaverTimerPlan,
};
use yazelix_zellij_pane_orchestrator::status_bar_cache_contract::{
    resolve_status_bar_cache_runtime, StatusBarCacheRuntime,
};
use yazelix_zellij_pane_orchestrator::status_bar_workspace_pipe_contract::{
    workspace_pipe_protocol_payload, ZJSTATUS_WORKSPACE_PIPE_MESSAGE,
};
use yazelix_zellij_pane_orchestrator::timer_schedule_contract::next_timer_delay;
use zellij_tile::prelude::*;

use crate::commands::{session_state, tab_name};
use crate::model::WorkspaceSource;
use crate::State;

const HEARTBEAT_FIRST: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const SCREEN_TITLE: &str = "yzx_screen";

#[derive(Default)]
pub(crate) struct Services {
    runtime_dir: PathBuf,
    runtime_generation: String,
    screen: Screen,
    status: Status,
    heartbeat: Heartbeat,
    timer_armed_for: Option<Instant>,
    session_exit: SessionExitState,
}

#[derive(Default)]
struct Screen {
    config: ScreenSaverConfig,
    last_input: Option<Instant>,
    next_timeout: Option<Instant>,
    pane: Option<PaneId>,
    restore_floating_layer: bool,
}

#[derive(Default)]
struct Status {
    runtime: Option<StatusBarCacheRuntime>,
    last_payload: Option<String>,
    workspace_payload_by_plugin: HashMap<u32, String>,
}

#[derive(Default)]
struct Heartbeat {
    started_at: u64,
    next_flush: Option<Instant>,
    last_event_kind: Option<String>,
    last_event_at: Option<u64>,
    last_timer_at: Option<u64>,
    last_pipe_name: Option<String>,
    last_pipe_at: Option<u64>,
    last_status_write_at: Option<u64>,
}

impl State {
    pub(crate) fn initialize_services(
        &mut self,
        configuration: &BTreeMap<String, String>,
        initial_cwd: &Path,
    ) {
        self.services.runtime_dir = configuration
            .get("runtime_dir")
            .map(|value| PathBuf::from(value.trim()))
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| initial_cwd.to_path_buf());
        self.services.runtime_generation = configuration
            .get("runtime_config_generation")
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        self.services.screen.config = ScreenSaverConfig::from_plugin_configuration(configuration);
        self.services.session_exit = SessionExitState::new(
            configuration
                .get("quit_on_last_terminal_close")
                .is_some_and(|value| value.trim() == "true"),
        );
        if self.services.screen.config.enabled {
            self.services.screen.last_input = Some(Instant::now());
            self.schedule_screen_timeout(self.services.screen.config.idle_seconds);
        }
        self.services.heartbeat.started_at = unix_time();
        self.services.heartbeat.next_flush = Some(Instant::now() + HEARTBEAT_FIRST);
    }

    pub(crate) fn screen_enabled(&self) -> bool {
        self.services.screen.config.enabled
    }

    pub(crate) fn record_event(&mut self, event: &Event) {
        self.services.heartbeat.last_event_kind = Some(event_kind(event).to_string());
        self.services.heartbeat.last_event_at = Some(unix_time());
    }

    pub(crate) fn record_pipe(&mut self, name: &str) {
        self.services.heartbeat.last_pipe_name = Some(name.to_string());
        self.services.heartbeat.last_pipe_at = Some(unix_time());
    }

    pub(crate) fn observe_manifest_for_exit(&mut self, manifest: &PaneManifest) {
        let has_terminal = manifest
            .panes
            .values()
            .flatten()
            .any(|pane| !pane.is_plugin);
        if self
            .services
            .session_exit
            .observe_pane_snapshot(has_terminal)
            && self.permissions_granted
        {
            quit_zellij();
        }
    }

    pub(crate) fn pane_closed(&mut self, pane: PaneId) {
        self.services
            .session_exit
            .record_pane_closed(matches!(pane, PaneId::Terminal(_)));
        self.screen_pane_closed(pane);
    }

    pub(crate) fn command_pane_exited(&mut self, id: u32) {
        self.screen_pane_closed(PaneId::Terminal(id));
    }

    fn screen_pane_closed(&mut self, pane: PaneId) {
        if self.services.screen.pane == Some(pane) {
            self.services.screen.pane = None;
            self.services.screen.last_input = Some(Instant::now());
            self.restore_floating_layer();
            let idle = self.services.screen.config.idle_seconds;
            self.schedule_screen_timeout(idle);
        }
    }

    pub(crate) fn screen_input(&mut self) {
        if !self.services.screen.config.enabled {
            return;
        }
        self.services.screen.last_input = Some(Instant::now());
        let idle = self.services.screen.config.idle_seconds;
        self.schedule_screen_timeout(idle);
        if let Some(pane) = self.services.screen.pane.take() {
            close_pane_with_id(pane);
        }
        self.restore_floating_layer();
    }

    pub(crate) fn handle_service_timer(&mut self) {
        self.services.timer_armed_for = None;
        self.services.heartbeat.last_timer_at = Some(unix_time());
        self.handle_screen_timer();
        self.handle_heartbeat_timer();
    }

    pub(crate) fn arm_timer(&mut self) {
        let now = Instant::now();
        let Some((deadline, delay)) = next_timer_delay(
            now,
            [
                self.reconcile_at,
                self.services.screen.next_timeout,
                self.services.heartbeat.next_flush,
            ],
            self.services.timer_armed_for,
        ) else {
            return;
        };
        set_timeout(delay.as_secs_f64());
        self.services.timer_armed_for = Some(deadline);
    }

    pub(crate) fn refresh_status(&mut self) {
        if !self.permissions_granted {
            return;
        }
        let Some(tab) = self.session.active() else {
            return;
        };
        let plugin_id = tab.zjstatus_plugin_id();
        let workspace_payload = workspace_pipe_protocol_payload(
            tab.workspace
                .as_ref()
                .filter(|workspace| workspace.source == WorkspaceSource::Explicit)
                .map(|workspace| format!(" [{}]", tab_name(&workspace.root)))
                .unwrap_or_default()
                .as_str(),
        );
        let status_payload = match serde_json::to_string(&session_state(tab)) {
            Ok(payload) => payload,
            Err(_) => return,
        };
        let live_plugin_ids = self
            .session
            .tabs()
            .filter_map(|tab| tab.zjstatus_plugin_id())
            .collect::<HashSet<_>>();
        self.services
            .status
            .workspace_payload_by_plugin
            .retain(|id, _| live_plugin_ids.contains(id));
        if let Some(plugin_id) = plugin_id {
            let previous = self
                .services
                .status
                .workspace_payload_by_plugin
                .get(&plugin_id);
            if previous != Some(&workspace_payload) {
                pipe_message_to_plugin(
                    MessageToPlugin::new(ZJSTATUS_WORKSPACE_PIPE_MESSAGE)
                        .with_destination_plugin_id(plugin_id)
                        .with_payload(workspace_payload.clone()),
                );
                self.services
                    .status
                    .workspace_payload_by_plugin
                    .insert(plugin_id, workspace_payload);
            }
        }
        if self.services.status.last_payload.as_ref() == Some(&status_payload) {
            return;
        }
        let Some(runtime) = self.status_runtime() else {
            return;
        };
        let command = [
            runtime.yzx_control_path.as_str(),
            "zellij",
            "status-cache-write",
            "--path",
            runtime.cache_path.as_str(),
            "--payload",
            status_payload.as_str(),
        ];
        run_command_with_env_variables_and_cwd(&command, runtime.env, runtime.cwd, BTreeMap::new());
        self.services.status.last_payload = Some(status_payload);
        self.services.heartbeat.last_status_write_at = Some(unix_time());
    }

    pub(crate) fn reload_runtime_config(&mut self, message: &PipeMessage) {
        if !self.permissions_granted {
            self.respond(message, "permissions_denied");
            return;
        }
        if self.session.active().is_none() {
            self.respond(message, "not_ready");
            return;
        }
        match decode_runtime_config_reload(
            message.payload.as_deref(),
            &self.services.runtime_generation,
        ) {
            Ok(config) => {
                let was_enabled = self.services.screen.config.enabled;
                self.services.screen.config = config.screen_saver_config();
                if self.services.screen.config.enabled {
                    if !was_enabled {
                        subscribe(&crate::event_subscriptions(true));
                    }
                    self.services.screen.last_input = Some(Instant::now());
                    let idle = self.services.screen.config.idle_seconds;
                    self.schedule_screen_timeout(idle);
                } else {
                    self.services.screen.last_input = None;
                    self.services.screen.next_timeout = None;
                    if let Some(pane) = self.services.screen.pane.take() {
                        close_pane_with_id(pane);
                    }
                    self.restore_floating_layer();
                }
                self.respond(message, "ok");
                self.arm_timer();
            }
            Err(RuntimeConfigReloadError::InvalidPayload) => {
                self.respond(message, "invalid_payload")
            }
            Err(RuntimeConfigReloadError::UnsupportedVersion) => {
                self.respond(message, "version_mismatch")
            }
            Err(RuntimeConfigReloadError::StaleGeneration) => {
                self.respond(message, "stale_generation")
            }
        }
    }

    fn schedule_screen_timeout(&mut self, seconds: u64) {
        self.services.screen.next_timeout = self
            .services
            .screen
            .config
            .enabled
            .then(|| Instant::now() + Duration::from_secs(seconds));
    }

    fn handle_screen_timer(&mut self) {
        if !self.services.screen.config.enabled {
            return;
        }
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.services.screen.last_input.unwrap_or(now));
        match resolve_screen_saver_timer_plan(
            &self.services.screen.config,
            elapsed,
            self.services.screen.pane.is_some(),
        ) {
            ScreenSaverTimerPlan::Disabled => self.services.screen.next_timeout = None,
            ScreenSaverTimerPlan::Wait(delay) => {
                self.services.screen.next_timeout = Some(Instant::now() + delay)
            }
            ScreenSaverTimerPlan::Open { style } => {
                self.services.screen.next_timeout = None;
                self.open_screen(&style);
            }
        }
    }

    fn open_screen(&mut self, style: &str) {
        if !self.permissions_granted || self.services.screen.pane.is_some() {
            let idle = self.services.screen.config.idle_seconds;
            self.schedule_screen_timeout(idle);
            return;
        }
        let launcher = self
            .services
            .runtime_dir
            .join("shells")
            .join("posix")
            .join("yzx_cli.sh");
        if !launcher.exists() {
            let idle = self.services.screen.config.idle_seconds;
            self.schedule_screen_timeout(idle);
            return;
        }
        self.services.screen.restore_floating_layer = hide_floating_panes(None).unwrap_or(false);
        let cwd = self
            .session
            .active()
            .and_then(|tab| tab.workspace.as_ref())
            .map(|workspace| PathBuf::from(&workspace.root))
            .unwrap_or_else(|| self.services.runtime_dir.clone());
        let command = CommandToRun {
            path: launcher,
            args: vec!["screen".to_string(), style.to_string()],
            cwd: Some(cwd),
        };
        match open_command_pane(command, BTreeMap::new()) {
            Some(pane) => {
                rename_pane_with_id(pane, SCREEN_TITLE);
                focus_pane_with_id(pane, true, false);
                toggle_pane_id_fullscreen(pane);
                self.services.screen.pane = Some(pane);
            }
            None => {
                self.restore_floating_layer();
                let idle = self.services.screen.config.idle_seconds;
                self.schedule_screen_timeout(idle);
            }
        }
    }

    fn restore_floating_layer(&mut self) {
        if self.services.screen.restore_floating_layer {
            let _ = show_floating_panes(None);
            self.services.screen.restore_floating_layer = false;
        }
    }

    fn status_runtime(&mut self) -> Option<StatusBarCacheRuntime> {
        if self.services.status.runtime.is_none() {
            self.services.status.runtime =
                resolve_status_bar_cache_runtime(&get_session_environment_variables());
        }
        self.services.status.runtime.clone()
    }

    fn handle_heartbeat_timer(&mut self) {
        let Some(deadline) = self.services.heartbeat.next_flush else {
            self.services.heartbeat.next_flush = Some(Instant::now() + HEARTBEAT_FIRST);
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.flush_heartbeat();
        self.services.heartbeat.next_flush = Some(Instant::now() + HEARTBEAT_INTERVAL);
    }

    fn flush_heartbeat(&mut self) {
        if !self.permissions_granted {
            return;
        }
        let Some(runtime) = self.status_runtime() else {
            return;
        };
        let now = unix_time();
        let heartbeat = &self.services.heartbeat;
        let payload = build_orchestrator_heartbeat_payload(OrchestratorHeartbeatPayload {
            heartbeat_at_unix_seconds: now,
            started_at_unix_seconds: heartbeat.started_at,
            last_event_kind: heartbeat.last_event_kind.clone(),
            last_event_at_unix_seconds: heartbeat.last_event_at,
            last_timer_at_unix_seconds: heartbeat.last_timer_at,
            last_pipe_name: heartbeat.last_pipe_name.clone(),
            last_pipe_at_unix_seconds: heartbeat.last_pipe_at,
            last_status_cache_write_at_unix_seconds: heartbeat.last_status_write_at,
        })
        .to_string();
        let command = [
            runtime.yzx_control_path.as_str(),
            "zellij",
            "status-cache-heartbeat",
            "--path",
            runtime.cache_path.as_str(),
            "--payload",
            payload.as_str(),
        ];
        run_command_with_env_variables_and_cwd(&command, runtime.env, runtime.cwd, BTreeMap::new());
    }
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn event_kind(event: &Event) -> &'static str {
    match event {
        Event::TabUpdate(_) => "tab_update",
        Event::PaneUpdate(_) => "pane_update",
        Event::PermissionRequestResult(_) => "permission_request_result",
        Event::InputReceived => "input_received",
        Event::Timer(_) => "timer",
        Event::PaneClosed(_) => "pane_closed",
        Event::CommandPaneExited(_, _, _) => "command_pane_exited",
        _ => "other",
    }
}
