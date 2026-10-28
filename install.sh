#!/usr/bin/env bash
# Symlink every skill in ./skills into ~/.claude/skills.
# A real folder with the same name is moved to ./backup first. uninstall.sh restores it.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
target="${CLAUDE_SKILLS_DIR:-$HOME/.claude/skills}"
backup="$root/backup"

mkdir -p "$target" "$backup"

for skill in "$root"/skills/*/; do
  name="$(basename "$skill")"
  link="$target/$name"

  if [ -L "$link" ]; then
    if [ "$(readlink "$link")" = "${skill%/}" ]; then
      echo "ok       $name (already linked)"
      continue
    fi
    echo "refusing $name: $link is a symlink to $(readlink "$link")" >&2
    exit 1
  fi

  if [ -e "$link" ]; then
    if [ -e "$backup/$name" ]; then
      echo "refusing $name: $backup/$name already exists" >&2
      exit 1
    fi
    mv "$link" "$backup/$name"
    echo "backup   $name -> $backup/$name"
  fi

  ln -s "${skill%/}" "$link"
  echo "linked   $name"
done
