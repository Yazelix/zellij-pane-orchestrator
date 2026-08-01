use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::Deserialize;
use yazelix_zellij_pane_orchestrator::workspace_popup_contract::{
    workspace_popup_destination_id, workspace_popup_payload, workspace_popup_yazi_response,
    workspace_popup_yazi_tab_id,
};
use zellij_tile::prelude::*;

use crate::panes::pane_id_to_string;
use crate::{
    State, RESULT_DENIED, RESULT_INVALID_PAYLOAD, RESULT_MISSING, RESULT_NOT_READY, RESULT_OK,
};

const YAZI_POPUP_ID: &str = "yazi";
const POPUP_YAZI_READY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspacePopupYaziState {
    pane_id: String,
    yazi_id: String,
    cwd: String,
}

#[derive(Deserialize)]
struct WorkspacePopupYaziRegistration {
    pane_id: String,
    yazi_id: String,
    cwd: String,
}

pub(crate) struct PendingWorkspacePopupYazi {
    pipe_message: PipeMessage,
    deadline: Instant,
}

impl State {
    pub(crate) fn register_workspace_popup_yazi_state(&mut self, pipe_message: &PipeMessage) {
        if !self.permissions_granted {
            self.respond(pipe_message, RESULT_DENIED);
            return;
        }
        let Some(payload) = pipe_message.payload.as_deref() else {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        };
        let registration: WorkspacePopupYaziRegistration = match serde_json::from_str(payload) {
            Ok(registration) => registration,
            Err(_) => {
                self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
                return;
            }
        };
        let pane_id = registration.pane_id.trim().to_string();
        let yazi_id = registration.yazi_id.trim().to_string();
        let cwd = registration.cwd.trim().to_string();
        if pane_id.is_empty() || yazi_id.is_empty() || cwd.is_empty() {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        }
        let Some(tab_id) = self.workspace_popup_yazi_tab_id(&pane_id) else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        self.workspace_popup_yazi_state_by_tab.insert(
            tab_id,
            WorkspacePopupYaziState {
                pane_id,
                yazi_id,
                cwd,
            },
        );
        self.respond(pipe_message, RESULT_OK);
        self.complete_pending_workspace_popup_yazi(tab_id);
    }

    pub(crate) fn focus_workspace_popup_yazi(&mut self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };
        let Some((destination_plugin_id, payload)) =
            self.workspace_popup_request(active_tab_id, YAZI_POPUP_ID)
        else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        let registered = self
            .get_workspace_popup_yazi(active_tab_id)
            .map(|state| state.yazi_id.clone());
        if registered.is_none()
            && matches!(pipe_message.source, PipeSource::Cli(_))
            && self
                .pending_workspace_popup_yazi_by_tab
                .contains_key(&active_tab_id)
        {
            self.respond(pipe_message, RESULT_NOT_READY);
            return;
        }
        if registered.is_none() && matches!(pipe_message.source, PipeSource::Cli(_)) {
            self.pending_workspace_popup_yazi_by_tab.insert(
                active_tab_id,
                PendingWorkspacePopupYazi {
                    pipe_message: pipe_message.clone(),
                    deadline: Instant::now() + POPUP_YAZI_READY_TIMEOUT,
                },
            );
        }

        pipe_message_to_plugin(
            MessageToPlugin::new("ensure")
                .with_destination_plugin_id(destination_plugin_id)
                .with_payload(payload),
        );

        if let Some(yazi_id) = registered {
            self.respond_workspace_popup_yazi(pipe_message, &yazi_id);
        } else if !matches!(pipe_message.source, PipeSource::Cli(_)) {
            self.respond(pipe_message, RESULT_OK);
        }
        self.arm_next_timer();
    }

    pub(crate) fn reconcile_workspace_popup_yazi_state(&mut self) {
        let valid_panes = self.workspace_popup_yazi_panes();
        self.workspace_popup_yazi_state_by_tab
            .retain(|tab_id, state| valid_panes.contains(&(*tab_id, state.pane_id.clone())));
    }

    pub(crate) fn retain_workspace_popup_yazi_tabs(&mut self, current_tab_ids: &HashSet<usize>) {
        self.workspace_popup_yazi_state_by_tab
            .retain(|tab_id, _| current_tab_ids.contains(tab_id));
        let removed = self
            .pending_workspace_popup_yazi_by_tab
            .keys()
            .filter(|tab_id| !current_tab_ids.contains(tab_id))
            .copied()
            .collect::<Vec<_>>();
        for tab_id in removed {
            if let Some(pending) = self.pending_workspace_popup_yazi_by_tab.remove(&tab_id) {
                self.respond(&pending.pipe_message, RESULT_MISSING);
            }
        }
    }

    pub(crate) fn handle_workspace_popup_yazi_timer(&mut self) {
        let now = Instant::now();
        let expired = self
            .pending_workspace_popup_yazi_by_tab
            .iter()
            .filter(|(_, pending)| pending.deadline <= now)
            .map(|(tab_id, _)| *tab_id)
            .collect::<Vec<_>>();
        for tab_id in expired {
            if let Some(pending) = self.pending_workspace_popup_yazi_by_tab.remove(&tab_id) {
                self.respond(&pending.pipe_message, RESULT_NOT_READY);
            }
        }
    }

    pub(crate) fn workspace_popup_yazi_next_timeout(&self) -> Option<Instant> {
        self.pending_workspace_popup_yazi_by_tab
            .values()
            .map(|pending| pending.deadline)
            .min()
    }

    fn workspace_popup_request(&self, tab_id: usize, popup_id: &str) -> Option<(u32, String)> {
        let workspace = self
            .workspace_state_by_tab
            .get(&tab_id)
            .or(self.initial_workspace_state.as_ref())?;
        let payload = workspace_popup_payload(popup_id, &workspace.root)?;
        let plugin_url = self.popup_plugin_url.as_deref()?;
        let destination = self.last_pane_manifest.as_ref().and_then(|manifest| {
            workspace_popup_destination_id(
                plugin_url,
                manifest
                    .panes
                    .values()
                    .flatten()
                    .map(|pane| (pane.id, pane.exited, pane.plugin_url.as_deref())),
            )
        })?;
        Some((destination, payload))
    }

    fn workspace_popup_yazi_tab_id(&self, pane_id: &str) -> Option<usize> {
        let title = self.workspace_popup_yazi_pane_title.as_deref()?;
        workspace_popup_yazi_tab_id(
            title,
            pane_id,
            self.tab_pane_caches
                .terminal_panes_by_tab
                .iter()
                .flat_map(|(tab_id, panes)| {
                    panes.iter().filter_map(move |pane| {
                        pane_id_to_string(Some(pane.pane_id))
                            .map(|id| (*tab_id, id, pane.title.as_str(), pane.is_floating))
                    })
                }),
        )
    }

    fn workspace_popup_yazi_panes(&self) -> HashSet<(usize, String)> {
        let Some(title) = self.workspace_popup_yazi_pane_title.as_deref() else {
            return HashSet::new();
        };
        self.tab_pane_caches
            .terminal_panes_by_tab
            .iter()
            .flat_map(|(tab_id, panes)| {
                panes.iter().filter_map(move |pane| {
                    (pane.is_floating && pane.title.trim() == title)
                        .then(|| pane_id_to_string(Some(pane.pane_id)))
                        .flatten()
                        .map(|pane_id| (*tab_id, pane_id))
                })
            })
            .collect()
    }

    fn get_workspace_popup_yazi(&self, tab_id: usize) -> Option<&WorkspacePopupYaziState> {
        let state = self.workspace_popup_yazi_state_by_tab.get(&tab_id)?;
        (self.workspace_popup_yazi_tab_id(&state.pane_id) == Some(tab_id)).then_some(state)
    }

    fn complete_pending_workspace_popup_yazi(&mut self, tab_id: usize) {
        let Some(yazi_id) = self
            .get_workspace_popup_yazi(tab_id)
            .map(|state| state.yazi_id.clone())
        else {
            return;
        };
        if let Some(pending) = self.pending_workspace_popup_yazi_by_tab.remove(&tab_id) {
            self.respond_workspace_popup_yazi(&pending.pipe_message, &yazi_id);
        }
    }

    fn respond_workspace_popup_yazi(&self, pipe_message: &PipeMessage, yazi_id: &str) {
        match workspace_popup_yazi_response(yazi_id) {
            Some(response) => self.respond(pipe_message, &response),
            None => self.respond(pipe_message, RESULT_INVALID_PAYLOAD),
        }
    }
}
