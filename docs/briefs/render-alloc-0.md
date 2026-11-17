# render-alloc-0: a steady full frame allocates nothing (AGENTS rule 6)

Measured on main at c37781b (render-assets-2 follow-up): a textured scene allocates about 52,000 times over 120 steady frames of `Gfx::render`, about 440 per frame. The existing no-alloc test covers only the scene update and cull path, not the frame. render-assets-2 left an ignored test, `engine/app/tests/full_render_allocations.rs`, that names this count.

Done means:
1. That test is no longer ignored and asserts 0 allocations over 120 steady frames, after warm-up, on the desktop tier and the phone tier, with TAA on and off.
2. A second scene runs the same test: the IMAGINE match frame with the HUD.
3. The four gates pass.
4. bench-gate passes three times, with all three runs reported.

Read first:
- `AGENTS.md` (rule 6);
- `engine/render/src/gfx/` (the frame path, passes and staging);
- `engine/app/tests/no_alloc.rs`;
- `engine/app/tests/full_render_allocations.rs`;
- `engine/ui` (the draw-list build).

## Design

**1. Find every site, with evidence.**
- Extend the counting allocator in the test support with a site-capturing mode, behind a test-only feature. When enabled, it records `std::backtrace::Backtrace` for each allocation after warm-up and prints the top 20 sites by count.
- Do not use Instruments or any tool that needs administrator rights.
- List the sites in the report before fixing anything.

**2. Fix at the cause.** Expected kinds:
- `Vec` built per pass or per frame: own it in the pass and clear it, never rebuild it;
- wgpu command encoders and descriptors with owned labels: use `&'static str` labels;
- `format!` in labels or debug names;
- bind groups created per frame: cache them by the resource generation and rebuild only when a resource changes;
- staging-belt or buffer-mapping callbacks: reuse;
- UI draw-list and text-layout buffers: keep them across frames.

Allocation inside wgpu or the driver that we cannot remove:
- prove it with the site list;
- then measure it in a separate counter that excludes those named frames, with a written reason in the test;
- and record it as a decision in `docs/README.md` §7.
Excluding a site without that evidence is not allowed.

**3. Keep it fixed.** Both tests run in `cargo test --workspace`, so any new per-frame allocation fails CI.

**Do not touch:** `engine/netcode`, `engine/physics`, `engine/scene`, `kits/`, or game logic.

**Docs in the same change:**
- `docs/RENDERING.md` (the frame allocates nothing, and how that is tested);
- `docs/research/2026-11-11-engine-audit.md` (the allocation finding, if one is listed);
- a decision in `docs/README.md` §7 for any wgpu-side exception;
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
