#!/usr/bin/env bash
# The one quality gate: formatting, layering, clippy and tests.
#
#   scripts/gate.sh [--base REF]   changed crates only: fmt, layers, clippy and tests of the
#                                  crates this branch touched (default base: main)
#   scripts/gate.sh --full         the whole workspace, run once per merge batch on main
#
# A change to the workspace root (Cargo.toml, Cargo.lock, .cargo/, clippy.toml, the layer table)
# can affect every crate, so it always runs the full gate.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

base="main"
full=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --full) full=1; shift ;;
    --base) base="$2"; shift 2 ;;
    *) echo "gate: unknown argument $1" >&2; exit 2 ;;
  esac
done

step() { echo; echo "== gate: $* =="; }

step "fmt"
cargo fmt --check

step "layers"
cargo run -q -p check-layers

if [[ "$full" -eq 0 ]]; then
  # Files changed on this branch since it left the base, plus uncommitted and untracked ones.
  changed="$(
    {
      git diff --name-only "$(git merge-base HEAD "$base")"
      git ls-files --others --exclude-standard
    } | sort -u
  )"
  if grep -qE '^(Cargo\.toml|Cargo\.lock|\.cargo/|clippy\.toml|rust-toolchain|tools/check-layers/layers\.json)' <<<"$changed"; then
    echo "gate: the workspace root changed, running the full gate"
    full=1
  fi
fi

if [[ "$full" -eq 1 ]]; then
  step "clippy (workspace)"
  cargo clippy --workspace --all-targets -- -D warnings
  step "test (workspace)"
  cargo test --workspace --no-fail-fast
  echo; echo "gate: full gate passed"
  exit 0
fi

# Map each changed file to the workspace crate whose folder holds it (the deepest match).
crates="$(
  cargo metadata --no-deps --format-version 1 | CHANGED="$changed" python3 -I -c '
import json, os, sys
root = os.getcwd()
meta = json.load(sys.stdin)
dirs = sorted(
    ((os.path.relpath(os.path.dirname(p["manifest_path"]), root) + "/", p["name"]) for p in meta["packages"]),
    key=lambda d: -len(d[0]),
)
found = set()
for path in os.environ["CHANGED"].splitlines():
    for folder, name in dirs:
        if path.startswith(folder):
            found.add(name)
            break
print(" ".join(sorted(found)))
'
)"

if [[ -z "$crates" ]]; then
  echo; echo "gate: no crate changed (docs or content only); fmt and layers passed"
  exit 0
fi

packages=()
for crate in $crates; do packages+=(-p "$crate"); done
echo; echo "gate: changed crates: $crates"

step "clippy (changed crates)"
cargo clippy "${packages[@]}" --all-targets -- -D warnings
step "test (changed crates)"
cargo test "${packages[@]}" --no-fail-fast
echo; echo "gate: changed-crate gate passed (the reviewer runs --full per merge batch)"
