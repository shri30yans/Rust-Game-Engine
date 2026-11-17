# physics-ramp-penetration: the character controller ends inside a tilted box (property test failure)

Goal: `move_character` never leaves the capsule embedded in static geometry deeper than its skin width. Done means the saved proptest case passes, the property test passes 20 times in a row with fresh seeds (`PROPTEST_CASES=2000`), and the cause is named in the report.

Read first: `AGENTS.md`; `docs/README.md` §7 D55 (follow Unreal when in doubt); `engine/physics/src/character.rs`, `engine/physics/src/world.rs`, `engine/physics/tests/properties.rs` and `properties.proptest-regressions` (the new last line is the failing case: `obstacle_x = 4.46049, angle = 0.5289006, speed = 4.8609233, use_ramp = true`, a 0.4 × 2 × 3 box rotated 0.53 rad about Z, walked into at 4.86 m/s).

## Design

1. **Find the cause first.** Reproduce with the saved case, log per tick: the desired translation, the corrected translation Rapier returns, whether autostep or snap-to-ground fired, the penetration depth (contact distance) after the move. State in the report which step puts the capsule inside the box (likely candidates: autostep climbing onto a steep face, snap-to-ground pulling into a slope steeper than `slope_limit`, or a translation applied after Rapier's correction).
2. **Fix at the cause, in the controller.** Simplest correct option wins. If a step is the cause (autostep or snap onto a surface steeper than `slope_limit`), the controller must not accept that step: for example, refuse autostep when the step's landing normal exceeds the slope limit, the same rule Unreal's `UCharacterMovementComponent::StepUp` applies (`IsWalkable` check on the landing hit). If Rapier itself returns a penetrating result for a legal move, add a depenetration pass after the move (Unreal's `ResolvePenetration`: push out along the contact normal by the depth, once, using the existing query API) and say why.
3. **Do not loosen the test** (the clearance inset stays the skin width) unless step 1 proves the overlap is within Rapier's documented `offset` and the test's inset is computed wrong; then explain with the numbers.

**Tests:** the saved case; the property test at 2,000 cases, 20 runs; one new unit test in `character.rs` for the exact cause (e.g. "autostep onto a 30° steep face is refused").

**Do not touch:** anything outside `engine/physics` except `docs/briefs/README.md` (status) and `docs/research/2026-11-11-engine-audit.md` only if a finding there changes.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
