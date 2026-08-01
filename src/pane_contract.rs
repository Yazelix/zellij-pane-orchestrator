#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneSnapshot<'a> {
    pub title: &'a str,
    pub is_plugin: bool,
    pub exited: bool,
    pub is_focused: bool,
    pub is_suppressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusContextPolicy {
    Editor,
    Sidebar,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartRevealAction {
    ForwardToEditor,
    CloseYaziPopup,
    ToggleEditorSidebarFocus,
}

pub fn select_managed_pane_index(
    panes: &[PaneSnapshot<'_>],
    expected_title: &str,
) -> Option<usize> {
    let matching_indexes: Vec<usize> = panes
        .iter()
        .enumerate()
        .filter(|(_, pane)| !pane.is_plugin)
        .filter(|(_, pane)| !pane.exited)
        .filter(|(_, pane)| pane.title.trim() == expected_title)
        .map(|(index, _)| index)
        .collect();

    matching_indexes
        .iter()
        .copied()
        .find(|index| panes[*index].is_focused)
        .or_else(|| {
            matching_indexes
                .iter()
                .copied()
                .find(|index| !panes[*index].is_suppressed)
        })
        .or_else(|| matching_indexes.first().copied())
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

pub fn resolve_smart_reveal_action(
    focus_context: FocusContextPolicy,
    focused_pane_title: Option<&str>,
    focused_pane_is_floating: bool,
    focused_pane_is_suppressed: bool,
    floating_panes_visible: bool,
) -> SmartRevealAction {
    if focus_context == FocusContextPolicy::Editor {
        SmartRevealAction::ForwardToEditor
    } else if floating_panes_visible
        && focused_pane_is_floating
        && !focused_pane_is_suppressed
        && focused_pane_title.is_some_and(|title| title.trim() == "yazi_popup")
    {
        SmartRevealAction::CloseYaziPopup
    } else {
        SmartRevealAction::ToggleEditorSidebarFocus
    }
}

// Test lane: maintainer
#[cfg(test)]
mod tests {
    use super::{
        resolve_focus_context, resolve_smart_reveal_action, select_managed_pane_index,
        FocusContextPolicy, PaneSnapshot, SmartRevealAction,
    };

    // Defends: managed-pane lookup keys off the canonical pane titles instead of editor binary names.
    #[test]
    fn only_exact_editor_title_counts_as_managed_editor() {
        let panes = [
            PaneSnapshot {
                title: "hx",
                is_plugin: false,
                exited: false,
                is_focused: true,
                is_suppressed: false,
            },
            PaneSnapshot {
                title: "editor",
                is_plugin: false,
                exited: false,
                is_focused: false,
                is_suppressed: false,
            },
        ];

        assert_eq!(select_managed_pane_index(&panes, "editor"), Some(1));
        assert_eq!(select_managed_pane_index(&panes, "hx"), Some(0));
    }

    // Defends: focused managed panes win over unfocused duplicates when multiple panes share the same managed title.
    #[test]
    fn focused_managed_editor_wins_when_multiple_editor_titled_panes_exist() {
        let panes = [
            PaneSnapshot {
                title: "editor",
                is_plugin: false,
                exited: false,
                is_focused: false,
                is_suppressed: false,
            },
            PaneSnapshot {
                title: "editor",
                is_plugin: false,
                exited: false,
                is_focused: true,
                is_suppressed: false,
            },
        ];

        assert_eq!(select_managed_pane_index(&panes, "editor"), Some(1));
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

    // Defends: Alt-r hides only the visible focused Yazi popup while retaining the existing editor and sidebar routes.
    #[test]
    fn smart_reveal_routes_editor_yazi_popup_and_other_focus() {
        assert_eq!(
            resolve_smart_reveal_action(
                FocusContextPolicy::Editor,
                Some("editor"),
                false,
                false,
                false,
            ),
            SmartRevealAction::ForwardToEditor
        );
        assert_eq!(
            resolve_smart_reveal_action(
                FocusContextPolicy::Other,
                Some(" yazi_popup "),
                true,
                false,
                true,
            ),
            SmartRevealAction::CloseYaziPopup
        );
        for (title, is_suppressed, floating_panes_visible) in [
            ("git_popup", false, true),
            ("yazi_popup", true, true),
            ("yazi_popup", false, false),
            ("sidebar", false, false),
        ] {
            assert_eq!(
                resolve_smart_reveal_action(
                    FocusContextPolicy::Other,
                    Some(title),
                    true,
                    is_suppressed,
                    floating_panes_visible,
                ),
                SmartRevealAction::ToggleEditorSidebarFocus
            );
        }
    }
}
