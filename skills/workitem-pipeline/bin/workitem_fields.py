"""Reads and edits the flat `key: value` frontmatter of a workitem.md, for the watch scripts."""
import os, re


def read_fields(path):
    with open(path) as f:
        text = f.read()
    head = text[4:].split("\n---", 1)[0] if text.startswith("---\n") else ""
    return {k.strip(): v.strip() for k, sep, v in (line.partition(":") for line in head.splitlines()) if sep}


def with_fields(text, updates):
    """`text` with the keys set in its frontmatter, appending the ones that are not there yet. A
    value of None removes the key. Text without frontmatter comes back as it is."""
    if not text.startswith("---\n") or "\n---" not in text[3:]:
        return text
    end = text.index("\n---", 3)
    head, rest = text[:end], text[end:]
    for key, value in updates.items():
        pattern = rf"^{re.escape(key)}:.*$"
        if value is None:
            head = re.sub(pattern + r"\n?", "", head, count=1, flags=re.M).rstrip("\n")
            continue
        line = f"{key}: {value}".rstrip()
        if re.search(pattern, head, re.M):
            head = re.sub(pattern, lambda _: line, head, count=1, flags=re.M)
        else:
            head += "\n" + line
    return head + rest


def set_fields(path, updates):
    """Sets keys in the frontmatter (see with_fields). Agents, the backend and other watchers write
    the same file, so it rereads the file right before replacing it, starts over when someone
    wrote in between, and replaces it in one rename."""
    if not updates:
        return
    for _ in range(5):
        with open(path) as f:
            text = f.read()
        new = with_fields(text, updates)
        if new == text:
            return
        tmp = f"{path}.{os.getpid()}.tmp"
        with open(tmp, "w") as f:
            f.write(new)
        with open(path) as f:
            if f.read() == text:
                os.replace(tmp, path)
                return
        os.remove(tmp)
    raise RuntimeError(f"{path} kept changing; not written")
