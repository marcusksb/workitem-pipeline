use std::path::{Path, PathBuf};

use tokio::process::Command;

/// Paths of the worktrees git has registered for `repo`.
pub async fn worktrees(repo: &Path) -> Vec<PathBuf> {
    let Ok(out) = Command::new("git").args(["worktree", "list", "--porcelain"]).current_dir(repo).output().await else {
        return vec![];
    };
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).collect()
}

/// Where the workitem's worktree goes: `../<repo-name>-<dir>`.
pub fn worktree_path(repo: &Path, dir: &str) -> PathBuf {
    let name = repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    repo.with_file_name(format!("{name}-{dir}"))
}

/// Path of the workitem's worktree among `worktrees`.
pub fn worktree_for<'a>(worktrees: &'a [PathBuf], repo: &Path, dir: &str) -> Option<&'a PathBuf> {
    let name = worktree_path(repo, dir).file_name()?.to_owned();
    worktrees.iter().find(|p| p.file_name() == Some(name.as_os_str()))
}

async fn git_out(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(cwd).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Commits of the checked-out branch that its remote head lacks: the upstream if it tracks a
/// branch of the same name, else `origin/<branch>`. An upstream on another branch (a worktree
/// added from `origin/master`, or a stacked branch on its parent) is the base, not where the
/// branch is pushed. Commits are matched by patch, not hash, so a branch that was
/// rebased and force-pushed (for example by GitHub's update-branch) does not count as unpushed. A branch with neither was never pushed, so its commits that no
/// remote-tracking ref has count. Reads local refs only, so it is as fresh as the last fetch.
pub async fn unpushed(worktree: &Path) -> Option<u32> {
    let branch = git_out(worktree, &["symbolic-ref", "--short", "HEAD"]).await?;
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
