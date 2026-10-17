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
    /// A tiled pane, which native moves may swap or which may block them.
    pub is_tiled: bool,
    pub is_focused: bool,
    pub pane_x: usize,
    pub pane_y: usize,
    pub pane_columns: usize,
    pub pane_rows: usize,
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

/// Work-pane indices grouped into columns from left to right, each top to bottom,
/// as seen from the work pane at `focused_index`.
///
/// An empty column marks a boundary that Zellij's swaps could cross only by
/// involving another pane: the left column must be one pane flush against the
/// right column and spanning its height, and no other tiled pane may overlap
/// the two columns, where it could join a stack or block the climb before a
/// crossing. A pane spanning both columns cannot fit beside the left pane, so
/// it is a row such as a bar rather than a column member. Geometry where
/// overlapping panes do not form disjoint columns yields only the focused
/// pane's vertical cycle.
pub fn work_pane_columns(panes: &[VerticalPaneSnapshot], focused_index: usize) -> Vec<Vec<usize>> {
    let columns = disjoint_columns(panes);
    let disjoint = columns.iter().all(|column| {
        column.iter().all(|index| {
            panes
                .iter()
                .enumerate()
                .filter(|(_, pane)| pane.is_work_pane)
                .all(|(other, pane)| share_column(&panes[*index], pane) == column.contains(&other))
        })
    });
    if !disjoint {
        return vec![work_pane_cycle(panes, panes.get(focused_index))
            .into_iter()
            .map(|(index, _)| index)
            .collect()];
    }
    let mut result: Vec<Vec<usize>> = Vec::new();
    for column in columns {
        if let Some(left) = result.last() {
            if !crossable(panes, left, &column) {
                result.push(Vec::new());
            }
        }
        result.push(column);
    }
    result
}

fn crossable(panes: &[VerticalPaneSnapshot], left: &[usize], right: &[usize]) -> bool {
    let [left] = left else {
        return false;
    };
    let left = &panes[*left];
    let (Some(top), Some(bottom), Some(edge)) = (
        right.iter().map(|index| panes[*index].pane_y).min(),
        right
            .iter()
            .map(|index| panes[*index].pane_y + panes[*index].pane_rows)
            .max(),
        right
            .iter()
            .map(|index| panes[*index].pane_x + panes[*index].pane_columns)
            .max(),
    ) else {
        return false;
    };
    right
        .iter()
        .all(|index| left.pane_x + left.pane_columns == panes[*index].pane_x)
        && left.pane_y <= top
        && left.pane_y + left.pane_rows >= bottom
        && !panes.iter().any(|pane| {
            let end = pane.pane_x + pane.pane_columns;
            pane.is_tiled
                && !pane.is_work_pane
                && pane.pane_x < edge
                && end > left.pane_x
                && !(pane.pane_x <= left.pane_x && end >= edge)
        })
}

fn disjoint_columns(panes: &[VerticalPaneSnapshot]) -> Vec<Vec<usize>> {
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

/// Plans one requested move from the last projected state in `states` and
/// appends the expected columns after each native single-step move; `None`
/// means `pane` is not a work pane. Zellij swaps a pane with the most recently
/// active pane beside it, so a pane crosses only beside a single-pane left
/// column, climbing to the top of its own column first.
pub fn project_pane_move<T: Copy + PartialEq>(
    states: &mut Vec<Vec<Vec<T>>>,
    pane: T,
    direction: PaneMoveDirection,
) -> Option<Vec<(T, PaneMoveDirection)>> {
    let columns = states.last()?;
    let column = columns.iter().position(|column| column.contains(&pane))?;
    let position = columns[column].iter().position(|id| *id == pane)?;
    let count = columns[column].len();
    let moves = match direction {
        // Wrapping travels the other way through the whole column.
        PaneMoveDirection::Up if count > 1 && position == 0 => {
            vec![(pane, PaneMoveDirection::Down); count - 1]
        }
        PaneMoveDirection::Down if count > 1 && position + 1 == count => {
            vec![(pane, PaneMoveDirection::Up); count - 1]
        }
        PaneMoveDirection::Up | PaneMoveDirection::Down if count > 1 => vec![(pane, direction)],
        PaneMoveDirection::Left if column > 0 && columns[column - 1].len() == 1 => {
            let mut moves = vec![(pane, PaneMoveDirection::Up); position];
            moves.push((pane, PaneMoveDirection::Left));
            moves
        }
        PaneMoveDirection::Right if count == 1 => match columns.get(column + 1) {
            Some(right) if !right.is_empty() => vec![(right[0], PaneMoveDirection::Left)],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    for (moved, direction) in &moves {
        let mut next = states.last()?.clone();
        apply_native_move(&mut next, *moved, *direction);
        states.push(next);
    }
    Some(moves)
}

/// Drops projected states that an observed update has passed. Returns whether
/// moves are still pending; any state outside the projection abandons it.
/// Updates may skip states, so a repeated arrangement is ambiguous: while moves
/// may still be in flight it matches its earliest occurrence, keeping a
/// projection that agrees with the observed state; once `settled`, its latest,
/// so a finished round trip ends the projection.
pub fn reconcile_projection<T: PartialEq>(
    states: &mut Vec<Vec<Vec<T>>>,
    observed: &[Vec<T>],
    settled: bool,
) -> bool {
    let matches = |state: &Vec<Vec<T>>| state == observed;
    let index = if settled {
        states.iter().rposition(matches)
    } else {
        states.iter().position(matches)
    };
    match index {
        Some(index) if index + 1 < states.len() => {
            states.drain(..index);
            true
        }
        _ => false,
    }
}

/// Zellij's swap with the neighboring pane, for the moves the planner emits.
fn apply_native_move<T: Copy + PartialEq>(
    columns: &mut [Vec<T>],
    pane: T,
    direction: PaneMoveDirection,
) {
    let Some(column) = columns.iter().position(|column| column.contains(&pane)) else {
        return;
    };
    let Some(position) = columns[column].iter().position(|id| *id == pane) else {
        return;
    };
    match direction {
        PaneMoveDirection::Up if position > 0 => columns[column].swap(position, position - 1),
        PaneMoveDirection::Down if position + 1 < columns[column].len() => {
            columns[column].swap(position, position + 1)
        }
        PaneMoveDirection::Left if column > 0 && columns[column - 1].len() == 1 => {
            columns[column][position] = std::mem::replace(&mut columns[column - 1][0], pane);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        project_pane_move, reconcile_projection, resolve_vertical_focus, work_pane_columns,
        PaneMoveDirection, VerticalDirection, VerticalFocusPlan, VerticalPaneSnapshot,
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
            is_tiled: true,
            is_focused,
            pane_x,
            pane_y,
            pane_columns,
            pane_rows: 1,
        }
    }

    fn rows(pane: VerticalPaneSnapshot, pane_rows: usize) -> VerticalPaneSnapshot {
        VerticalPaneSnapshot { pane_rows, ..pane }
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

    type Columns = Vec<Vec<usize>>;

    /// Plans `direction` for `pane` from `columns`, returning the native moves
    /// and the final projected columns.
    fn step(
        columns: &[Vec<usize>],
        pane: usize,
        direction: PaneMoveDirection,
    ) -> Option<(Vec<(usize, PaneMoveDirection)>, Columns)> {
        let mut states = vec![columns.to_vec()];
        let moves = project_pane_move(&mut states, pane, direction)?;
        Some((moves, states.pop().unwrap()))
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
        let columns = work_pane_columns(&stacked, 1);
        assert_eq!(columns, vec![vec![1, 2, 3]]);
        let (moves, columns) = step(&columns, 1, Up).unwrap();
        assert_eq!(moves, vec![(1, Down), (1, Down)]);
        assert_eq!(columns, vec![vec![2, 3, 1]]);
        let (moves, columns) = step(&columns, 1, Down).unwrap();
        assert_eq!(moves, vec![(1, Up), (1, Up)]);
        assert_eq!(columns, vec![vec![1, 2, 3]]);
        let (moves, columns) = step(&columns, 2, Up).unwrap();
        assert_eq!(moves, vec![(2, Up)]);
        assert_eq!(columns, vec![vec![2, 1, 3]]);
        for direction in [Left, Right] {
            assert_eq!(step(&columns, 2, direction).unwrap().0, vec![]);
        }
        assert_eq!(step(&columns, 0, Up), None);
    }

    #[test]
    fn crosses_columns_preserving_the_order_of_other_panes() {
        let split = [
            pane(false, false, 0, 0, 24),
            rows(pane(true, false, 24, 1, 40), 3),
            pane(true, false, 64, 1, 40),
            pane(true, false, 64, 2, 40),
            pane(true, true, 64, 3, 40),
            pane(false, false, 104, 1, 16),
            // Bars span both columns and the sidebar, so they are rows.
            pane(false, false, 0, 0, 120),
            pane(false, false, 0, 4, 120),
        ];
        let columns = work_pane_columns(&split, 4);
        assert_eq!(columns, vec![vec![1], vec![2, 3, 4]]);
        let (moves, columns) = step(&columns, 4, Left).unwrap();
        assert_eq!(moves, vec![(4, Up), (4, Up), (4, Left)]);
        assert_eq!(columns, vec![vec![4], vec![1, 2, 3]]);
        assert_eq!(step(&columns, 4, Left).unwrap().0, vec![]);
        let (moves, columns) = step(&columns, 4, Right).unwrap();
        assert_eq!(moves, vec![(1, Left)]);
        assert_eq!(columns, vec![vec![1], vec![4, 2, 3]]);
        assert_eq!(step(&columns, 4, Right).unwrap().0, vec![]);

        // Zellij would swap with an arbitrary pane of a multi-pane left column.
        let columns = vec![vec![1, 2], vec![3]];
        assert_eq!(step(&columns, 3, Left).unwrap().0, vec![]);
        assert_eq!(step(&columns, 1, Right).unwrap().0, vec![]);
    }

    #[test]
    fn projection_survives_only_its_own_intermediate_states() {
        let mut states = vec![vec![vec![1], vec![2, 3, 4]]];
        // Rapid moves plan from the projection before any pane update.
        assert_eq!(
            project_pane_move(&mut states, 1, Right),
            Some(vec![(2, Left)])
        );
        assert_eq!(
            project_pane_move(&mut states, 1, Down),
            Some(vec![(1, Down)])
        );
        assert_eq!(states.last().unwrap(), &vec![vec![2], vec![3, 1, 4]]);

        let mut pending = states.clone();
        assert!(reconcile_projection(
            &mut pending,
            &[vec![1], vec![2, 3, 4]],
            false
        ));
        assert!(reconcile_projection(
            &mut pending,
            &[vec![2], vec![1, 3, 4]],
            false
        ));
        assert!(!reconcile_projection(
            &mut pending,
            &[vec![2], vec![3, 1, 4]],
            false
        ));
        // A rearrangement outside the projection abandons it.
        assert!(!reconcile_projection(
            &mut states,
            &[vec![2], vec![3, 4, 1]],
            false
        ));

        // Rapid Right, Left, Right repeats arrangements. In flight, an early
        // update keeps the remaining moves; once settled, a finished round trip
        // ends the projection even when updates skip intermediate states.
        let mut states = vec![vec![vec![1], vec![2, 3]]];
        for direction in [Right, Left, Right] {
            project_pane_move(&mut states, 1, direction);
        }
        let mut in_flight = states.clone();
        assert!(reconcile_projection(
            &mut in_flight,
            &[vec![2], vec![1, 3]],
            false
        ));
        assert!(reconcile_projection(
            &mut in_flight,
            &[vec![1], vec![2, 3]],
            false
        ));
        assert_eq!(in_flight.last().unwrap(), &vec![vec![2], vec![1, 3]]);
        states.pop();
        assert!(!reconcile_projection(
            &mut states,
            &[vec![1], vec![2, 3]],
            true
        ));
    }

    #[test]
    fn never_crosses_a_boundary_that_would_swap_another_pane() {
        // A non-work pane between the columns, a left pane shorter than the
        // column beside it, and non-work panes inside the right or left column,
        // including one only partly inside the right column.
        for (left, ui, right_x) in [
            (
                rows(pane(true, false, 24, 1, 40), 3),
                pane(false, false, 64, 1, 16),
                80,
            ),
            (
                rows(pane(true, false, 24, 1, 40), 2),
                pane(false, false, 0, 0, 24),
                64,
            ),
            (
                rows(pane(true, false, 24, 1, 40), 4),
                pane(false, false, 64, 2, 40),
                64,
            ),
            (
                rows(pane(true, false, 24, 1, 40), 3),
                pane(false, false, 24, 5, 40),
                64,
            ),
            (
                rows(pane(true, false, 24, 1, 40), 4),
                pane(false, false, 64, 2, 80),
                64,
            ),
        ] {
            let panes = [
                left,
                rows(ui, 3),
                pane(true, false, right_x, 1, 40),
                pane(true, false, right_x, 2, 40),
                pane(true, true, right_x, 3, 40),
            ];
            let columns = work_pane_columns(&panes, 4);
            assert_eq!(columns, vec![vec![0], vec![], vec![2, 3, 4]]);
            assert_eq!(step(&columns, 4, Left).unwrap().0, vec![]);
            assert_eq!(step(&columns, 0, Right).unwrap().0, vec![]);
        }
    }

    #[test]
    fn irregular_geometry_keeps_the_focused_vertical_cycle() {
        // A wide pane above two half-width panes.
        let panes = [
            pane(true, false, 0, 0, 80),
            pane(true, true, 0, 10, 40),
            pane(true, false, 40, 10, 40),
        ];
        let columns = work_pane_columns(&panes, 1);
        assert_eq!(columns, vec![vec![0, 1]]);
        assert_eq!(step(&columns, 1, Down).unwrap().0, vec![(1, Up)]);
        assert_eq!(step(&columns, 0, Right).unwrap().0, vec![]);
    }
}
