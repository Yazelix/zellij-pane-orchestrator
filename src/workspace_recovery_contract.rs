use std::path::{Path, PathBuf};

pub fn recovered_workspace_root(
    current_root: &Path,
    is_bootstrap: bool,
    editor_cwd: &Path,
) -> Option<PathBuf> {
    (is_bootstrap && editor_cwd.is_absolute() && editor_cwd != current_root)
        .then(|| editor_cwd.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::recovered_workspace_root;
    use std::path::{Path, PathBuf};

    #[test]
    fn recovers_only_stale_bootstrap_state_from_an_absolute_editor_cwd() {
        assert_eq!(
            recovered_workspace_root(Path::new("/home/user"), true, Path::new("/repo")),
            Some(PathBuf::from("/repo"))
        );
        assert_eq!(
            recovered_workspace_root(Path::new("/home/user"), false, Path::new("/repo")),
            None
        );
        assert_eq!(
            recovered_workspace_root(Path::new("/home/user"), true, Path::new("/home/user")),
            None
        );
        assert_eq!(
            recovered_workspace_root(Path::new("/home/user"), true, Path::new("repo")),
            None
        );
    }
}
