#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneSnapshot<'a> {
    pub title: &'a str,
    pub is_focused: bool,
    pub is_suppressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusContextPolicy {
    Editor,
    Sidebar,
    Other,
}

pub fn startup_picker_tab_id<'a>(
    panes: impl IntoIterator<Item = (usize, u32, &'a str)>,
    pane_id: u32,
) -> Option<usize> {
    panes.into_iter().find_map(|(tab_id, id, title)| {
        (id == pane_id && title.trim() == "yazi_picker").then_some(tab_id)
    })
}

#[derive(Default)]
pub struct SessionExitState {
    enabled: bool,
    terminal_close_pending: bool,
}

impl SessionExitState {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    pub fn record_pane_closed(&mut self, is_terminal: bool) {
        self.terminal_close_pending |= self.enabled && is_terminal;
    }

    pub fn observe_pane_snapshot(&mut self, has_terminal_pane: bool) -> bool {
        std::mem::take(&mut self.terminal_close_pending) && !has_terminal_pane
    }
}

pub fn select_managed_pane_index<'a>(
    panes: impl IntoIterator<Item = (usize, PaneSnapshot<'a>)>,
    expected_title: &str,
) -> Option<usize> {
    let mut first = None;
    let mut first_visible = None;
    for (index, pane) in panes
        .into_iter()
        .filter(|(_, pane)| pane.title.trim() == expected_title)
    {
        first.get_or_insert(index);
        if pane.is_focused {
            return Some(index);
        }
        if !pane.is_suppressed {
            first_visible.get_or_insert(index);
        }
    }
    first_visible.or(first)
}

pub fn resolve_focus_context(
    focused_title: Option<&str>,
    previous_focus_context: FocusContextPolicy,
) -> FocusContextPolicy {
    match focused_title.map(str::trim) {
        Some("editor") => FocusContextPolicy::Editor,
        Some("sidebar") => FocusContextPolicy::Sidebar,
        Some(title) if title.starts_with("yzx_") => previous_focus_context,
        Some(_) | None => FocusContextPolicy::Other,
    }
}

// Test lane: maintainer
#[cfg(test)]
mod tests {
    use super::{
        resolve_focus_context, select_managed_pane_index, startup_picker_tab_id,
        FocusContextPolicy, PaneSnapshot, SessionExitState,
    };

    #[test]
    fn quits_only_after_a_terminal_closes_and_the_next_snapshot_has_no_terminal() {
        let mut state = SessionExitState::default();

        state.record_pane_closed(true);
        assert!(!state.observe_pane_snapshot(false));

        let mut state = SessionExitState::new(true);

        assert!(!state.observe_pane_snapshot(false));
        state.record_pane_closed(false);
        assert!(!state.observe_pane_snapshot(false));

        state.record_pane_closed(true);
        assert!(!state.observe_pane_snapshot(true));
        assert!(!state.observe_pane_snapshot(false));

        state.record_pane_closed(true);
        assert!(state.observe_pane_snapshot(false));
    }

    #[test]
    fn startup_picker_cleanup_targets_its_stable_tab_only() {
        let panes = [
            (41, 7, "editor"),
            (52, 8, "yazi_picker"),
            (63, 9, "sidebar"),
        ];

        assert_eq!(startup_picker_tab_id(panes, 8), Some(52));
        assert_eq!(startup_picker_tab_id(panes, 7), None);
        assert_eq!(startup_picker_tab_id(panes, 99), None);
    }

    // Defends: managed-pane lookup keys off the canonical pane titles instead of editor binary names.
    #[test]
    fn only_exact_editor_title_counts_as_managed_editor() {
        let panes = [
            PaneSnapshot {
                title: "hx",
                is_focused: true,
                is_suppressed: false,
            },
            PaneSnapshot {
                title: "editor",
                is_focused: false,
                is_suppressed: false,
            },
        ];

        assert_eq!(
            select_managed_pane_index(panes.iter().copied().enumerate(), "editor"),
            Some(1)
        );
        assert_eq!(
            select_managed_pane_index(panes.into_iter().enumerate(), "hx"),
            Some(0)
        );
    }

    // Defends: focused managed panes win over unfocused duplicates when multiple panes share the same managed title.
    #[test]
    fn focused_managed_editor_wins_when_multiple_editor_titled_panes_exist() {
        let panes = [
            PaneSnapshot {
                title: "editor",
                is_focused: false,
                is_suppressed: false,
            },
            PaneSnapshot {
                title: "editor",
                is_focused: true,
                is_suppressed: false,
            },
        ];

        assert_eq!(
            select_managed_pane_index(panes.into_iter().enumerate(), "editor"),
            Some(1)
        );
    }

    // Defends: yzx helper panes preserve the previous focus context instead of hijacking focus-policy state.
    #[test]
    fn yzx_helper_panes_preserve_previous_focus_context() {
        assert_eq!(
            resolve_focus_context(Some("yzx_menu"), FocusContextPolicy::Editor),
            FocusContextPolicy::Editor
        );
        assert_eq!(
            resolve_focus_context(Some("something_else"), FocusContextPolicy::Sidebar),
            FocusContextPolicy::Other
        );
    }
}
