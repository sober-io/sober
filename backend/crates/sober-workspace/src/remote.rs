//! Git remote operations.

use std::path::Path;

use crate::WorkspaceError;

/// Detect the remote URL for a repository.
///
/// Tries "origin" first, then falls back to the first available remote.
/// Returns `None` if no remotes are configured.
pub fn detect_remote_url(repo_path: &Path) -> Result<Option<String>, WorkspaceError> {
    let repo = git2::Repository::open(repo_path).map_err(WorkspaceError::Git)?;

    // Try "origin" first
    if let Ok(remote) = repo.find_remote("origin") {
        if let Some(url) = remote.url() {
            return Ok(Some(url.to_string()));
        }
    }

    // Fall back to first available remote
    let remotes = repo.remotes().map_err(WorkspaceError::Git)?;
    for name in remotes.iter().flatten() {
        if let Ok(remote) = repo.find_remote(name) {
            if let Some(url) = remote.url() {
                return Ok(Some(url.to_string()));
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo_with_remote(path: &Path, url: &str) -> git2::Repository {
        let repo = git2::Repository::init(path).unwrap();
        repo.remote("origin", url).unwrap();
        // Create initial commit so HEAD exists
        {
            let sig = git2::Signature::now("Test", "test@test.com").unwrap();
            let tree_id = repo.index().unwrap().write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
                .unwrap();
        }
        repo
    }

    #[test]
    fn detect_remote_url_finds_origin() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        init_repo_with_remote(&repo_path, "git@github.com:user/repo.git");

        let url = detect_remote_url(&repo_path).unwrap();
        assert_eq!(url.as_deref(), Some("git@github.com:user/repo.git"));
    }

    #[test]
    fn detect_remote_url_returns_none_without_remotes() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        git2::Repository::init(&repo_path).unwrap();

        let url = detect_remote_url(&repo_path).unwrap();
        assert!(url.is_none());
    }

    #[test]
    fn detect_remote_url_falls_back_to_first_remote() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        let repo = git2::Repository::init(&repo_path).unwrap();
        repo.remote("upstream", "https://example.com/repo.git")
            .unwrap();

        let url = detect_remote_url(&repo_path).unwrap();
        assert_eq!(url.as_deref(), Some("https://example.com/repo.git"));
    }
}
