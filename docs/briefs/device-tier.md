# device-tier: the engine picks the starting quality tier from the device class and the GPU (audit C1, D19, D33)

Goal: a phone never starts on the desktop setting. Done means:
1. `engine-app` builds the device profile, including the GPU profile from the real adapter, in one place.
2. The starting tier comes from a tested table.
3. The player's saved quality choice overrides it.
4. Games stop calling `DeviceProfile::detect` themselves.
5. The four gates pass.

Starts after `asset-source` merges. That brief moves start-up into the engine: the engine builds the `DeviceProfile` (class and touch from the platform) before the game is built, and games stop calling `DeviceProfile::detect`. This brief adds the GPU part and the tier on top of that one start-up path.

**The real API on main (checked 2026-11-12, after render-high-black merged):**
- Render settings are `render::Settings` (`engine/render/src/gfx/config.rs`) with a `taa: bool`. `RenderTier::settings()` (`tier.rs`) builds one per tier. The prepass is not a setting: `RenderFeatures` derives `Prepass::{Off, On{gtao, taa}}` from `Settings` when the frame plan is built, so TAA or GTAO bring the prepass with them. Keep that: the starting tier is a `Settings` value, produced through `RenderTier::settings()` plus the table's MSAA choice.
- `Gfx::set_settings(Settings) -> Result<(), RenderError>` already validates the new plan before changing anything. Apply the saved override through it.
- `GpuProfile` already exists (`engine/platform/src/profile.rs`, `GpuProfile::from_adapter(adapter, known_bad)` via `Gpu::profile`), with name, vendor, device, device type, backend, features, limits and the known-bad match. There is no driver field and none is needed: known-bad entries match on vendor and device. Do not add one.

Read first:
- `AGENTS.md`;
- `docs/research/2026-11-11-engine-audit.md` C1;
- `docs/README.md` §7 D19, D33, D35, D54;
- `engine/platform/src/profile.rs` (`DeviceProfile::detect`, `GpuProfile`, the known-bad GPU list);
- `engine/render/src/{tier.rs, gfx/config.rs}`;
- `engine/app/src/client.rs` (`ClientApp::new`, `init`, `run_client`);
- `games/imagine/game/src/{lib.rs, settings.rs}` and `games/sandbox/game/src/lib.rs`.

## Today
`games/imagine/game/src/lib.rs` and `games/sandbox/game/src/lib.rs` each call `DeviceProfile::detect(class, None)`. The GPU profile is never passed, so the known-bad list is never checked, and every device starts on the Medium default (MSAA 4×, 2048² shadows, 32 lights).

## Design
- **One owner.** `engine-app` builds the profile:
  - The CPU part, the device class and touch come from the engine's start-up path (asset-source), before the game is built.
  - When `ClientApp::init` receives the GPU, it builds `GpuProfile` with `Gpu::profile(known_bad)` and attaches it with one method that returns the completed profile.
  - The known-bad list is engine data (the engine's content), not a game's choice.
- **Starting tier**, a pure function in `engine-render` or `engine-app` (wherever the tier type lives): `starting_tier(RenderClass, &GpuProfile) -> Settings`.
  - `DeviceClass` stays as it is (Desktop, Handheld, Web, Server). Laptops are Desktop, and the GPU kind separates them. Phones and tablets are Handheld.
  - Server never renders (D33). Make that unrepresentable: `RenderClass { Desktop, Handheld, Web }` and `DeviceClass::render_class() -> Option<RenderClass>`, which is `None` for Server. Only a `RenderClass` can reach `starting_tier` or `Gfx`.
  - The GPU kind is one of: known-bad, integrated, discrete, Apple, unknown. The table, for every pair:
    - Known-bad on any class: Low, MSAA off.
    - Handheld (any other kind): Low, MSAA 4× (D54: MSAA rather than TAA on phones).
    - Desktop, integrated or unknown: Medium.
    - Desktop, discrete or Apple: High.
    - Web, integrated or unknown: Low, MSAA 4×.
    - Web, discrete or Apple: Medium. WebGPU stays conservative until it is measured.
  - Encode these rules as a table, not as nested ifs.
- **Override.** If the game has a saved quality choice, it wins. The engine applies the game's override after the starting tier through one setter that already exists for settings changes. IMAGINE's settings store "Auto" or a tier, and Auto means the starting tier.
- **Run-time tier change goes through the one setter.** Do not add `set_tier`: a tier change is `gfx.set_settings(tier.settings())`, the same call the override and the settings menu use (Unreal: every scalability change goes through `UGameUserSettings::ApplySettings`). Make `set_settings` rebuild only what differs between the old and new frame plan (MSAA, prepass, TAA, GTAO, shadow size), and make it a no-op for equal settings. Test that High → Low → High leaves each tier's golden unchanged, and that an equal-settings call rebuilds nothing.
- **One tier type.** IMAGINE has its own `QualityTier` enum in `games/imagine/game/src/settings.rs`, duplicating the engine's `RenderTier`. Replace it with the engine type: the setting is `Auto | Fixed(RenderTier)`, saved under the same key with a migration for old saved values. IMAGINE's quality setting, now disabled, is enabled and calls `set_settings`.
- **Report.** Log the chosen tier and why once at start-up (class, GPU name, rule). Expose it in `FrameStats` or the dev overlay data so screenshots show it.
- The D19 start-up benchmark that moves the tier up or down is not part of this brief. Add it to `docs/briefs/README.md` as a `queued` row named `tier-bench`.

## Tests
- A table test: every `RenderClass` × GPU kind (known-bad, integrated, discrete, Apple, unknown) gives the stated tier and MSAA; Server has no render class.
- `GpuProfile` from adapter info: a fixture adapter info for a known-bad GPU is flagged.
- The override wins over the starting tier, and Auto follows it.
- A headless client test: the profile handed to the renderer contains the GPU profile, never `None`, after `init`.
- A grep-style test or `check-layers` rule is not needed. Removing the game call sites is checked in review.

**Do not touch:** `engine/netcode`, `engine/physics`, `engine/ui`, `kits/`.

**Docs in the same change:**
- `docs/research/2026-11-11-engine-audit.md` C1 status;
- `docs/RENDERING.md` §7 (the starting-tier table);
- `docs/ARCHITECTURE.md` §1 (platform, render and app rows);
- the decision written into `docs/RENDERING.md` §15 (the engine owns the device profile and the starting tier; Auto plus override);
- `docs/briefs/README.md` status `review`.

## Gates
scripts/gate.sh
