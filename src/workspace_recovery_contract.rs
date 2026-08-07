use std::path::{Path, PathBuf};

pub fn recovered_workspace_root(current_root: &Path, editor_cwd: &Path) -> Option<PathBuf> {
    (editor_cwd.is_absolute() && editor_cwd != current_root).then(|| editor_cwd.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::recovered_workspace_root;
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
}
