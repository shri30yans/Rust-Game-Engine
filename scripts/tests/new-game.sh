#!/usr/bin/env bash
# Tests for scripts/new-game.sh. Each case runs the generator against a scratch copy of the
# workspace manifest and the sandbox template (via REPO_ROOT), so the real repository is untouched.
#
#   scripts/tests/new-game.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
generator="$repo/scripts/new-game.sh"

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

# Runs the generator with the given arguments and checks its exit status.
expect_status() {
  local want="$1"
  shift
  local got=0
  REPO_ROOT="$fixture" "$generator" "$@" >/dev/null 2>&1 || got=$?
  if [[ "$got" -ne "$want" ]]; then
    fail "new-game $* exited $got, expected $want"
  fi
}

fixture="$scratch/repo"
mkdir -p "$fixture/games"
cp -R "$repo/games/sandbox" "$fixture/games/sandbox"
cp "$repo/Cargo.toml" "$fixture/Cargo.toml"

# Usage and name validation are checked before anything is copied.
expect_status 2
expect_status 2 Bad-Name
expect_status 2 1starts_with_digit
expect_status 2 has.dot
[[ ! -e "$fixture/games/Bad-Name" ]] || fail "an invalid name must not create a directory"

# A new game is created, renamed, and added to the workspace after the sandbox.
expect_status 0 tiny_game
game="$fixture/games/tiny_game"
[[ -f "$game/game/Cargo.toml" ]] || fail "games/tiny_game/game/Cargo.toml was not created"
[[ -f "$game/game/src/main.rs" ]] || fail "the copied sources are missing"

names="$(grep -c '^name = "tiny_game"$' "$game/game/Cargo.toml" || true)"
[[ "$names" -eq 3 ]] || fail "expected package, lib and bin renamed to tiny_game, found $names"
if grep -q 'sandbox' "$game/game/Cargo.toml"; then
  fail "the new Cargo.toml still mentions sandbox"
fi
if grep -rq 'sandbox::' "$game/game/src"; then
  fail "the new sources still refer to the sandbox crate"
fi
grep -q 'tiny_game::' "$game/game/src/main.rs" || fail "main.rs does not refer to the renamed crate"

members="$(grep -c '"games/tiny_game/game",' "$fixture/Cargo.toml" || true)"
[[ "$members" -eq 1 ]] || fail "expected one workspace member for tiny_game, found $members"
sandbox_line="$(grep -n '"games/sandbox/game",' "$fixture/Cargo.toml" | cut -d: -f1)"
tiny_line="$(grep -n '"games/tiny_game/game",' "$fixture/Cargo.toml" | cut -d: -f1)"
[[ "$tiny_line" -eq $((sandbox_line + 1)) ]] || fail "the new member must follow the sandbox member"

# An existing game is never overwritten, and the manifest is left alone.
before="$(cat "$fixture/Cargo.toml")"
expect_status 1 tiny_game
expect_status 1 sandbox
[[ "$before" == "$(cat "$fixture/Cargo.toml")" ]] || fail "a refused run changed the manifest"

echo "PASS: scripts/tests/new-game.sh"
