# Workflow: how work gets done

## 1. Roles

| Who | Does |
|---|---|
| **Owner** (you) | decides direction, picks the title and target hardware, models final characters and guns in Blender, approves merges, playtests |
| **Claude** (Claude Code) | researches, designs each task, writes the brief, builds it, tests it, reviews its own diff against the tenets, keeps the docs true |

No other coding agent is used (owner, 2026-11-18). When two tasks touch different files, Claude may run them in parallel with its own subagents, each in its own git worktree, and reviews each diff before it merges.

## 2. The loop for one task

1. **Pick the next task** from the current build step of the game driving the work (engine work: the order of work in [RENDERING.md](RENDERING.md) §19 and the engine test scenes in [PERFORMANCE.md](PERFORMANCE.md) §3; a game's work: its plan, for IMAGINE [PLAN.md](../games/imagine/docs/PLAN.md) §9).
2. **Write the brief** in `docs/briefs/NAME.md` using the template below, and add it to [briefs/README.md](briefs/README.md) with status `ready`. A brief is the design written down before code: small fixes can skip it, anything that adds a type, a crate or a decision cannot.
3. **Build** on a branch `NAME` (or on `main` for a small fix the owner asked for), with tests in the same change.
4. **Gate:** `scripts/gate.sh` must pass.
5. **Review the whole diff** against the tenets before calling it done: design drift (new abstractions, moved responsibilities, duplicated logic), unwired code, weak tests, files over the size limit.
6. **Merge** only when the owner says so: `git merge --no-ff NAME`, then `scripts/gate.sh --full` and `scripts/build-android.sh imagine` on main, then remove the worktree (each worktree's target folder is 10–17 GB).
7. **Update the docs** in the same change: the brief's status, the crate's state in ARCHITECTURE §1, and any decision in the doc that owns its topic.

## 3. Brief template

```markdown
# NAME: one-line summary (build step N / GXn)

Goal: one sentence. Done means: observable result.

Read first: AGENTS.md, the docs sections that apply, the crate headers involved.

## Design
- Layering: crate, layer, allowed dependencies, the line to add to layers.json.
- Files: each file with its types and function signatures.
- Decisions already made (do not change them).
- What gets deleted.

Tests: unit, property, determinism, no-allocation, GPU (skipped with a printed reason when no adapter).

Do not touch: other crates and files.

When a detail is not specified: pick the simplest correct design that keeps one code path, write it into the brief, and continue. Stop and ask the owner only if the goal itself cannot be met.

## Gates
scripts/gate.sh
```

## 4. Gates (definition of done)

One script, two modes ([scripts/gate.sh](../scripts/gate.sh)):

```bash
scripts/gate.sh
```

Each change, before it is called done: `cargo fmt --check`, `check-layers`, then clippy (`--all-targets -D warnings`) and tests for the crates the branch changed. A change to the workspace root (`Cargo.toml`, `Cargo.lock`, `.cargo/`, `clippy.toml`, `layers.json`) runs the full gate.

```bash
scripts/gate.sh --full
```

Once per merge batch on main, together with `scripts/build-android.sh imagine`. This catches what a change breaks in crates that depend on it. Branches merged in one batch are reviewed one by one before it, so a failure points at a small set of branches. It is fixed at its cause before the next batch.

**Why.** Five runs each testing the whole workspace at once compiled the engine five times over on a shared CPU, and a single run sat in its test step for over twenty minutes. Unreal works the same way: a local preflight on what changed, and the full build on the farm.

Plus, for rendering work: `cargo run --release -p bench-gate -- --device apple-m5`, the phone bench, and the goldens ([PERFORMANCE.md](PERFORMANCE.md) §3–4).

### Recording PNG goldens

PNG screenshot tests use engine-render's test-only golden feature. Every golden declares at least one named content expectation, such as a region's luminance range or a minimum percentage of pixels that differ from the clear colour. A shared uniform-frame check rejects images whose luminance standard deviation is below 0.25 byte levels or whose dominant pixel colour covers at least 99.9% of the frame. These checks run before comparison and before recording.

Set GOLDEN_RECORD to a name pattern to record only matching images; * matches any sequence of characters. The rendered image is recorded only after the uniform-frame check and every declared content expectation pass.

```bash
GOLDEN_RECORD='imagine/*' cargo test -p imagine --test headless_render_golden
GOLDEN_RECORD='sandbox/main' cargo test -p sandbox --test headless_render_golden
```

Without a matching pattern the same test compares against the committed reference. A mismatch reports the reference path and measured mean and maximum channel errors, and writes the rendered image beside the reference as <name>.actual.png.

## 5. Git

- `main` is the integration branch. Task work lands on a branch named after its brief and is merged by hand. The Vulkan move lands on a `vulkan` branch first ([RENDERING.md](RENDERING.md) §2).
- Commit only when the owner asks. Binary sources go in Git LFS ([CONTENT.md](CONTENT.md) §3).

## 6. Keeping the docs canonical

- **One topic, one file.** The map is in [README.md](README.md). If two files say the same thing, one of them links to the other instead.
- **Code and docs change together.** A merged task updates its brief status, the ARCHITECTURE crate table and any decision.
- **Decisions are never silent and never a separate log.** A new or changed choice is written into the doc that owns its topic, as the current design with Decision, Why and Rejected ([PRINCIPLES.md](PRINCIPLES.md) §4). The old version stays in git history.
- **Branches stay narrow.** A task branch builds on the main it started from. When briefs overlap, the brief names the overlap and keeps each side's change narrow; they are integrated at merge.
- **A brief names where its decision goes** (the doc and section). Decisions are no longer numbered; old numbers D1–D83 resolve through [README.md](README.md) §7.
- **Open questions** are marked **[Q]** where they arise and listed in README §5.
