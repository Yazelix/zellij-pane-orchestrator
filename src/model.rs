#![cfg_attr(test, allow(dead_code))]

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use yazelix_zellij_pane_orchestrator::horizontal_focus_contract::{
    horizontal_role_for_pane, HorizontalPaneRole,
};
use yazelix_zellij_pane_orchestrator::layout_state_contract::{
    is_base_layout_name, AgentState, LayoutVariant, SidebarState,
};
use yazelix_zellij_pane_orchestrator::pane_contract::{resolve_focus_context, FocusContextPolicy};
use yazelix_zellij_pane_orchestrator::pane_contract::{select_managed_pane_index, PaneSnapshot};
use yazelix_zellij_pane_orchestrator::sidebar_contract::is_managed_sidebar_plugin;
use yazelix_zellij_pane_orchestrator::tab_identity_contract::position_pane_identity_conflicts_with_cached_tabs;
use yazelix_zellij_pane_orchestrator::vertical_focus_contract::{
    vertical_work_pane_order, VerticalPaneSnapshot,
};
use zellij_tile::prelude::{PaneId, PaneInfo, PaneManifest, TabInfo};

pub(crate) const EDITOR_TITLE: &str = "editor";
pub(crate) const SIDEBAR_TITLE: &str = "sidebar";
pub(crate) const AGENT_TITLE: &str = "agent";

#[derive(Default)]
pub(crate) struct Session {
    tabs: HashMap<usize, Tab>,
    active_tab_id: Option<usize>,
    pending_manifest: Option<PaneManifest>,
    bootstrap_workspace: Workspace,
}

pub(crate) struct Tab {
    pub(crate) id: usize,
    pub(crate) position: usize,
    pub(crate) swap_layout: Option<String>,
    pub(crate) floating_panes_visible: bool,
    pub(crate) panes: Vec<PaneInfo>,
    pane_snapshot_received: bool,
    pub(crate) focus: FocusContextPolicy,
    pub(crate) workspace: Option<Workspace>,
    pub(crate) projected_move: Option<ProjectedMove>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Workspace {
    pub(crate) root: String,
    pub(crate) source: WorkspaceSource,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceSource {
    Bootstrap,
    Explicit,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            root: String::new(),
            source: WorkspaceSource::Bootstrap,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ManagedPane {
    pub(crate) id: PaneId,
    pub(crate) columns: usize,
}

pub(crate) struct ProjectedMove {
    pub(crate) pane_id: PaneId,
    pub(crate) order: Vec<PaneId>,
}

impl Session {
    pub(crate) fn with_bootstrap(root: String) -> Self {
        Self {
            bootstrap_workspace: Workspace {
                root,
                source: WorkspaceSource::Bootstrap,
            },
            ..Self::default()
        }
    }

    pub(crate) fn update_tabs(&mut self, infos: &[TabInfo]) -> bool {
        let mut previous = std::mem::take(&mut self.tabs);
        self.active_tab_id = None;

        for info in infos {
            let mut tab = previous
                .remove(&info.tab_id)
                .unwrap_or_else(|| Tab::new(info.tab_id));
            tab.position = info.position;
            tab.swap_layout = info.active_swap_layout_name.clone();
            tab.floating_panes_visible = info.are_floating_panes_visible;
            if info.active {
                self.active_tab_id = Some(info.tab_id);
            }
            self.tabs.insert(info.tab_id, tab);
        }
        let bootstrap = self.bootstrap_workspace.clone();
        if let Some(tab) = self.active_mut() {
            tab.workspace.get_or_insert(bootstrap);
        }

        if infos.is_empty() {
            self.pending_manifest = None;
            true
        } else {
            self.retry_join()
        }
    }

    pub(crate) fn update_panes(&mut self, manifest: PaneManifest) -> bool {
        self.pending_manifest = Some(manifest);
        self.retry_join()
    }

    pub(crate) fn retry_join(&mut self) -> bool {
        let Some(manifest) = self.pending_manifest.as_ref() else {
            return self.tabs.values().all(|tab| tab.pane_snapshot_received);
        };
        let tab_id_by_position = self
            .tabs
            .values()
            .map(|tab| (tab.position, tab.id))
            .collect::<HashMap<_, _>>();
        if manifest.panes.len() != tab_id_by_position.len()
            || !manifest
                .panes
                .keys()
                .all(|position| tab_id_by_position.contains_key(position))
        {
            return false;
        }

        let pane_ids_by_position = manifest
            .panes
            .iter()
            .map(|(position, panes)| (*position, terminal_ids(panes)))
            .collect::<HashMap<_, _>>();
        let cached_pane_ids_by_tab = self
            .tabs
            .iter()
            .map(|(id, tab)| (*id, terminal_ids(&tab.panes)))
            .collect::<HashMap<_, _>>();
        if position_pane_identity_conflicts_with_cached_tabs(
            &pane_ids_by_position,
            &tab_id_by_position,
            &cached_pane_ids_by_tab,
        ) {
            return false;
        }

        for (position, panes) in &manifest.panes {
            if let Some(tab) = tab_id_by_position
                .get(position)
                .and_then(|id| self.tabs.get_mut(id))
            {
                tab.replace_panes(panes.clone());
            }
        }
        self.pending_manifest = None;
        true
    }

    pub(crate) fn active(&self) -> Option<&Tab> {
        self.active_tab_id
            .and_then(|id| self.tabs.get(&id))
            .filter(|tab| tab.pane_snapshot_received)
    }

    pub(crate) fn active_mut(&mut self) -> Option<&mut Tab> {
        self.active_tab_id.and_then(|id| self.tabs.get_mut(&id))
    }

    pub(crate) fn tab(&self, id: usize) -> Option<&Tab> {
        self.tabs.get(&id)
    }

    pub(crate) fn tab_mut(&mut self, id: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(&id)
    }

    pub(crate) fn tabs(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.values()
    }

    pub(crate) fn panes(&self) -> Vec<&PaneInfo> {
        self.pending_manifest
            .as_ref()
            .map(|manifest| manifest.panes.values().flatten().collect())
            .unwrap_or_else(|| {
                self.tabs
                    .values()
                    .flat_map(|tab| tab.panes.iter())
                    .collect()
            })
    }
}

impl Tab {
    fn new(id: usize) -> Self {
        Self {
            id,
            position: 0,
            swap_layout: None,
            floating_panes_visible: false,
            panes: Vec::new(),
            pane_snapshot_received: false,
            focus: FocusContextPolicy::Other,
            workspace: None,
            projected_move: None,
        }
    }

    fn replace_panes(&mut self, panes: Vec<PaneInfo>) {
        let focused_title = panes.iter().find_map(|pane| {
            if !pane.is_focused {
                return None;
            }
            if pane.is_plugin {
                is_managed_sidebar_plugin(
                    pane.is_plugin,
                    pane.exited,
                    pane.is_floating,
                    &pane.title,
                )
                .then_some("sidebar")
            } else if !pane.exited {
                Some(pane.title.as_str())
            } else {
                None
            }
        });
        self.focus = resolve_focus_context(focused_title, self.focus);
        self.panes = panes;
        self.pane_snapshot_received = true;
        self.reconcile_projected_move();
    }

    pub(crate) fn terminals(&self) -> impl Iterator<Item = &PaneInfo> {
        self.panes
            .iter()
            .filter(|pane| !pane.is_plugin && !pane.exited)
    }

    pub(crate) fn managed(&self, title: &str) -> Option<ManagedPane> {
        let snapshots = self
            .panes
            .iter()
            .map(|pane| PaneSnapshot {
                title: &pane.title,
                is_plugin: pane.is_plugin,
                exited: pane.exited,
                is_focused: pane.is_focused,
                is_suppressed: pane.is_suppressed,
            })
            .collect::<Vec<_>>();
        let pane =
            select_managed_pane_index(&snapshots, title).and_then(|index| self.panes.get(index))?;
        Some(ManagedPane {
            id: PaneId::Terminal(pane.id),
            columns: pane.pane_columns,
        })
    }

    pub(crate) fn editor(&self) -> Option<ManagedPane> {
        self.managed(EDITOR_TITLE)
    }

    pub(crate) fn sidebar(&self) -> Option<ManagedPane> {
        self.managed(SIDEBAR_TITLE).or_else(|| {
            self.panes
                .iter()
                .find(|pane| {
                    is_managed_sidebar_plugin(
                        pane.is_plugin,
                        pane.exited,
                        pane.is_floating,
                        &pane.title,
                    )
                })
                .map(|pane| ManagedPane {
                    id: PaneId::Plugin(pane.id),
                    columns: pane.pane_columns,
                })
        })
    }

    pub(crate) fn agent(&self) -> Option<ManagedPane> {
        self.managed(AGENT_TITLE)
    }

    pub(crate) fn focused_terminal(&self) -> Option<PaneId> {
        self.terminals()
            .find(|pane| pane.is_focused)
            .map(|pane| PaneId::Terminal(pane.id))
    }

    pub(crate) fn fallback_terminal(&self) -> Option<PaneId> {
        self.editor().map(|pane| pane.id).or_else(|| {
            self.terminals()
                .find(|pane| !matches!(pane.title.trim(), SIDEBAR_TITLE | AGENT_TITLE))
                .map(|pane| PaneId::Terminal(pane.id))
        })
    }

    pub(crate) fn user_pane_count(&self) -> usize {
        self.terminals()
            .filter(|pane| !pane.is_floating && !pane.is_suppressed)
            .count()
    }

    pub(crate) fn zjstatus_plugin_id(&self) -> Option<u32> {
        self.panes
            .iter()
            .find(|pane| {
                pane.is_plugin
                    && !pane.exited
                    && pane.plugin_url.as_deref().is_some_and(|url| {
                        let url = url.trim();
                        url == "zjstatus.wasm" || url.ends_with("/zjstatus.wasm")
                    })
            })
            .map(|pane| pane.id)
    }

    pub(crate) fn layout(&self) -> Option<LayoutVariant> {
        self.swap_layout
            .as_deref()
            .and_then(LayoutVariant::from_layout_name)
            .or_else(|| {
                if !is_base_layout_name(self.swap_layout.as_deref()) {
                    return None;
                }
                let sidebar = self.sidebar()?;
                Some(LayoutVariant::new(
                    if sidebar.columns <= 2 {
                        SidebarState::Closed
                    } else {
                        SidebarState::Open
                    },
                    if self.agent().is_some() {
                        AgentState::Open
                    } else {
                        AgentState::Absent
                    },
                ))
            })
    }

    pub(crate) fn terminal_panes(&self) -> Vec<&PaneInfo> {
        self.terminals().collect()
    }

    pub(crate) fn vertical_snapshots(&self, terminals: &[&PaneInfo]) -> Vec<VerticalPaneSnapshot> {
        let sidebar = self.sidebar().map(|pane| pane.id);
        let agent = self.agent().map(|pane| pane.id);
        terminals
            .iter()
            .map(|pane| VerticalPaneSnapshot {
                is_work_pane: horizontal_role_for_pane(
                    &PaneId::Terminal(pane.id),
                    &pane.title,
                    sidebar.as_ref(),
                    agent.as_ref(),
                ) == HorizontalPaneRole::Other
                    && !pane.is_floating
                    && !pane.is_suppressed,
                is_focused: pane.is_focused,
                pane_x: pane.pane_x,
                pane_y: pane.pane_y,
                pane_columns: pane.pane_columns,
            })
            .collect()
    }

    fn reconcile_projected_move(&mut self) {
        let Some(projected) = self.projected_move.as_ref() else {
            return;
        };
        let terminals = self.terminal_panes();
        let Some(index) = terminals
            .iter()
            .position(|pane| PaneId::Terminal(pane.id) == projected.pane_id)
        else {
            self.projected_move = None;
            return;
        };
        let snapshots = self.vertical_snapshots(&terminals);
        let order = vertical_work_pane_order(&snapshots, index)
            .into_iter()
            .filter_map(|index| terminals.get(index))
            .map(|pane| PaneId::Terminal(pane.id))
            .collect::<Vec<_>>();
        let same_panes = order.len() == projected.order.len()
            && order.iter().all(|pane| projected.order.contains(pane));
        if !same_panes || order == projected.order {
            self.projected_move = None;
        }
    }
}

fn terminal_ids(panes: &[PaneInfo]) -> HashSet<u32> {
    panes
        .iter()
        .filter(|pane| !pane.is_plugin && !pane.exited)
        .map(|pane| pane.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Session;
    use std::collections::HashMap;
    use zellij_tile::prelude::{PaneInfo, PaneManifest, TabInfo};

    fn tabs(order: &[(usize, usize)]) -> Vec<TabInfo> {
        order
            .iter()
            .enumerate()
            .map(|(index, (position, id))| TabInfo {
                position: *position,
                tab_id: *id,
                active: index == 0,
                ..TabInfo::default()
            })
            .collect()
    }

    fn pane(id: u32) -> PaneInfo {
        PaneInfo {
            id,
            ..PaneInfo::default()
        }
    }

    fn manifest(entries: &[(usize, u32)]) -> PaneManifest {
        PaneManifest {
            panes: entries
                .iter()
                .map(|(position, id)| (*position, vec![pane(*id)]))
                .collect::<HashMap<_, _>>(),
        }
    }

    #[test]
    fn joins_panes_to_stable_tabs_and_rejects_a_stale_position_race() {
        let mut session = Session::with_bootstrap("/home/user".to_string());
        assert!(!session.update_tabs(&tabs(&[(0, 10), (1, 20)])));
        assert!(session.active().is_none());
        assert!(session.update_panes(manifest(&[(0, 1), (1, 2)])));
        assert_eq!(session.active().map(|tab| tab.id), Some(10));
        assert_eq!(
            session.tab(10).unwrap().workspace.as_ref().unwrap().root,
            "/home/user"
        );
        assert_eq!(session.tab(10).unwrap().panes[0].id, 1);
        assert_eq!(session.tab(20).unwrap().panes[0].id, 2);

        assert!(!session.update_panes(manifest(&[(0, 2), (1, 1)])));
        assert_eq!(session.tab(10).unwrap().panes[0].id, 1);
        assert_eq!(session.tab(20).unwrap().panes[0].id, 2);

        assert!(session.update_tabs(&tabs(&[(0, 20), (1, 10)])));
        assert_eq!(
            session.tab(20).unwrap().workspace.as_ref().unwrap().root,
            "/home/user"
        );
        assert_eq!(session.tab(10).unwrap().panes[0].id, 1);
        assert_eq!(session.tab(20).unwrap().panes[0].id, 2);
    }
}
