"""Where a repo keeps its workitems. Shared by the scripts in this folder."""
from pathlib import Path


def workitems_dir(repo):
    """`<repo>/.workitems`, or the older `<repo>/.claude/workitems` when only that exists."""
    new, old = Path(repo, ".workitems"), Path(repo, ".claude/workitems")
    return old if old.exists() and not new.exists() else new
