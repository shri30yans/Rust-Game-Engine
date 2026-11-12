#!/usr/bin/env bash
# Create a new game from the sandbox template.
#
#   scripts/new-game.sh NAME
#
# Copies games/sandbox to games/NAME, renames the crate (package, lib and bin names, and the
# `sandbox` identifier in its sources and docs), and adds games/NAME/game to the workspace members
# right after the sandbox member. NAME must match [a-z][a-z0-9_]*. An existing games/NAME is never
# overwritten, and a run that fails part-way removes the copy it made. No network access.
#
# REPO_ROOT overrides the repository root; the tests use it to run against a scratch copy.
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/new-game.sh NAME" >&2
  exit 2
fi
name="$1"
if [[ ! "$name" =~ ^[a-z][a-z0-9_]*$ ]]; then
  echo "new-game: NAME must match [a-z][a-z0-9_]*, got '$name'" >&2
  exit 2
fi

repo="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
template="$repo/games/sandbox"
target="$repo/games/$name"
manifest="$repo/Cargo.toml"
anchor='    "games/sandbox/game",'

if [[ ! -d "$template" ]]; then
  echo "new-game: template $template not found" >&2
  exit 1
fi
if [[ -e "$target" ]]; then
  echo "new-game: $target already exists; refusing to overwrite" >&2
  exit 1
fi
if ! grep -qF "$anchor" "$manifest"; then
  echo "new-game: workspace member line for the sandbox not found in $manifest" >&2
  exit 1
fi

done_ok=0
trap 'if [[ $done_ok -eq 0 ]]; then rm -rf "$target"; fi' EXIT

cp -R "$template" "$target"

export NEW_NAME="$name"
find "$target" -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.md' \) \
  -exec perl -pi -e 's/\bsandbox\b|(?<=\blib)sandbox\b/$ENV{NEW_NAME}/g' {} +

perl -0pi -e 's|(    "games/sandbox/game",\n)|$1    "games/$ENV{NEW_NAME}/game",\n|' "$manifest"

done_ok=1
echo "new-game: created games/$name and added games/$name/game to the workspace."
echo "new-game: next, run 'cargo test -p $name' to check it (this updates Cargo.lock)."
