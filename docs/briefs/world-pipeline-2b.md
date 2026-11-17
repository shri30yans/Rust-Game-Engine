# world-pipeline-2b: fix the blank sample arena and split the oversized scene files (2026-11-12)

This run builds on the branch `kiro/world-pipeline-2` (live level diffs, stable ids, Blender reproducibility, sample arena; 14 commits). Read that branch's diff against main first. Its review is in `docs/briefs/world-pipeline-2.md`, section "Review of the Kiro run". This brief does items 1 and 2 of that review. Items 3 to 5 (one hot-reload path, no presenter asset hook, no build-machine paths) wait for `asset-source` and are a later run.

## Design
1. **The sample arena renders blank white.**
   - `games/sandbox/game/tests/fixtures/sample-arena-{desktop,phone,lockdown}.png` are the same pure-white 640×360 image, and `games/sandbox/game/tests/headless_render_golden.rs` passes by comparing white to white.
   - Find the cause by rendering the scene and inspecting the frame: the camera inside or behind geometry, exposure, no lights, the level not loading, or the clear colour. Fix it where it is.
   - Re-record the three goldens. Look at each one and describe it in the report: what is visible and where.
   - Add a check to that test file that the frame is not uniform (luminance standard deviation above a floor) before comparing or recording. `golden-images` (running at the same time) moves every golden test onto one shared module later, so keep this check small and local.
2. **Split the files over the 500-line hard limit (AGENTS.md tenet 5)** into folders by responsibility, with no behaviour change:
   - `engine/scene/src/spawn.rs` (1,702 lines on the branch);
   - `engine/scene/src/model.rs` (1,493);
   - `engine/app/src/live_scene.rs` (1,321).
   - Each `mod.rs` only declares, re-exports and documents.
   - Each file has one responsibility and a short header, for example:
     - spawn: plan, instances, prefabs, visuals;
     - model: level, shapes, layouts, validation;
     - live scene: diff, apply, transaction.
   - Unit tests over about 150 lines move to a sibling `tests.rs`. Public paths stay the same (re-export), so callers do not change.
   - Every new file is under 300 non-test lines where the responsibility allows, and none is over 500.

Done means:
- items 1 and 2 complete;
- `scripts/gate.sh --base main` passes;
- the report lists the new files with their line counts and shows the three goldens.

Do not touch `engine/render`, `engine/ui`, `engine/assets`, `engine/platform`, or the start-up code in `engine/app/src/client.rs` beyond what the split needs. Other runs are in those.

Read first:
- `AGENTS.md`;
- `docs/briefs/world-pipeline-2.md`;
- `git diff main...HEAD`.

Docs in the same change: `docs/ARCHITECTURE.md` §1 (scene and app rows, if module names appear there).

## Gates
scripts/gate.sh --base main

## Completion

The sample level contains 390 authored entities; extraction capacity now starts from the loaded
world count plus the game's declared dynamic budget and is recalculated after successful loads.
The blank frame came from the old 256-entity extraction ceiling: extraction failed, and the headless
runner saved the clear frame. Those failures now enter the presentation error path, which records
telemetry and uses the existing log and error banner.

Cooked prefab references must resolve to both an index entry and an existing prefab file. IMAGINE's
committed index already named `turret_base.prefab.ron`, while that cooked file was absent; the
generated prefab is now committed and the Blender reproducibility gate passes.

The goldens have telemetry off and compare every pixel. Their camera looks down from the interior
walkway across the floor and perimeter. Open shows the central mover raised; lockdown lowers it out
of sight behind the near floor edge. The phone image includes only the lower-right jump ring as UI.
`scripts/gate.sh --base main` passes after merging `main`.
