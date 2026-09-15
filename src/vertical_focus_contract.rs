#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalDirection {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerticalFocusPlan {
    FocusPane(usize),
    PreserveFocus,
    MissingFocusedPane,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerticalMovePlan {
    MovePane {
        pane_index: usize,
        direction: VerticalDirection,
        repetitions: usize,
        expected_pane_order: Vec<usize>,
    },
    PreservePanes,
    MissingFocusedPane,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerticalPaneSnapshot {
    pub is_work_pane: bool,
    pub is_focused: bool,
    pub pane_x: usize,
    pub pane_y: usize,
    pub pane_columns: usize,
}

fn work_pane_cycle(
    panes: &[VerticalPaneSnapshot],
    focused: Option<&VerticalPaneSnapshot>,
) -> Vec<(usize, usize)> {
    let mut cycle = panes
        .iter()
        .enumerate()
        .filter(|(_, pane)| pane.is_work_pane)
        .filter(|(_, pane)| {
            focused.is_none_or(|current| {
                current.pane_x.max(pane.pane_x)
                    < current
                        .pane_x
                        .saturating_add(current.pane_columns)
                        .min(pane.pane_x.saturating_add(pane.pane_columns))
            })
        })
        .map(|(index, pane)| (index, pane.pane_y))
        .collect::<Vec<_>>();
    cycle.sort_by_key(|(_, pane_y)| *pane_y);
    cycle
}

pub fn vertical_work_pane_order(
    panes: &[VerticalPaneSnapshot],
    focused_index: usize,
) -> Vec<usize> {
    work_pane_cycle(panes, panes.get(focused_index))
        .into_iter()
        .map(|(index, _)| index)
        .collect()
}

pub fn resolve_vertical_focus(
    panes: &[VerticalPaneSnapshot],
    direction: VerticalDirection,
    sidebar_is_focused: bool,
    visible_popup_is_open: bool,
) -> VerticalFocusPlan {
    if visible_popup_is_open {
        return VerticalFocusPlan::PreserveFocus;
    }

    let focused = panes
        .iter()
        .enumerate()
        .find(|(_, pane)| pane.is_work_pane && pane.is_focused);
    if focused.is_none() && !sidebar_is_focused {
        return VerticalFocusPlan::MissingFocusedPane;
    }

    let cycle = work_pane_cycle(panes, focused.map(|(_, pane)| pane));
    let Some(target) = focused
        .and_then(|(focused_index, _)| cycle.iter().position(|(index, _)| *index == focused_index))
        .map(|position| match direction {
            VerticalDirection::Up => (position + cycle.len() - 1) % cycle.len(),
            VerticalDirection::Down => (position + 1) % cycle.len(),
        })
        .or_else(|| match direction {
            VerticalDirection::Up => cycle.len().checked_sub(1),
            VerticalDirection::Down => (!cycle.is_empty()).then_some(0),
        })
    else {
        return VerticalFocusPlan::MissingFocusedPane;
    };

    VerticalFocusPlan::FocusPane(cycle[target].0)
}

pub fn resolve_vertical_move(
    panes: &[VerticalPaneSnapshot],
    direction: VerticalDirection,
    visible_popup_is_open: bool,
) -> VerticalMovePlan {
    if visible_popup_is_open {
        return VerticalMovePlan::PreservePanes;
    }

    let Some((focused_index, focused)) = panes
        .iter()
        .enumerate()
        .find(|(_, pane)| pane.is_work_pane && pane.is_focused)
    else {
        return VerticalMovePlan::MissingFocusedPane;
    };
    let cycle = work_pane_cycle(panes, Some(focused));
    if cycle.len() < 2 {
        return VerticalMovePlan::PreservePanes;
    }
    let Some(position) = cycle.iter().position(|(index, _)| *index == focused_index) else {
        return VerticalMovePlan::MissingFocusedPane;
    };
    let target_position = match direction {
        VerticalDirection::Up => (position + cycle.len() - 1) % cycle.len(),
        VerticalDirection::Down => (position + 1) % cycle.len(),
    };
    let (native_direction, repetitions) = match direction {
        VerticalDirection::Up if position == 0 => (VerticalDirection::Down, cycle.len() - 1),
        VerticalDirection::Down if position + 1 == cycle.len() => {
            (VerticalDirection::Up, cycle.len() - 1)
        }
        direction => (direction, 1),
    };
    let mut expected_pane_order = cycle.iter().map(|(index, _)| *index).collect::<Vec<_>>();
    let focused_index = expected_pane_order.remove(position);
    expected_pane_order.insert(target_position, focused_index);

    VerticalMovePlan::MovePane {
        pane_index: focused_index,
        direction: native_direction,
        repetitions,
        expected_pane_order,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_vertical_focus, resolve_vertical_move, VerticalDirection, VerticalFocusPlan,
        VerticalMovePlan, VerticalPaneSnapshot,
    };
    fn pane(
        is_work_pane: bool,
        is_focused: bool,
        pane_x: usize,
        pane_y: usize,
        pane_columns: usize,
    ) -> VerticalPaneSnapshot {
        VerticalPaneSnapshot {
            is_work_pane,
            is_focused,
            pane_x,
            pane_y,
            pane_columns,
        }
    }

    #[test]
    fn cycles_only_the_work_pane_column_and_preserves_popups() {
        let top_focused = [
            pane(false, false, 0, 0, 24),
            pane(true, true, 24, 0, 80),
            pane(true, false, 24, 18, 80),
            pane(true, false, 24, 19, 80),
            pane(false, false, 104, 0, 16),
        ];
        assert_eq!(
            resolve_vertical_focus(&top_focused, VerticalDirection::Up, false, false),
            VerticalFocusPlan::FocusPane(3)
        );

        let bottom_focused = [
            pane(false, false, 0, 0, 24),
            pane(true, false, 24, 0, 80),
            pane(true, false, 24, 1, 80),
            pane(true, true, 24, 2, 80),
            pane(false, false, 104, 0, 16),
        ];
        assert_eq!(
            resolve_vertical_focus(&bottom_focused, VerticalDirection::Down, false, false),
            VerticalFocusPlan::FocusPane(1)
        );
        assert_eq!(
            resolve_vertical_focus(&bottom_focused, VerticalDirection::Up, false, false),
            VerticalFocusPlan::FocusPane(2)
        );

        let radar_focused = [
            pane(false, false, 0, 0, 24),
            pane(true, false, 24, 0, 80),
            pane(true, false, 24, 19, 80),
            pane(false, false, 104, 0, 16),
        ];
        assert_eq!(
            resolve_vertical_focus(&radar_focused, VerticalDirection::Up, true, false),
            VerticalFocusPlan::FocusPane(2)
        );
        assert_eq!(
            resolve_vertical_focus(&radar_focused, VerticalDirection::Down, true, false),
            VerticalFocusPlan::FocusPane(1)
        );
        assert_eq!(
            resolve_vertical_focus(&top_focused, VerticalDirection::Down, false, true),
            VerticalFocusPlan::PreserveFocus
        );
    }

    #[test]
    fn moves_work_panes_circularly_without_crossing_ui_panes() {
        let top_focused = [
            pane(false, false, 0, 0, 24),
            pane(true, true, 24, 1, 80),
            pane(true, false, 24, 18, 80),
            pane(true, false, 24, 19, 80),
            pane(false, false, 0, 20, 120),
        ];
        assert_eq!(
            resolve_vertical_move(&top_focused, VerticalDirection::Up, false),
            VerticalMovePlan::MovePane {
                pane_index: 1,
                direction: VerticalDirection::Down,
                repetitions: 2,
                expected_pane_order: vec![2, 3, 1],
            }
        );

        let bottom_focused = [
            pane(false, false, 0, 0, 24),
            pane(true, false, 24, 1, 80),
            pane(true, false, 24, 2, 80),
            pane(true, true, 24, 19, 80),
            pane(false, false, 0, 20, 120),
        ];
        assert_eq!(
            resolve_vertical_move(&bottom_focused, VerticalDirection::Down, false),
            VerticalMovePlan::MovePane {
                pane_index: 3,
                direction: VerticalDirection::Up,
                repetitions: 2,
                expected_pane_order: vec![3, 1, 2],
            }
        );
        assert_eq!(
            resolve_vertical_move(&bottom_focused, VerticalDirection::Up, false),
            VerticalMovePlan::MovePane {
                pane_index: 3,
                direction: VerticalDirection::Up,
                repetitions: 1,
                expected_pane_order: vec![1, 3, 2],
            }
        );
        assert_eq!(
            resolve_vertical_move(&bottom_focused, VerticalDirection::Up, true),
            VerticalMovePlan::PreservePanes
        );
    }
}
