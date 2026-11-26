"""Test helper: makes a script's run see a concurrent writer. `env(tmp, file, line, after)` returns
the env for a subprocess in which, right after the script's `after`-th read of `file`, `line` is
added to the frontmatter on disk, as if another process wrote it in between."""
import os
from pathlib import Path

SITECUSTOMIZE = '''
import builtins, io, os
_file, _line, _after = os.environ["RACE_FILE"], os.environ["RACE_LINE"], int(os.environ["RACE_AFTER"])
_open, _reads = io.open, 0

def _racing_open(path, mode="r", *args, **kwargs):
    global _reads
    if "r" not in mode or "+" in mode or "b" in mode or isinstance(path, int) or os.path.realpath(path) != _file:
        return _open(path, mode, *args, **kwargs)
    with _open(path, mode, *args, **kwargs) as f:
        text = f.read()
    _reads += 1
    if _reads == _after:
        with _open(_file, "w") as f:
            f.write(text.replace("\\n---", "\\n" + _line + "\\n---", 1))
    return io.StringIO(text)

builtins.open = io.open = _racing_open
'''


def env(tmp, file, line, after=1):
    folder = Path(tmp, "race")
    folder.mkdir(exist_ok=True)
    (folder / "sitecustomize.py").write_text(SITECUSTOMIZE)
    return {
        **os.environ,
        "PYTHONPATH": os.pathsep.join(filter(None, [str(folder), os.environ.get("PYTHONPATH")])),
        "RACE_FILE": os.path.realpath(file),
        "RACE_LINE": line,
        "RACE_AFTER": str(after),
    }
