//! Git worktree operations via `git2`.
//!
//! This module handles the filesystem/git side of worktree lifecycle.
//! Database tracking lives in `sober-db`.

use std::path::Path;
use std::time::Instant;

use metrics::{counter, histogram};

use crate::WorkspaceError;

/// Create a new git worktree for the given branch at the specified path.
///
/// If the branch does not exist, it is created from HEAD.
pub fn create_git_worktree(
    repo_path: &Path,
    worktree_path: &Path,
    branch: &str,
) -> Result<(), WorkspaceError> {
    let start = Instant::now();

    let repo = git2::Repository::open(repo_path).map_err(|e| {
        counter!("sober_workspace_worktree_operations_total", "operation" => "create", "status" => "error")
            .increment(1);
        histogram!("sober_workspace_worktree_duration_seconds", "operation" => "create")
            .record(start.elapsed().as_secs_f64());
        WorkspaceError::Git(e)
    })?;

    // Derive a worktree name from the branch (replace / with -)
    let wt_name = branch.replace('/', "-");

    // Check if branch exists; if not, create it from HEAD
    let branch_ref = match repo.find_branch(branch, git2::BranchType::Local) {
        Ok(b) => b.into_reference(),
        Err(_) => {
            let head_commit = repo
                .head()
                .map_err(WorkspaceError::Git)?
                .peel_to_commit()
                .map_err(WorkspaceError::Git)?;
            repo.branch(branch, &head_commit, false)
                .map_err(WorkspaceError::Git)?
                .into_reference()
        }
    };

    repo.worktree(
        &wt_name,
        worktree_path,
        Some(git2::WorktreeAddOptions::new().reference(Some(&branch_ref))),
    )
    .map_err(|e| {
        counter!("sober_workspace_worktree_operations_total", "operation" => "create", "status" => "error")
            .increment(1);
        histogram!("sober_workspace_worktree_duration_seconds", "operation" => "create")
            .record(start.elapsed().as_secs_f64());
        WorkspaceError::Git(e)
    })?;

    counter!("sober_workspace_worktree_operations_total", "operation" => "create", "status" => "success")
        .increment(1);
    histogram!("sober_workspace_worktree_duration_seconds", "operation" => "create")
        .record(start.elapsed().as_secs_f64());

    Ok(())
}

/// Remove a git worktree by path.
///
/// Prunes the worktree reference from the parent repo and removes the
/// directory from disk.
pub fn remove_git_worktree(repo_path: &Path, worktree_path: &Path) -> Result<(), WorkspaceError> {
    let start = Instant::now();

    let repo = git2::Repository::open(repo_path).map_err(|e| {
        counter!("sober_workspace_worktree_operations_total", "operation" => "remove", "status" => "error")
            .increment(1);
        histogram!("sober_workspace_worktree_duration_seconds", "operation" => "remove")
            .record(start.elapsed().as_secs_f64());
        WorkspaceError::Git(e)
    })?;

    // Find the worktree by matching its path
    let worktrees = repo.worktrees().map_err(WorkspaceError::Git)?;
    for name in worktrees.iter().flatten() {
        if let Ok(wt) = repo.find_worktree(name)
            && wt.path() == worktree_path
        {
            wt.prune(Some(
                git2::WorktreePruneOptions::new()
                    .valid(true)
                    .working_tree(true),
            ))
            .map_err(|e| {
                counter!("sober_workspace_worktree_operations_total", "operation" => "remove", "status" => "error")
                    .increment(1);
                histogram!("sober_workspace_worktree_duration_seconds", "operation" => "remove")
                    .record(start.elapsed().as_secs_f64());
                WorkspaceError::Git(e)
            })?;

            counter!("sober_workspace_worktree_operations_total", "operation" => "remove", "status" => "success")
                .increment(1);
            histogram!("sober_workspace_worktree_duration_seconds", "operation" => "remove")
                .record(start.elapsed().as_secs_f64());
            return Ok(());
        }
    }

    // If we didn't find it as a registered worktree, just remove the directory
    if worktree_path.exists() {
        std::fs::remove_dir_all(worktree_path).map_err(WorkspaceError::Filesystem)?;
    }

    counter!("sober_workspace_worktree_operations_total", "operation" => "remove", "status" => "success")
        .increment(1);
    histogram!("sober_workspace_worktree_duration_seconds", "operation" => "remove")
        .record(start.elapsed().as_secs_f64());

    Ok(())
}

/// List all worktree paths for a repository.
pub fn list_git_worktrees(repo_path: &Path) -> Result<Vec<String>, WorkspaceError> {
    let repo = git2::Repository::open(repo_path).map_err(WorkspaceError::Git)?;
    let worktrees = repo.worktrees().map_err(WorkspaceError::Git)?;

    Ok(worktrees.iter().flatten().map(String::from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_repo_with_commit(path: &Path) -> git2::Repository {
        let repo = git2::Repository::init(path).unwrap();

        // Create initial commit so we have a HEAD
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
    fn create_and_verify_worktree() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        init_repo_with_commit(&repo_path);

        let wt_path = tmp.path().join("worktree");
        create_git_worktree(&repo_path, &wt_path, "feat/test").unwrap();

        assert!(wt_path.exists());
        // .git file (not directory) is created for worktrees
        assert!(wt_path.join(".git").exists());
    }

    #[test]
    fn remove_worktree_cleans_up() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        init_repo_with_commit(&repo_path);

        let wt_path = tmp.path().join("worktree");
        create_git_worktree(&repo_path, &wt_path, "feat/remove-me").unwrap();
        assert!(wt_path.exists());

        remove_git_worktree(&repo_path, &wt_path).unwrap();
        assert!(!wt_path.exists());
    }

    #[test]
    fn list_worktrees_after_creation() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        init_repo_with_commit(&repo_path);

        let wt_path = tmp.path().join("wt1");
        create_git_worktree(&repo_path, &wt_path, "feat/one").unwrap();

        let names = list_git_worktrees(&repo_path).unwrap();
        assert!(!names.is_empty());
    }

    #[test]
    fn create_worktree_creates_branch_if_missing() {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().join("repo");
        let repo = init_repo_with_commit(&repo_path);

        let wt_path = tmp.path().join("worktree");
        create_git_worktree(&repo_path, &wt_path, "feat/new-branch").unwrap();

        // Branch should now exist
        let branch = repo.find_branch("feat/new-branch", git2::BranchType::Local);
        assert!(branch.is_ok());
    }
}
