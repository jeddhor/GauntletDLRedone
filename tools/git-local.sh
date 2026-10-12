#!/usr/bin/env bash
# The session reserves the root .git path; keep metadata in an ignored local directory.
set -euo pipefail
gdl_root="$(cd "$(dirname "$0")/.." && pwd)"
exec git --git-dir="$gdl_root/private/source-control.git" --work-tree="$gdl_root" "$@"
