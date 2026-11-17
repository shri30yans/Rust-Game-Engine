# vk-1-gpu: `engine-gpu`, our Vulkan layer, proven headless on the Mac and both phones (D82)

Goal: a safe Rust API over Vulkan that the renderer can be ported onto. Done means:
1. `engine/gpu` → `engine-gpu` exists at layer 0 and is the only crate naming `ash`.
2. Its GPU tests render offscreen (a depth-tested, textured, instanced mesh through a two-subpass render pass), read the image back, and compare it with expected pixels, with Vulkan validation and synchronization validation on and reporting nothing.
3. Those tests pass on the M5 through KosmicKrisp and on both test phones through `scripts/android-test.sh engine-gpu`.
4. `tools/cook` compiles Slang shaders to SPIR-V with reflection into the content pack.
5. `scripts/gate.sh` passes.

This brief lands on the `vulkan` branch, not on `main` (D82: main never has two renderers). It changes no existing crate except `tools/cook`, `engine/assets` (one shader record type) and the workspace files, so it can run in parallel with anything else.

Read first:
- `AGENTS.md` (rule 4 and its D82 exception);
- `docs/README.md` §7 D38, D72, D73, D82;
- `docs/ARCHITECTURE.md` §1, §2.1;
- `engine/core/src` (`Slots<T>`), `engine/assets/src` (pack records), `tools/cook/src`;
- `engine/render/src/{targets.rs, gfx/setup.rs, gfx/pipelines/}` to see which GPU features the renderer needs today.

## Design

- **Layering.** `engine/gpu` at layer 0. Allowed dependencies: `engine-core` is layer 0 too, so `engine-gpu` must not depend on it; add `{ "from": "engine-gpu", "to": "engine-core" }` to `same_layer_allowed` in `tools/check-layers/layers.json` with a D82 note (generation-checked `Slots<T>` must not be duplicated, tenet 3). Libraries: `ash`, `ash-window`, `raw-window-handle`, `gpu-allocator`, `thiserror`, `bytemuck`, `serde`. Add `{ "pattern": "engine/gpu", "layer": 0 }`.
- **`unsafe`.** The crate root has `#![deny(unsafe_op_in_unsafe_fn)]` and `#![deny(clippy::undocumented_unsafe_blocks)]`; `unsafe` is allowed only in modules under `src/vk/`. Every public item is safe to call. Handles cannot outlive the device (they are ids into the device's slot tables) and destruction is deferred until the GPU has finished the frames that used them.
- **Files** (each one job, under 300 lines where possible):
  - `lib.rs`: header with the invariants above; re-exports.
  - `error.rs`: `GpuError` (`Unsupported { missing: Vec<RequiredFeature> }`, `OutOfMemory`, `DeviceLost`, `SurfaceLost`, `Validation(String)` in tests, `InvalidHandle`, `Shader { name, reason }`).
  - `caps.rs`: `GpuCaps` (vendor, device id, driver version, name, limits used by the renderer, timestamp period, formats: ASTC 4×4, ETC2, BC7, `B10G11R11_UFLOAT` as colour attachment, depth formats, and whether a lazily allocated memory type exists) and `RequiredFeature` with the fixed Vulkan 1.1 list the renderer needs. Checked once in device creation.
  - `vk/instance.rs`: loader, instance, validation and synchronization validation layers plus a debug messenger in debug builds and tests; messages go to a `ValidationLog` that tests read.
  - `vk/device.rs`: physical device choice (discrete > integrated > other; first that meets the required list), one graphics+compute queue, one transfer queue if separate.
  - `vk/memory.rs`: `gpu-allocator`; transient attachments take a lazily allocated memory type when `GpuCaps` has one.
  - `resources.rs` (safe) and `vk/resources.rs`: `Buffer`, `Texture`, `Sampler` handles; descriptors `BufferDesc { size, usage: BufferUsage, memory: MemoryKind }`, `TextureDesc { size, format: Format, mips, layers, samples, usage: TextureUsage, transient: bool }`. Our own `Format` enum: only the formats we use.
  - `render_pass.rs` and `vk/render_pass.rs`: `RenderPassDesc { attachments: [AttachmentDesc { format, samples, load: Load, store: Store, transient }], subpasses: [SubpassDesc { colour, input, depth, resolve }], dependencies }` → cached `RenderPass` and framebuffers. Build-time validation: an input attachment must be written by an earlier subpass of the same pass; a transient attachment must have `Store::DontCare`.
  - `shader.rs`: `ShaderModuleData { name, stage, spirv: Vec<u32>, reflection: ShaderReflection }` (serde) and `ShaderReflection` (descriptor bindings per set, push-constant ranges, vertex inputs, specialization constant ids). This type is what `tools/cook` writes and `engine-assets` loads.
  - `pipeline.rs` and `vk/pipeline.rs`: graphics and compute pipelines from `ShaderModuleData` plus fixed state and specialization constants; layouts built from reflection; the pipeline cache loaded from and saved to bytes the caller stores (`engine-platform` config store, in vk-2).
  - `descriptors.rs` and `vk/descriptors.rs`: sets by update rate; per-frame pools reset at frame start.
  - `commands.rs` and `vk/commands.rs`: `CommandList` (`Send`): render pass begin, next subpass, end; bind pipeline and sets; push constants; draw, draw indexed, draw indexed indirect; dispatch; copies; explicit barriers (`Barrier::{Buffer, Texture}` with our own access and layout enums); timestamps; debug labels.
  - `frame.rs` and `vk/frame.rs`: two frames in flight; fences and binary semaphores; acquire, submit, present; deferred deletion; a per-frame staging ring for uploads.
  - `surface.rs` and `vk/swapchain.rs`: surface from raw window and display handles; FIFO present; recreation on resize and on surface loss. A lost or zero-size surface is a state, not an error (D73 lifecycle).
- **Shaders in cook.** `tools/cook/src/shaders/` compiles every `.slang` file under a shader root to SPIR-V with `slangc` (from the LunarG Vulkan SDK; README §3 setup gains it), reads Slang's reflection JSON into `ShaderReflection`, and writes `ShaderModuleData` records under `engine/shaders/<name>` in the game's pack. A missing `slangc` is a cook error naming the setup step. For this brief the shader root is `engine/gpu/tests/shaders/` (test shaders only); vk-2 points it at `engine/render/shaders/`.
- **Android test runner.** `scripts/android-test.sh <crate>` builds the crate's test binaries for `aarch64-linux-android` with the NDK linker (same as `build-android.sh`), pushes them and the validation layer `.so` from the NDK to `/data/local/tmp`, runs them with the layer enabled, and pulls the output.
- **Decisions already made (do not change):** Vulkan 1.1 baseline with a required-feature list; render passes with subpasses (no dynamic rendering); no bindless; no timeline semaphores; `gpu-allocator`; Slang; compile-time backend choice.
- **What gets deleted:** nothing on this branch yet.

Tests (GPU tests are skipped with a printed reason when no Vulkan device exists):
- Device creation reports `Unsupported` naming the missing features (unit test of the check against a fixture `GpuCaps`).
- Render pass validation: input attachment not written earlier, transient with `Store::Store`, mismatched sample counts.
- Offscreen draw: instanced textured cubes with depth, two subpasses (the second reads the first's colour as an input attachment and tonemaps), readback matches expected pixels at fixed points; validation log empty.
- Transient attachment: depth is transient and lazily allocated where available; the image still matches.
- Deferred deletion: a texture dropped while in flight is destroyed only after its frame's fence.
- Pipeline cache: save, reload, and build the same pipeline from cached bytes.
- Specialization constants select two code paths in one shader.
- A steady loop of 300 offscreen frames allocates nothing on the CPU after warm-up (counting allocator).
- Cook: a Slang file compiles to SPIR-V whose reflection lists the expected bindings; a syntax error names file and line.

Do not touch: `engine/render`, `engine/platform`, `engine/app`, games, kits.

When a detail is not specified: pick the simplest correct design that keeps one code path, state it in the report, and continue. Stop only if the goal itself cannot be met.

Docs in the same change: this brief's status; ARCHITECTURE §1 `engine/gpu` row; README §3 setup (Vulkan SDK with KosmicKrisp and `slangc`); D82 gains the measured result line (which phones and drivers passed).

## Gates
scripts/gate.sh
