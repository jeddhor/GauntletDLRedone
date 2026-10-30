#!/usr/bin/env bash
# The development session reserves the root .git path; keep metadata in an
# ignored local directory. A clone without it uses its own .git.
set -euo pipefail
gdl_root="$(cd "$(dirname "$0")/.." && pwd)"
if [ -d "$gdl_root/private/source-control.git" ]; then
    exec git --git-dir="$gdl_root/private/source-control.git" --work-tree="$gdl_root" "$@"
fi
exec git -C "$gdl_root" "$@"
