# render-perf-2: front-to-back opaque order, light tiles past 32 lights, a smaller HDR target where the GPU allows it (audit H7, M1, M4)

Three open findings from `docs/research/2026-11-11-engine-audit.md` cost the most on phone GPUs (Mali and Adreno), which pay the full fragment shader for hidden pixels and pay for every byte moved.

Done means:
1. Opaque bins draw nearest first.
2. The light tiles support more than 32 lights, with each light projected once.
3. The main colour and bloom targets use `Rg11b10Ufloat` where the adapter can render to it, and `Rgba16Float` elsewhere.
4. Each change is measured, and the four gates and bench-gate pass on three runs.

Starts after `render-alloc-0` merges: all three edit `engine/render`.

Read first:
- `AGENTS.md`;
- `docs/research/2026-11-11-engine-audit.md` H7, M1, M4 and A.3.1;
- `docs/RENDERING.md` §6 and §6.2 (bins and sort keys);
- `docs/README.md` §7 D54, D55;
- `engine/render/src/{tier.rs, gfx/, targets.rs}`;
- `engine/render/shaders/{tiles.wgsl, main.wgsl}`;
- `tools/bench-gate`.

## Design

**1. Front-to-back opaque order (H7).**
- The opaque sort key gets a coarse depth: the view distance of the bin's nearest visible instance, quantised to 16 bits. It is computed in the existing cull loop, with no extra pass over instances.
- Key order is pipeline, then depth, then material, then mesh, as in Unreal's mobile renderer for tile GPUs. A pipeline switch costs more than overdraw, so pipeline stays first.
- Transparent work keeps back to front.
- Measure the prepass on and off for Low and Medium in the bench, and report the numbers. Turn the prepass on for Medium only if it is faster there. Phones decide later, from the C2 measurements; leave a bench case they can run.
- **Test:** the sort-key order on a fixture of bins at known distances.

**2. Light tiles (M1).**
- Add a first compute pass that projects every light once into a screen rectangle and a depth range, stored in a buffer.
- Tiles test against the rectangles. Each tile's light list is an index list with a fixed per-tile capacity from tier data (for example 64), not a 32-bit mask.
- Overflow is counted in `FrameStats`, never silent. When a tile overflows, it keeps the nearest lights.
- Keep the storage-buffer count per stage within `downlevel_defaults` (4 per stage), using the group 0 layout. If this needs a fifth storage buffer, stop and report it.
- **Tests:**
  - 100 lights on the desktop tier light the right pixels (a golden);
  - an overflowing tile keeps the nearest lights and reports the overflow;
  - the old 32-light golden is unchanged.

**3. HDR format (M4).**
- `Gfx` picks the HDR colour format once at creation:
  - `Rg11b10Ufloat` when the adapter reports it renderable and blendable, through `TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES` if available, or the format features otherwise;
  - `Rgba16Float` otherwise.
- TAA history stays `Rgba16Float`; RENDERING.md already records why.
- One format choice is stored in `Gfx` and used by every pipeline that renders to the target. No per-pass guess.
- **Tests:** the format choice from fixture feature sets; the goldens stay within the TAA noise bound on both formats.

**4. Measure.**
- Run bench-gate three times before and three times after, and report every number: GPU frame, wall p50 and p99 for each case.
- Add the new cases (Low and Medium with the prepass on and off, and 100 lights), recorded with a note.

**Do not touch:** `engine/netcode`, `engine/physics`, `engine/scene`, `engine/ui`, `kits/`, `games/` (except re-recording goldens, with the diff explained).

**Docs in the same change:**
- `docs/research/2026-11-11-engine-audit.md` (H7, M1 and M4 status, with the numbers);
- `docs/RENDERING.md` §6 and §6.2;
- a decision in `docs/README.md` §7 (the sort key, the tile light lists, the HDR format choice);
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
