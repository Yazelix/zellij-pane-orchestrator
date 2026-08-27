use crate::pane_contract::FocusContextPolicy;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarVisibilityAction {
    Open,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarPostLayoutFocus {
    Preserve,
    MoveRightToNonSidebar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SidebarVisibilityTogglePlan {
    pub action: SidebarVisibilityAction,
    pub post_layout_focus: SidebarPostLayoutFocus,
}

pub fn resolve_sidebar_visibility_toggle(
    sidebar_is_closed: bool,
    focus_context: FocusContextPolicy,
    has_editor: bool,
    has_focus_fallback: bool,
) -> SidebarVisibilityTogglePlan {
    if sidebar_is_closed {
        SidebarVisibilityTogglePlan {
            action: SidebarVisibilityAction::Open,
            post_layout_focus: SidebarPostLayoutFocus::Preserve,
        }
    } else if focus_context == FocusContextPolicy::Sidebar && (has_editor || has_focus_fallback) {
        SidebarVisibilityTogglePlan {
            action: SidebarVisibilityAction::Close,
            post_layout_focus: SidebarPostLayoutFocus::MoveRightToNonSidebar,
        }
    } else {
        SidebarVisibilityTogglePlan {
            action: SidebarVisibilityAction::Close,
            post_layout_focus: SidebarPostLayoutFocus::Preserve,
        }
    }
}

pub fn resolve_sidebar_hide(
    sidebar_is_closed: bool,
    focus_context: FocusContextPolicy,
    has_editor: bool,
    has_focus_fallback: bool,
) -> Option<SidebarPostLayoutFocus> {
    if sidebar_is_closed {
        return None;
    }

    if focus_context == FocusContextPolicy::Sidebar && (has_editor || has_focus_fallback) {
        Some(SidebarPostLayoutFocus::MoveRightToNonSidebar)
    } else {
        Some(SidebarPostLayoutFocus::Preserve)
    }
}

pub fn is_managed_sidebar_plugin(
    is_plugin: bool,
    exited: bool,
    is_floating: bool,
    title: &str,
) -> bool {
    if !is_plugin || exited || is_floating {
        return false;
    }
    title.trim() == "sidebar"
}

pub fn sidebar_post_layout_focus_nudges(
    post_layout_focus: SidebarPostLayoutFocus,
) -> &'static [u64] {
    const MOVE_RIGHT_TO_NON_SIDEBAR: [u64; 2] = [35, 105];

    match post_layout_focus {
        SidebarPostLayoutFocus::Preserve => &[],
        SidebarPostLayoutFocus::MoveRightToNonSidebar => &MOVE_RIGHT_TO_NON_SIDEBAR,
    }
}

// Test lane: maintainer
#[cfg(test)]
mod tests {
    use super::{
        is_managed_sidebar_plugin, resolve_sidebar_hide, resolve_sidebar_visibility_toggle,
        sidebar_post_layout_focus_nudges, SidebarPostLayoutFocus, SidebarVisibilityAction,
        SidebarVisibilityTogglePlan,
    };
    use crate::pane_contract::FocusContextPolicy;

    // Defends: opening the sidebar preserves the current focus context instead of forcing a focus jump.
    #[test]
    fn opening_sidebar_preserves_current_focus() {
        assert_eq!(
            resolve_sidebar_visibility_toggle(true, FocusContextPolicy::Editor, true, true),
            SidebarVisibilityTogglePlan {
                action: SidebarVisibilityAction::Open,
                post_layout_focus: SidebarPostLayoutFocus::Preserve
            }
        );
    }

    // Defends: closing a focused sidebar prefers the editor when that fallback exists.
    #[test]
    fn closing_focused_sidebar_prefers_editor_fallback() {
        assert_eq!(
            resolve_sidebar_visibility_toggle(false, FocusContextPolicy::Sidebar, true, true),
            SidebarVisibilityTogglePlan {
                action: SidebarVisibilityAction::Close,
                post_layout_focus: SidebarPostLayoutFocus::MoveRightToNonSidebar
            }
        );
    }

    // Defends: closing a focused sidebar falls back to a non-sidebar target when the editor is missing.
    #[test]
    fn closing_focused_sidebar_uses_non_sidebar_fallback_when_editor_missing() {
        assert_eq!(
            resolve_sidebar_visibility_toggle(false, FocusContextPolicy::Sidebar, false, true),
            SidebarVisibilityTogglePlan {
                action: SidebarVisibilityAction::Close,
                post_layout_focus: SidebarPostLayoutFocus::MoveRightToNonSidebar
            }
        );
    }

    // Regression: the programmatic hide path must move focus off the sidebar before a missing editor pane is opened.
    #[test]
    fn hide_focused_sidebar_uses_non_sidebar_fallback_when_editor_missing() {
        assert_eq!(
            resolve_sidebar_hide(false, FocusContextPolicy::Sidebar, false, true),
            Some(SidebarPostLayoutFocus::MoveRightToNonSidebar)
        );
    }

    // Defends: hiding an already hidden sidebar is a no-op and does not inject focus motion.
    #[test]
    fn hide_closed_sidebar_is_noop() {
        assert_eq!(
            resolve_sidebar_hide(true, FocusContextPolicy::Sidebar, false, true),
            None
        );
    }

    #[test]
    fn only_tiled_live_sidebar_role_is_the_managed_plugin_sidebar() {
        assert!(is_managed_sidebar_plugin(true, false, false, "sidebar"));
        assert!(!is_managed_sidebar_plugin(true, false, true, "sidebar"));
        assert!(!is_managed_sidebar_plugin(false, false, false, "sidebar"));
        assert!(!is_managed_sidebar_plugin(true, false, false, "radar"));
    }

    // Defends: closing a non-focused sidebar does not inject extra focus motion.
    #[test]
    fn closing_unfocused_sidebar_preserves_current_focus() {
        assert_eq!(
            resolve_sidebar_visibility_toggle(false, FocusContextPolicy::Editor, true, true),
            SidebarVisibilityTogglePlan {
                action: SidebarVisibilityAction::Close,
                post_layout_focus: SidebarPostLayoutFocus::Preserve
            }
        );
    }

    // Defends: the reusable sidebar contract owns the post-layout focus nudge sequence.
    #[test]
    fn post_layout_focus_nudges_are_contract_owned() {
        assert_eq!(
            sidebar_post_layout_focus_nudges(SidebarPostLayoutFocus::MoveRightToNonSidebar),
            [35, 105]
        );
        assert!(sidebar_post_layout_focus_nudges(SidebarPostLayoutFocus::Preserve).is_empty());
    }
}
