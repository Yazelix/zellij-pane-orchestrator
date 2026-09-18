mod commands;
mod model;
mod services;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use model::Session;
use services::Services;
use yazelix_zellij_pane_orchestrator::right_sidebar_command_contract::RightSidebarCommandConfig;
use zellij_tile::prelude::*;

const RECONCILE_DELAY: Duration = Duration::from_millis(500);

fn event_subscriptions(screen_enabled: bool) -> Vec<EventType> {
    let mut events = vec![
        EventType::TabUpdate,
        EventType::PaneUpdate,
        EventType::PaneClosed,
        EventType::CommandPaneExited,
        EventType::PermissionRequestResult,
        EventType::Timer,
    ];
    if screen_enabled {
        events.push(EventType::InputReceived);
    }
    events
}

#[derive(Default)]
struct State {
    session: Session,
    reconcile_at: Option<Instant>,
    permissions_granted: bool,
    managed_agent_command_marker: Option<String>,
    right_sidebar_command: Option<RightSidebarCommandConfig>,
    popup_plugin_url: Option<String>,
    services: Services,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        set_selectable(false);
        let plugin_ids = get_plugin_ids();
        self.session = Session::with_bootstrap(plugin_ids.initial_cwd.display().to_string());
        self.managed_agent_command_marker = configuration
            .get("managed_agent_command_marker")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.right_sidebar_command =
            RightSidebarCommandConfig::from_plugin_configuration(&configuration);
        self.popup_plugin_url = configuration
            .get("popup_plugin_url")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.initialize_services(&configuration, &plugin_ids.initial_cwd);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::OpenTerminalsOrPlugins,
            PermissionType::RunCommands,
            PermissionType::WriteToStdin,
            PermissionType::ReadCliPipes,
            PermissionType::MessageAndLaunchOtherPlugins,
            PermissionType::ReadSessionEnvironmentVariables,
        ]);
        subscribe(&event_subscriptions(self.screen_enabled()));
        self.arm_timer();
    }

    fn update(&mut self, event: Event) -> bool {
        self.record_event(&event);
        match event {
            Event::TabUpdate(tabs) => {
                let joined = self.session.update_tabs(&tabs);
                self.join(joined);
            }
            Event::PaneUpdate(manifest) => {
                self.observe_manifest_for_exit(&manifest);
                let joined = self.session.update_panes(manifest);
                self.join(joined);
            }
            Event::PermissionRequestResult(status) => {
                self.permissions_granted = status == PermissionStatus::Granted;
            }
            Event::InputReceived => self.screen_input(),
            Event::PaneClosed(pane) => self.pane_closed(pane),
            Event::CommandPaneExited(id, _, _) => self.command_pane_exited(id),
            Event::Timer(_) => {
                self.handle_service_timer();
                self.retry_join();
            }
            _ => {}
        }
        self.refresh_status();
        self.arm_timer();
        false
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        self.record_pipe(&message.name);
        match message.name.as_str() {
            "get_active_tab_session_state" => self.get_active_tab_session_state(&message),
            "move_focus_left_or_tab" => {
                self.move_horizontal_focus(&message, commands::HorizontalDirection::Left)
            }
            "move_focus_right_or_tab" => {
                self.move_horizontal_focus(&message, commands::HorizontalDirection::Right)
            }
            "move_focus_down" => {
                self.move_vertical_focus(&message, commands::VerticalDirection::Down)
            }
            "move_focus_up" => self.move_vertical_focus(&message, commands::VerticalDirection::Up),
            "move_pane_down" => {
                self.move_vertical_pane(&message, commands::VerticalDirection::Down)
            }
            "move_pane_up" => self.move_vertical_pane(&message, commands::VerticalDirection::Up),
            "next_family" => {
                self.switch_layout_family(&message, commands::LayoutFamilyDirection::Next)
            }
            "previous_family" => {
                self.switch_layout_family(&message, commands::LayoutFamilyDirection::Previous)
            }
            "toggle_sidebar" => self.toggle_sidebar(&message),
            "hide_sidebar" => self.hide_sidebar(&message),
            "toggle_agent_sidebar" => self.toggle_agent_sidebar(&message),
            "toggle_editor_right_sidebar_focus" => self.toggle_editor_right_sidebar_focus(&message),
            "open_file" => self.open_file(&message),
            "set_managed_editor_cwd" => self.set_managed_editor_cwd(&message),
            "retarget_workspace" => self.retarget_workspace(&message),
            "close_startup_picker_tab" => self.close_startup_picker_tab(&message),
            "complete_startup_picker_handoff" => self.complete_startup_picker_handoff(&message),
            "open_terminal_in_cwd" => self.open_terminal_in_cwd(&message),
            "open_workspace_terminal" => self.open_workspace_terminal(&message),
            "toggle_workspace_popup" => self.toggle_workspace_popup(&message),
            "maintainer_debug_editor_state" => self.maintainer_debug_editor_state(&message),
            "debug_write_literal" => self.debug_write_literal(&message),
            "debug_send_escape" => self.debug_send_escape(&message),
            "reload_runtime_config" => self.reload_runtime_config(&message),
            _ => {}
        }
        false
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

impl State {
    fn join(&mut self, joined: bool) {
        if joined {
            self.reconcile_at = None;
            self.recover_workspaces();
        } else {
            self.reconcile_at = Some(Instant::now() + RECONCILE_DELAY);
        }
    }

    fn retry_join(&mut self) {
        let Some(deadline) = self.reconcile_at else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }
        self.reconcile_at = None;
        if self.session.retry_join() {
            self.recover_workspaces();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression: pinned Zellij replaces the screen-side background subscription set,
    // so enabling input events must resend every event the orchestrator still needs.
    #[test]
    fn screen_saver_subscription_keeps_core_events() {
        let events = event_subscriptions(true);

        assert!(event_subscriptions(false)
            .into_iter()
            .all(|event| events.contains(&event)));
        assert!(events.contains(&EventType::InputReceived));
    }
}
