#!/usr/bin/env bash
# Tests for scripts/build-android.sh game-name handling. Every case runs the script in --dry-run
# mode against a scratch copy of the script and a fake games folder, so no NDK, cargo or
# signing is needed and the real repository is untouched.
#
#   scripts/tests/build-android.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

fixture="$scratch/repo"
mkdir -p "$fixture/scripts" "$fixture/games"
cp "$repo/scripts/build-android.sh" "$fixture/scripts/build-android.sh"

# A fake game: a crate folder and an APK manifest, as the real games have.
make_game() {
  local name="$1"
  mkdir -p "$fixture/games/$name/game" "$fixture/games/$name/android"
  : > "$fixture/games/$name/game/Cargo.toml"
  : > "$fixture/games/$name/android/AndroidManifest.xml"
}
make_game sandbox
make_game demo
make_game tiny-game

script="$fixture/scripts/build-android.sh"

# Runs the script with the given arguments and checks its exit status; output goes to $out.
expect_status() {
  local want="$1"
  shift
  local got=0
  out="$("$script" "$@" 2>&1)" || got=$?
  if [[ "$got" -ne "$want" ]]; then
    fail "build-android $* exited $got, expected $want: $out"
  fi
}

# The default game is sandbox, with its library and APK names derived from it.
expect_status 0 --dry-run
grep -q 'dry run for game sandbox' <<<"$out" || fail "default game is not sandbox: $out"
grep -q 'library libsandbox.so' <<<"$out" || fail "sandbox library name is wrong: $out"
grep -q 'APK .*/target/android/sandbox-debug.apk' <<<"$out" || fail "sandbox APK path is wrong: $out"

# A named game derives its own names.
expect_status 0 --dry-run demo
grep -q 'library libdemo.so' <<<"$out" || fail "demo library name is wrong: $out"
grep -q 'APK .*/target/android/demo-debug.apk' <<<"$out" || fail "demo APK path is wrong: $out"
grep -q 'manifest .*games/demo/android/AndroidManifest.xml' <<<"$out" \
  || fail "demo manifest path is wrong: $out"

# A dash in the game name becomes an underscore in the library name only.
expect_status 0 --dry-run tiny-game
grep -q 'library libtiny_game.so' <<<"$out" || fail "dashed library name is wrong: $out"
grep -q 'APK .*/target/android/tiny-game-debug.apk' <<<"$out" \
  || fail "dashed APK name is wrong: $out"

# Invalid names are refused before any path is used.
expect_status 1 --dry-run Bad-Name
expect_status 1 --dry-run 1starts-with-digit
expect_status 1 --dry-run ../sandbox
expect_status 1 --dry-run 'has space'

# A name with no game crate, or with no manifest, is refused.
expect_status 1 --dry-run nosuchgame
mkdir -p "$fixture/games/nomanifest/game"
: > "$fixture/games/nomanifest/game/Cargo.toml"
expect_status 1 --dry-run nomanifest

# Extra arguments are refused.
expect_status 1 --dry-run demo extra

# A dry run builds nothing and writes nothing.
[[ ! -e "$fixture/target" ]] || fail "a dry run must not create target/"

echo "PASS: scripts/tests/build-android.sh"
