use std::path::{Path, PathBuf};

use tokio::process::Command;

/// Paths of the worktrees git has registered for `repo`.
pub async fn worktrees(repo: &Path) -> Vec<PathBuf> {
    let Ok(out) = Command::new("git").args(["worktree", "list", "--porcelain"]).current_dir(repo).output().await else {
        return vec![];
    };
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).collect()
}

/// Folder name of the workitem's worktree: `<repo-name>-<dir>`.
fn worktree_name(repo: &Path, dir: &str) -> String {
    let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{name}-{dir}")
}

/// Where the workitem's worktree goes: `<workspace>/.worktrees/<repo-name>-<dir>` (decision 80),
/// outside the item folder, so removing an item never deletes a worktree git refused to remove.
/// A worktree made before that, next to the repo, stays where it is and is used while it exists.
pub fn worktree_path(workspace: &Path, repo: &Path, dir: &str) -> PathBuf {
    let name = worktree_name(repo, dir);
    let sibling = repo.with_file_name(&name);
    if sibling.is_dir() {
        return sibling;
    }
    workspace.join(".worktrees").join(name)
}

/// Path of the workitem's worktree among `worktrees`, matched by folder name, so a worktree
/// in either place is found.
pub fn worktree_for<'a>(worktrees: &'a [PathBuf], repo: &Path, dir: &str) -> Option<&'a PathBuf> {
    let name = worktree_name(repo, dir);
    worktrees.iter().find(|p| p.file_name().is_some_and(|n| n == name.as_str()))
}

async fn git_out(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(cwd).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The checked-out branch of a worktree and how many of its commits are unpushed (see `unpushed`);
/// `None` on a detached HEAD.
pub async fn branch(worktree: &Path) -> Option<(String, Option<u32>)> {
    let branch = git_out(worktree, &["symbolic-ref", "--short", "HEAD"]).await?;
    let unpushed = unpushed(worktree, &branch).await;
    Some((branch, unpushed))
}

/// Commits of the checked-out `branch` that its remote head lacks: the upstream if it tracks a
/// branch of the same name, else `origin/<branch>`. An upstream on another branch (a worktree
/// added from `origin/master`, or a stacked branch on its parent) is the base, not where the
/// branch is pushed. Commits are matched by patch, not hash, so a branch that was
/// rebased and force-pushed (for example by GitHub's update-branch) does not count as unpushed. A branch with neither was never pushed, so its commits that no
/// remote-tracking ref has count. Reads local refs only, so it is as fresh as the last fetch.
async fn unpushed(worktree: &Path, branch: &str) -> Option<u32> {
    let upstream = git_out(worktree, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]).await;
    let remote = match upstream.filter(|u| u.ends_with(&format!("/{branch}"))) {
        Some(upstream) => Some(upstream),
        None => {
            let candidate = format!("origin/{branch}");
            git_out(worktree, &["rev-parse", "--verify", "--quiet", &format!("refs/remotes/{candidate}")]).await.map(|_| candidate)
        }
    };
    let count = match &remote {
        Some(remote) => {
            let cherry = git_out(worktree, &["cherry", remote, "HEAD"]).await?;
            cherry.lines().filter(|l| l.starts_with('+')).count().to_string()
        }
        None => git_out(worktree, &["rev-list", "--count", "HEAD", "--not", "--remotes"]).await?,
    };
    count.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worktree_goes_to_the_workspace_and_is_found_in_either_place() {
        let tmp = std::env::temp_dir().join(format!("wt-test-{}", std::process::id()));
        let (workspace, repo) = (tmp.join("ws"), tmp.join("code/app"));
        std::fs::create_dir_all(&repo).unwrap();
        assert_eq!(worktree_path(&workspace, &repo, "T-1-x"), workspace.join(".worktrees/app-T-1-x"));

        let old = tmp.join("code/app-T-1-x");
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(worktree_path(&workspace, &repo, "T-1-x"), old, "an existing worktree next to the repo stays in use");

        let found = [repo.clone(), old.clone(), workspace.join(".worktrees/app-T-2-y")];
        assert_eq!(worktree_for(&found, &repo, "T-1-x"), Some(&old));
        assert_eq!(worktree_for(&found, &repo, "T-2-y"), Some(&found[2]));
        assert_eq!(worktree_for(&found, &repo, "T-3-z"), None);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
