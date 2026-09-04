use std::thread::sleep;
use std::time::Duration;

use yazelix_zellij_pane_orchestrator::layout_state_contract::{
    is_base_layout_name, AgentState, LayoutFamily, LayoutFamilyDirection, LayoutVariant,
    SidebarState,
};
use yazelix_zellij_pane_orchestrator::sidebar_contract::{
    resolve_sidebar_hide, resolve_sidebar_visibility_toggle, sidebar_post_layout_focus_nudges,
    SidebarPostLayoutFocus, SidebarVisibilityAction,
};
use zellij_tile::prelude::*;

use crate::panes::ManagedTabPanes;
use crate::{State, RESULT_MISSING, RESULT_OK, RESULT_UNKNOWN_LAYOUT};

const CLOSED_BASE_SIDEBAR_COLUMNS: usize = 2;

impl State {
    pub(crate) fn switch_layout_family(
        &self,
        pipe_message: &PipeMessage,
        direction: LayoutFamilyDirection,
    ) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        if !self.can_switch_layout_family(active_tab_id) {
            self.respond(pipe_message, RESULT_OK);
            return;
        }

        let Some(layout_variant) = self.layout_variant_for_tab(active_tab_id) else {
            self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
            return;
        };

        let target_variant = layout_variant.with_next_family(direction);
        if target_variant == layout_variant {
            self.respond(pipe_message, RESULT_OK);
            return;
        }

        self.apply_layout_variant(target_variant);
        if target_variant.agent_state == AgentState::Open {
            self.move_agent_right_after_layout_settle(active_tab_id);
        }

        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn toggle_sidebar(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        if is_no_sidebar_mode(
            self.tab_pane_caches
                .managed_panes_by_tab
                .get(&active_tab_id),
        ) {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        }

        let Some(sidebar_is_closed) = self.sidebar_is_closed(active_tab_id) else {
            self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
            return;
        };

        let focus_context = self
            .tab_pane_caches
            .focus_context_by_tab
            .get(&active_tab_id)
            .copied()
            .unwrap_or(crate::panes::FocusContext::Other);
        let managed_tab_panes = self
            .tab_pane_caches
            .managed_panes_by_tab
            .get(&active_tab_id);
        let has_editor = managed_tab_panes.and_then(|tab| tab.editor).is_some();
        let has_focus_fallback = self
            .tab_pane_caches
            .fallback_terminal_pane_by_tab
            .contains_key(&active_tab_id);

        let plan = resolve_sidebar_visibility_toggle(
            sidebar_is_closed,
            focus_context,
            has_editor,
            has_focus_fallback,
        );

        self.set_sidebar_state(
            active_tab_id,
            match plan.action {
                SidebarVisibilityAction::Open => SidebarState::Open,
                SidebarVisibilityAction::Close => SidebarState::Closed,
            },
        );
        self.run_sidebar_post_layout_focus(plan.post_layout_focus);

        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn hide_sidebar(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        if is_no_sidebar_mode(
            self.tab_pane_caches
                .managed_panes_by_tab
                .get(&active_tab_id),
        ) {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        }

        let Some(sidebar_is_closed) = self.sidebar_is_closed(active_tab_id) else {
            self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
            return;
        };

        let focus_context = self
            .tab_pane_caches
            .focus_context_by_tab
            .get(&active_tab_id)
            .copied()
            .unwrap_or(crate::panes::FocusContext::Other);
        let managed_tab_panes = self
            .tab_pane_caches
            .managed_panes_by_tab
            .get(&active_tab_id);
        let has_editor = managed_tab_panes.and_then(|tab| tab.editor).is_some();
        let has_focus_fallback = self
            .tab_pane_caches
            .fallback_terminal_pane_by_tab
            .contains_key(&active_tab_id);

        if let Some(post_layout_focus) = resolve_sidebar_hide(
            sidebar_is_closed,
            focus_context,
            has_editor,
            has_focus_fallback,
        ) {
            self.set_sidebar_state(active_tab_id, SidebarState::Closed);
            self.run_sidebar_post_layout_focus(post_layout_focus);
        }

        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn get_active_layout_variant(&self, active_tab_id: usize) -> Option<LayoutVariant> {
        let active_swap_layout_name = self
            .active_swap_layout_name_by_tab
            .get(&active_tab_id)
            .cloned()
            .flatten();

        active_swap_layout_name
            .as_deref()
            .and_then(LayoutVariant::from_layout_name)
    }

    pub(crate) fn layout_variant_for_tab(&self, active_tab_id: usize) -> Option<LayoutVariant> {
        self.get_active_layout_variant(active_tab_id)
            .or_else(|| self.base_layout_variant(active_tab_id))
    }

    pub(crate) fn sidebar_is_closed(&self, active_tab_id: usize) -> Option<bool> {
        self.layout_variant_for_tab(active_tab_id)
            .map(|variant| variant.is_sidebar_closed())
    }

    pub(crate) fn agent_is_closed(&self, active_tab_id: usize) -> Option<bool> {
        self.layout_variant_for_tab(active_tab_id)
            .and_then(|variant| variant.agent_is_closed())
    }

    fn base_layout_sidebar_is_closed(&self, active_tab_id: usize) -> Option<bool> {
        if !self.active_layout_is_base(active_tab_id) {
            return None;
        }
        self.tab_pane_caches
            .managed_panes_by_tab
            .get(&active_tab_id)
            .and_then(|tab| tab.sidebar)
            .map(|sidebar| sidebar.pane_columns <= CLOSED_BASE_SIDEBAR_COLUMNS)
    }

    fn base_layout_variant(&self, active_tab_id: usize) -> Option<LayoutVariant> {
        if !self.active_layout_is_base(active_tab_id) {
            return None;
        }

        let sidebar_state = if self.base_layout_sidebar_is_closed(active_tab_id)? {
            SidebarState::Closed
        } else {
            SidebarState::Open
        };
        let agent_state = self
            .tab_pane_caches
            .managed_panes_by_tab
            .get(&active_tab_id)
            .and_then(|tab| tab.agent)
            .map(|_| AgentState::Open)
            .unwrap_or(AgentState::Absent);

        Some(LayoutVariant::new(
            LayoutFamily::Single,
            sidebar_state,
            agent_state,
        ))
    }

    fn active_layout_is_base(&self, active_tab_id: usize) -> bool {
        self.active_swap_layout_name_by_tab
            .get(&active_tab_id)
            .is_some_and(|layout| is_base_layout_name(layout.as_deref()))
    }

    fn can_switch_layout_family(&self, active_tab_id: usize) -> bool {
        let user_pane_count = self
            .tab_pane_caches
            .user_pane_count_by_tab
            .get(&active_tab_id)
            .copied()
            .unwrap_or(0);

        let managed_tab_panes = self
            .tab_pane_caches
            .managed_panes_by_tab
            .get(&active_tab_id);
        if is_no_sidebar_mode(managed_tab_panes) {
            user_pane_count >= 2
        } else if managed_tab_panes.and_then(|tab| tab.agent).is_some() {
            user_pane_count >= 4
        } else {
            user_pane_count >= 3
        }
    }

    fn apply_layout_variant(&self, target_variant: LayoutVariant) {
        apply_tiled_swap_layout(target_variant.layout_name());
    }

    pub(crate) fn set_agent_state(
        &self,
        active_tab_id: usize,
        agent_state: AgentState,
    ) -> Option<()> {
        let current_variant = self.layout_variant_for_tab(active_tab_id)?;
        let target_variant = current_variant.with_agent_state(agent_state);
        if target_variant != current_variant {
            self.apply_layout_variant(target_variant);
        }
        Some(())
    }

    fn set_sidebar_state(&self, active_tab_id: usize, sidebar_state: SidebarState) {
        if let Some(current_variant) = self.layout_variant_for_tab(active_tab_id) {
            let target_variant = current_variant.with_sidebar_state(sidebar_state);
            if target_variant == current_variant {
                return;
            }
            self.apply_layout_variant(target_variant);
        }
    }

    fn run_sidebar_post_layout_focus(&self, post_layout_focus: SidebarPostLayoutFocus) {
        for delay_ms in sidebar_post_layout_focus_nudges(post_layout_focus) {
            sleep(Duration::from_millis(*delay_ms));
            move_focus(Direction::Right);
        }
    }
}

fn is_no_sidebar_mode(managed_tab_panes: Option<&ManagedTabPanes>) -> bool {
    managed_tab_panes.and_then(|tab| tab.sidebar).is_none()
}
