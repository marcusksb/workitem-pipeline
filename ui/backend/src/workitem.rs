use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::config::Project;

#[derive(Serialize)]
pub struct Workitem {
    pub project: String,
    pub dir: String,
    pub title: String,
    pub kind: String,
    pub stage: String,
    /// Name of the board the item is on; `default` unless the frontmatter says otherwise.
    pub board: String,
    pub approved: bool,
    pub agent_status: String,
    pub issue: String,
    pub pr: String,
    /// Directory name of the workitem this one was split from, or empty.
    pub parent: String,
    pub created: String,
    /// PR head commit the item last looked at, or empty.
    pub head_sha: String,
}

/// Flat `key: value` frontmatter, as written by `workitem-create` and edited by `workitem-move`.
pub fn frontmatter(text: &str) -> HashMap<String, String> {
    let Some(rest) = text.strip_prefix("---\n") else {
        return HashMap::new();
    };
    let end = rest.find("\n---\n").or_else(|| rest.strip_suffix("\n---").map(str::len));
    let Some(end) = end else {
        return HashMap::new();
    };
    rest[..end]
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

/// `<repo>/.workitems`, or the older `<repo>/.claude/workitems` when only that exists.
pub fn workitems_dir(repo: &Path) -> PathBuf {
    let new = repo.join(".workitems");
    let old = repo.join(".claude/workitems");
    if old.exists() && !new.exists() {
        old
    } else {
        new
    }
}

/// `workitem.md` of the workitem `dir` in `repo`, whether or not it exists.
pub fn file(repo: &Path, dir: &str) -> PathBuf {
    workitems_dir(repo).join(dir).join("workitem.md")
}

/// Titles are written as YAML strings, so they often arrive wrapped in quotes.
fn unquote(value: &str) -> &str {
    ["\"", "'"]
        .iter()
        .find_map(|q| value.strip_prefix(q)?.strip_suffix(q))
        .unwrap_or(value)
}

fn parse(project: &Project, dir: &str, text: &str) -> Workitem {
    let fields = frontmatter(text);
    let field = |key: &str, default: &str| fields.get(key).filter(|v| !v.is_empty()).cloned().unwrap_or(default.to_string());
    Workitem {
        project: project.name.clone(),
        dir: dir.to_string(),
        title: unquote(&field("title", dir)).to_string(),
        kind: field("type", "-"),
        stage: field("stage", ""),
        board: field("board", crate::pipeline::DEFAULT_BOARD),
        approved: field("approved", "false") == "true",
        agent_status: field("agent_status", "ok"),
        issue: field("issue", "none"),
        pr: field("pr", ""),
        parent: field("parent", ""),
        created: field("created", ""),
        head_sha: field("head_sha", ""),
    }
}

pub fn list(project: &Project) -> Vec<Workitem> {
    let Ok(entries) = fs::read_dir(workitems_dir(&project.repo)) else {
        return vec![];
    };
    let mut items: Vec<Workitem> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let dir = entry.file_name().to_str()?.to_string();
            let text = fs::read_to_string(entry.path().join("workitem.md")).ok()?;
            Some(parse(project, &dir, &text))
        })
        .collect();
    items.sort_by(|a, b| a.dir.cmp(&b.dir));
    items
}

/// Path of `workitem.md` for a plain directory name that exists.
pub fn path(project: &Project, dir: &str) -> Option<PathBuf> {
    if dir.is_empty() || dir == "." || dir == ".." || dir.contains(['/', '\\', '\0']) {
        return None;
    }
    let path = file(&project.repo, dir);
    path.is_file().then_some(path)
}

/// Returns the item and its raw markdown.
pub fn read(project: &Project, dir: &str) -> Option<(Workitem, String)> {
    let text = fs::read_to_string(path(project, dir)?).ok()?;
    Some((parse(project, dir, &text), text))
}

fn frontmatter_end(text: &str) -> Option<usize> {
    let rest = text.strip_prefix("---\n")?;
    Some(4 + rest.find("\n---\n")?)
}

/// Sets frontmatter fields in place, appending missing ones. The body is untouched.
pub fn set_fields(path: &std::path::Path, fields: &[(&str, &str)]) -> io::Result<()> {
    let text = fs::read_to_string(path)?;
    let end = frontmatter_end(&text)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "workitem.md has no frontmatter"))?;
    let mut lines: Vec<String> = text[4..end].lines().map(String::from).collect();
    for (key, value) in fields {
        let line = format!("{key}: {value}");
        match lines.iter_mut().find(|l| l.split_once(':').is_some_and(|(k, _)| k.trim() == *key)) {
            Some(existing) => *existing = line,
            None => lines.push(line),
        }
    }
    fs::write(path, format!("---\n{}{}", lines.join("\n"), &text[end..]))
}

/// Replaces everything after the frontmatter with `body`, but only while it still equals `base`
/// (what the editor started from), so an agent's concurrent edit is never overwritten.
/// Returns `false` when it changed.
pub fn set_body(path: &std::path::Path, base: &str, body: &str) -> io::Result<bool> {
    let text = fs::read_to_string(path)?;
    let end = frontmatter_end(&text)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "workitem.md has no frontmatter"))?
        + "\n---\n".len();
    if text[end..] != *base {
        return Ok(false);
    }
    fs::write(path, format!("{}{body}", &text[..end]))?;
    Ok(true)
}

/// Replaces (or appends) a `## Rejection` section holding `comment`.
pub fn set_rejection(path: &std::path::Path, comment: &str) -> io::Result<()> {
    let text = fs::read_to_string(path)?;
    let mut kept = String::new();
    let mut skipping = false;
    for line in text.split_inclusive('\n') {
        if line.starts_with("## ") {
            skipping = line.trim_end() == "## Rejection";
        }
        if !skipping {
            kept.push_str(line);
        }
    }
    let kept = kept.trim_end();
    fs::write(path, format!("{kept}\n\n## Rejection\n\n{}\n", comment.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(repo: &std::path::Path) -> Project {
        Project { name: "app".into(), repo: repo.to_path_buf() }
    }

    #[test]
    fn workitems_dir_prefers_the_new_folder_and_falls_back_to_the_old_one() {
        let repo = tempfile::tempdir().unwrap();
        assert_eq!(workitems_dir(repo.path()), repo.path().join(".workitems"));
        fs::create_dir_all(repo.path().join(".claude/workitems")).unwrap();
        assert_eq!(workitems_dir(repo.path()), repo.path().join(".claude/workitems"));
        fs::create_dir_all(repo.path().join(".workitems")).unwrap();
        assert_eq!(workitems_dir(repo.path()), repo.path().join(".workitems"));
    }

    #[test]
    fn parses_frontmatter_values_with_colons() {
        let fields = frontmatter("---\ntitle: Fix: login\nstage: analyze\n---\n\n# body\nnot: field\n");
        assert_eq!(fields["title"], "Fix: login");
        assert_eq!(fields["stage"], "analyze");
        assert!(!fields.contains_key("not"));
        assert!(frontmatter("no frontmatter").is_empty());
    }

    #[test]
    fn lists_items_and_defaults_missing_fields() {
        let repo = tempfile::tempdir().unwrap();
        let dir = repo.path().join(".claude/workitems/SHAPE-1-foo");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("workitem.md"), "---\ntitle: Foo\nstage: analyze\nagent_status: blocked: why\napproved: true\npr: https://github.com/app/pull/9\n---\nbody").unwrap();
        fs::create_dir_all(repo.path().join(".claude/workitems/SHAPE-2-bar")).unwrap();
        fs::write(repo.path().join(".claude/workitems/SHAPE-2-bar/workitem.md"), "---\ntitle: Bar\nstage: analyze\nparent: SHAPE-1-foo\n---\nbody").unwrap();
        fs::create_dir_all(repo.path().join(".claude/workitems/no-file")).unwrap();
        let items = list(&project(repo.path()));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].stage, "analyze");
        assert_eq!(items[0].agent_status, "blocked: why");
        assert!(items[0].approved);
        assert_eq!(items[0].issue, "none");
        assert_eq!(items[0].pr, "https://github.com/app/pull/9");
        assert_eq!(items[1].pr, "");
        assert_eq!(items[1].parent, "SHAPE-1-foo");
        assert_eq!(items[0].parent, "");
    }

    #[test]
    fn set_fields_replaces_and_appends_without_touching_body() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("workitem.md");
        fs::write(&file, "---\ntitle: T\nagent_status: blocked: x\n---\n\nagent_status: in body\n").unwrap();
        set_fields(&file, &[("agent_status", "ok"), ("approved_at", "2026-09-28")]).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "---\ntitle: T\nagent_status: ok\napproved_at: 2026-09-28\n---\n\nagent_status: in body\n"
        );
    }

    #[test]
    fn set_body_keeps_frontmatter_and_refuses_a_stale_base() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("workitem.md");
        fs::write(&file, "---\nstage: analyze\n---\n\n# Old\n").unwrap();
        assert!(!set_body(&file, "\n# Something else\n", "\n# New\n").unwrap());
        assert!(set_body(&file, "\n# Old\n", "\n# New\n").unwrap());
        assert_eq!(fs::read_to_string(&file).unwrap(), "---\nstage: analyze\n---\n\n# New\n");
    }

    #[test]
    fn set_rejection_replaces_older_section_and_keeps_others() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("workitem.md");
        fs::write(&file, "---\na: 1\n---\n# T\n\n## Rejection\n\nold\n\n## Review brief\n\nkeep\n").unwrap();
        set_rejection(&file, "  new reason ").unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("## Review brief\n\nkeep"));
        assert!(text.ends_with("## Rejection\n\nnew reason\n"));
        assert!(!text.contains("old"));
    }

    #[test]
    fn read_rejects_path_traversal() {
        let repo = tempfile::tempdir().unwrap();
        let p = project(repo.path());
        assert!(read(&p, "../x").is_none());
        assert!(read(&p, "a/b").is_none());
        assert!(read(&p, "..").is_none());
    }
}
