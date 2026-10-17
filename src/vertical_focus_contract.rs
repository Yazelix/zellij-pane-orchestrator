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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneMoveDirection {
    Up,
    Down,
    Left,
    Right,
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
        .filter(|(_, pane)| focused.is_none_or(|current| share_column(current, pane)))
        .map(|(index, pane)| (index, pane.pane_y))
        .collect::<Vec<_>>();
    cycle.sort_by_key(|(_, pane_y)| *pane_y);
    cycle
}

fn share_column(left: &VerticalPaneSnapshot, right: &VerticalPaneSnapshot) -> bool {
    left.pane_x.max(right.pane_x)
        < left
            .pane_x
            .saturating_add(left.pane_columns)
            .min(right.pane_x.saturating_add(right.pane_columns))
}

/// Work-pane indices grouped into columns from left to right, each top to bottom.
pub fn work_pane_columns(panes: &[VerticalPaneSnapshot]) -> Vec<Vec<usize>> {
    let mut work = panes
        .iter()
        .enumerate()
        .filter(|(_, pane)| pane.is_work_pane)
        .collect::<Vec<_>>();
    work.sort_by_key(|(_, pane)| (pane.pane_x, pane.pane_y));
    let mut columns: Vec<(&VerticalPaneSnapshot, Vec<(usize, usize)>)> = Vec::new();
    for (index, pane) in work {
        match columns
            .iter_mut()
            .find(|(first, _)| share_column(first, pane))
        {
            Some((_, column)) => column.push((index, pane.pane_y)),
            None => columns.push((pane, vec![(index, pane.pane_y)])),
        }
    }
    columns
        .into_iter()
        .map(|(_, mut column)| {
            column.sort_by_key(|(_, pane_y)| *pane_y);
            column.into_iter().map(|(index, _)| index).collect()
        })
        .collect()
}

fn resolve_vertical_move_step(
    position: usize,
    pane_count: usize,
    direction: VerticalDirection,
) -> Option<(usize, VerticalDirection, usize)> {
    if pane_count < 2 || position >= pane_count {
        return None;
    }
    let target_position = match direction {
        VerticalDirection::Up => (position + pane_count - 1) % pane_count,
        VerticalDirection::Down => (position + 1) % pane_count,
    };
    let (native_direction, repetitions) = match direction {
        VerticalDirection::Up if position == 0 => (VerticalDirection::Down, pane_count - 1),
        VerticalDirection::Down if position + 1 == pane_count => {
            (VerticalDirection::Up, pane_count - 1)
        }
        direction => (direction, 1),
    };
    Some((target_position, native_direction, repetitions))
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

/// Applies one requested move to projected `columns` and returns the native
/// single-step moves that reproduce it; `None` means `pane` is not a work pane.
/// Zellij swaps a pane with the most recently active pane beside it, so a
/// cross-column move preserves pane order only beside a single-pane left column.
pub fn resolve_pane_move_step<T: Copy + PartialEq>(
    columns: &mut [Vec<T>],
    pane: T,
    direction: PaneMoveDirection,
) -> Option<Vec<(T, PaneMoveDirection)>> {
    let column = columns.iter().position(|column| column.contains(&pane))?;
    let position = columns[column].iter().position(|id| *id == pane)?;
    let moves = match direction {
        PaneMoveDirection::Up | PaneMoveDirection::Down => {
            let vertical = if direction == PaneMoveDirection::Up {
                VerticalDirection::Up
            } else {
                VerticalDirection::Down
            };
            let Some((target, native, repetitions)) =
                resolve_vertical_move_step(position, columns[column].len(), vertical)
            else {
                return Some(Vec::new());
            };
            columns[column].remove(position);
            columns[column].insert(target, pane);
            let native = match native {
                VerticalDirection::Up => PaneMoveDirection::Up,
                VerticalDirection::Down => PaneMoveDirection::Down,
            };
            vec![(pane, native); repetitions]
        }
        PaneMoveDirection::Left if column > 0 && columns[column - 1].len() == 1 => {
            let left = std::mem::replace(&mut columns[column - 1][0], pane);
            columns[column].remove(position);
            columns[column].insert(0, left);
            let mut moves = vec![(pane, PaneMoveDirection::Up); position];
            moves.push((pane, PaneMoveDirection::Left));
            moves
        }
        PaneMoveDirection::Right if columns[column].len() == 1 && column + 1 < columns.len() => {
            let right = std::mem::replace(&mut columns[column + 1][0], pane);
            columns[column][0] = right;
            vec![(right, PaneMoveDirection::Left)]
        }
        PaneMoveDirection::Left | PaneMoveDirection::Right => Vec::new(),
    };
    Some(moves)
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_pane_move_step, resolve_vertical_focus, work_pane_columns, PaneMoveDirection,
        VerticalDirection, VerticalFocusPlan, VerticalPaneSnapshot,
    };
    use PaneMoveDirection::{Down, Left, Right, Up};
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
        let stacked = [
            pane(false, false, 0, 0, 24),
            pane(true, true, 24, 1, 80),
            pane(true, false, 24, 18, 80),
            pane(true, false, 24, 19, 80),
            pane(false, false, 0, 20, 120),
        ];
        let mut columns = work_pane_columns(&stacked);
        assert_eq!(columns, vec![vec![1, 2, 3]]);
        assert_eq!(
            resolve_pane_move_step(&mut columns, 1, Up),
            Some(vec![(1, Down), (1, Down)])
        );
        assert_eq!(columns, vec![vec![2, 3, 1]]);
        // Consecutive moves apply to the projection before a pane update arrives.
        assert_eq!(
            resolve_pane_move_step(&mut columns, 1, Down),
            Some(vec![(1, Up), (1, Up)])
        );
        assert_eq!(columns, vec![vec![1, 2, 3]]);
        assert_eq!(
            resolve_pane_move_step(&mut columns, 2, Up),
            Some(vec![(2, Up)])
        );
        assert_eq!(columns, vec![vec![2, 1, 3]]);
        for direction in [Left, Right] {
            assert_eq!(
                resolve_pane_move_step(&mut columns, 2, direction),
                Some(vec![])
            );
        }
        assert_eq!(resolve_pane_move_step(&mut columns, 0, Up), None);
    }

    #[test]
    fn crosses_columns_preserving_the_order_of_other_panes() {
        let split = [
            pane(false, false, 0, 0, 24),
            pane(true, false, 24, 1, 40),
            pane(true, false, 64, 1, 40),
            pane(true, false, 64, 2, 40),
            pane(true, true, 64, 3, 40),
            pane(false, false, 104, 1, 16),
        ];
        let mut columns = work_pane_columns(&split);
        assert_eq!(columns, vec![vec![1], vec![2, 3, 4]]);
        assert_eq!(
            resolve_pane_move_step(&mut columns, 4, Left),
            Some(vec![(4, Up), (4, Up), (4, Left)])
        );
        assert_eq!(columns, vec![vec![4], vec![1, 2, 3]]);
        assert_eq!(resolve_pane_move_step(&mut columns, 4, Left), Some(vec![]));
        assert_eq!(
            resolve_pane_move_step(&mut columns, 4, Right),
            Some(vec![(1, Left)])
        );
        assert_eq!(columns, vec![vec![1], vec![4, 2, 3]]);
        assert_eq!(resolve_pane_move_step(&mut columns, 4, Right), Some(vec![]));

        // Zellij would swap with an arbitrary pane of a multi-pane left column.
        let mut columns = vec![vec![1, 2], vec![3]];
        assert_eq!(resolve_pane_move_step(&mut columns, 3, Left), Some(vec![]));
        assert_eq!(resolve_pane_move_step(&mut columns, 1, Right), Some(vec![]));
        assert_eq!(columns, vec![vec![1, 2], vec![3]]);
    }
}
