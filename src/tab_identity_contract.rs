use std::collections::{HashMap, HashSet};

pub fn position_pane_identity_conflicts_with_cached_tabs(
    current_pane_ids_by_position: &HashMap<usize, HashSet<u32>>,
    tab_id_by_position: &HashMap<usize, usize>,
    previous_pane_ids_by_tab: &HashMap<usize, HashSet<u32>>,
) -> bool {
    if previous_pane_ids_by_tab.len() < 2 {
        return false;
    }

    current_pane_ids_by_position
        .iter()
        .any(|(tab_position, current_pane_ids)| {
            if current_pane_ids.is_empty() {
                return false;
            }

            let Some(expected_tab_id) = tab_id_by_position.get(tab_position) else {
                return false;
            };
            let Some(expected_previous_pane_ids) = previous_pane_ids_by_tab.get(expected_tab_id)
            else {
                return false;
            };
            if expected_previous_pane_ids.is_empty()
                || !expected_previous_pane_ids.is_disjoint(current_pane_ids)
            {
                return false;
            }

            previous_pane_ids_by_tab
                .iter()
                .any(|(tab_id, previous_pane_ids)| {
                    tab_id != expected_tab_id && !previous_pane_ids.is_disjoint(current_pane_ids)
                })
        })
}

// Test lane: default
#[cfg(test)]
mod tests {
    use super::position_pane_identity_conflicts_with_cached_tabs;
    use std::collections::{HashMap, HashSet};

    // Regression: a PaneUpdate can arrive after a tab move but before the matching TabUpdate; in that window
    // the old position map must not reassign pane-local activity to the tab that moved into the old slot.
    #[test]
    fn detects_moved_tab_pane_identity_against_stale_position_map() {
        let previous_panes = HashMap::from([(10, HashSet::from([1])), (20, HashSet::from([2]))]);
        let stale_tab_id_by_position = HashMap::from([(0, 10), (1, 20)]);
        let moved_panes_by_position =
            HashMap::from([(0, HashSet::from([2])), (1, HashSet::from([1]))]);

        assert!(position_pane_identity_conflicts_with_cached_tabs(
            &moved_panes_by_position,
            &stale_tab_id_by_position,
            &previous_panes,
        ));
    }

    // Defends: ordinary pane additions within the same tab order still rebuild immediately.
    #[test]
    fn accepts_same_order_pane_identity_with_added_pane() {
        let previous_panes = HashMap::from([(10, HashSet::from([1])), (20, HashSet::from([2]))]);
        let tab_id_by_position = HashMap::from([(0, 10), (1, 20)]);
        let same_order_panes_by_position =
            HashMap::from([(0, HashSet::from([1, 3])), (1, HashSet::from([2]))]);

        assert!(!position_pane_identity_conflicts_with_cached_tabs(
            &same_order_panes_by_position,
            &tab_id_by_position,
            &previous_panes,
        ));
    }

    // Defends: replacing panes inside a tab is not treated as tab reordering unless the pane ids match another tab.
    #[test]
    fn accepts_replaced_pane_identity_without_cross_tab_match() {
        let previous_panes = HashMap::from([(10, HashSet::from([1])), (20, HashSet::from([2]))]);
        let tab_id_by_position = HashMap::from([(0, 10), (1, 20)]);
        let replaced_panes_by_position =
            HashMap::from([(0, HashSet::from([3])), (1, HashSet::from([2]))]);

        assert!(!position_pane_identity_conflicts_with_cached_tabs(
            &replaced_panes_by_position,
            &tab_id_by_position,
            &previous_panes,
        ));
    }
}
