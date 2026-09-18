#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransientPaneSnapshot<'a, Id> {
    pub pane_id: Id,
    pub title: &'a str,
    pub is_floating: bool,
    pub is_focused: bool,
}

pub fn select_transient_pane<'a, 'pane, Id>(
    panes: &'a [TransientPaneSnapshot<'pane, Id>],
    pane_title: &str,
) -> Option<&'a TransientPaneSnapshot<'pane, Id>> {
    panes
        .iter()
        .filter(|pane| pane.is_floating && pane.title.trim() == pane_title)
        .max_by_key(|pane| pane.is_focused)
}

// Test lane: maintainer
#[cfg(test)]
mod tests {
    use super::{select_transient_pane, TransientPaneSnapshot};

    fn transient_pane(
        pane_id: i32,
        title: &str,
        is_focused: bool,
    ) -> TransientPaneSnapshot<'_, i32> {
        TransientPaneSnapshot {
            pane_id,
            title,
            is_floating: true,
            is_focused,
        }
    }

    // Defends: transient panes are discovered by their exact managed title.
    #[test]
    fn selects_transient_pane_by_title() {
        let panes = [transient_pane(7, "floating_picker", false)];
        assert_eq!(
            select_transient_pane(&panes, "floating_picker").map(|pane| pane.pane_id),
            Some(7)
        );
    }

    // Defends: focused transient panes win over unfocused duplicates during transient lookup.
    #[test]
    fn prefers_focused_transient_pane_when_duplicates_exist() {
        let panes = [
            transient_pane(1, "floating_menu", false),
            transient_pane(2, "floating_menu", true),
        ];

        assert_eq!(
            select_transient_pane(&panes, "floating_menu").map(|pane| pane.pane_id),
            Some(2)
        );
    }

    // Defends: transient lookup ignores non-floating or unrelated panes instead of matching by stale titles alone.
    #[test]
    fn ignores_non_floating_or_irrelevant_panes() {
        let panes = [
            TransientPaneSnapshot {
                pane_id: 1,
                title: "floating_picker",
                is_floating: false,
                is_focused: false,
            },
            transient_pane(2, "editor", true),
        ];

        assert!(select_transient_pane(&panes, "floating_picker").is_none());
    }
}
