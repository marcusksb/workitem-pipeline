#!/usr/bin/env bash
# Remove the symlinks made by install.sh and restore any skill it moved to ./backup.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
target="${CLAUDE_SKILLS_DIR:-$HOME/.claude/skills}"
backup="$root/backup"

for skill in "$root"/skills/*/; do
  name="$(basename "$skill")"
  link="$target/$name"

  if [ -L "$link" ] && [ "$(readlink "$link")" = "${skill%/}" ]; then
    rm "$link"
    echo "removed  $name"
  elif [ -e "$link" ] || [ -L "$link" ]; then
    echo "skipped  $name: $link is not a link made by install.sh" >&2
    continue
  fi

  if [ -e "$backup/$name" ]; then
    mv "$backup/$name" "$link"
    echo "restored $name"
  fi
done
