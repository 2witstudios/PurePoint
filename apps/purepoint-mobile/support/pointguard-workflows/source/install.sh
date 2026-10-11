#!/bin/zsh
# Symlink these skills into the agent skill directories.
#   install.sh          — link missing skills, keep existing installs untouched
#   install.sh --force  — replace existing installs (and legacy aidd-* names)
set -euo pipefail

REPO="$(cd "$(dirname "$0")" && pwd)"
TARGETS=("$HOME/.claude/skills" "$HOME/.config/opencode/skills" "$HOME/.agents/skills")
FORCE=0
[[ "${1:-}" == "--force" ]] && FORCE=1

# legacy name -> new name (installed under the old aidd- prefix)
LEGACY=(aidd-triage:triage aidd-geo-interview:geo-interview)

for target in $TARGETS; do
  mkdir -p "$target"
  # clean up legacy-named copies first
  for pair in $LEGACY; do
    old="${pair%%:*}"; new="${pair##*:}"
    if [[ -e "$target/$old" && ! -L "$target/$old" ]]; then
      if (( FORCE )); then rm -rf "$target/$old"; echo "removed legacy $target/$old"; fi
    fi
  done
  for skill in "$REPO"/skills/*(/); do
    name="${skill:t}"
    dest="$target/$name"
    if [[ -L "$dest" ]]; then
      rm -f "$dest"
    elif [[ -e "$dest" ]]; then
      if (( ! FORCE )); then echo "skip $dest (exists, use --force to replace)"; continue; fi
      rm -rf "$dest"
    fi
    ln -s "$skill" "$dest"
    echo "linked $dest -> $skill"
  done
done
