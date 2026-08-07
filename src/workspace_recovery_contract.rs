use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub fn reconcile_active_workspace_state<T: Clone>(
    workspace_state_by_tab: &mut HashMap<usize, T>,
    current_tab_ids: &HashSet<usize>,
    active_tab_id: Option<usize>,
    initial_workspace_state: Option<&T>,
) {
    workspace_state_by_tab.retain(|tab_id, _| current_tab_ids.contains(tab_id));
    if let (Some(active_tab_id), Some(initial_workspace_state)) =
        (active_tab_id, initial_workspace_state)
    {
        workspace_state_by_tab
            .entry(active_tab_id)
            .or_insert_with(|| initial_workspace_state.clone());
    }
}

pub fn recovered_workspace_root(current_root: &Path, editor_cwd: &Path) -> Option<PathBuf> {
    (editor_cwd.is_absolute() && editor_cwd != current_root).then(|| editor_cwd.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{reconcile_active_workspace_state, recovered_workspace_root};
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};

    #[test]
    fn recovers_only_a_changed_absolute_editor_cwd() {
        assert_eq!(
            recovered_workspace_root(Path::new("/home/user"), Path::new("/repo")),
            Some(PathBuf::from("/repo"))
        );
        assert_eq!(
            recovered_workspace_root(Path::new("/home/user"), Path::new("/home/user")),
            None
        );
        assert_eq!(
            recovered_workspace_root(Path::new("/home/user"), Path::new("repo")),
            None
        );
    }

    #[test]
    fn reconstructed_plugin_seeds_each_preexisting_tab_when_it_becomes_active() {
        let current_tab_ids = HashSet::from([10, 20]);
        let mut workspace_state_by_tab = HashMap::from([(10, "/repo-a"), (30, "/closed")]);

        reconcile_active_workspace_state(
            &mut workspace_state_by_tab,
            &current_tab_ids,
            Some(20),
            Some(&"/home/user"),
        );

        assert_eq!(
            workspace_state_by_tab,
            HashMap::from([(10, "/repo-a"), (20, "/home/user")])
        );
    }
}
