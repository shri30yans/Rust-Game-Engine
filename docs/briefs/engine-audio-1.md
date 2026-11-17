# engine-audio-1: the sound engine, slice 1: mixer buses, 3D one-shots, voice limits, wired into the client (PLAN M1.3)

Goal: the engine plays game sounds through one small API, and IMAGINE has shots (near and far), footsteps, hits and kills audible in a match. Done means: (1) a new crate `engine-audio` wraps kira (D12) behind engine types; (2) `engine-app`'s client owns the audio engine and hands the presenter a way to play sounds each frame; (3) IMAGINE plays placeholder sounds from its own content for its events; (4) tests run without an audio device; the four gates pass.

Read first: `AGENTS.md`; `docs/README.md` §7 D12 (kira), D24, D53, D55; `docs/ARCHITECTURE.md` §0–1 (layer of `engine/audio`), §2 (client loop); `games/imagine/docs/design/MOTION_SOUND.md` (buses, ducking, formats, spatial); `engine/app/src/{client,lib}.rs` and the `Presenter` trait; `games/imagine/game/src/{present,feedback,hearing}.rs` (what events exist today).

## Design

Follow Unreal's audio model in its simplest form (D55): **sound assets** loaded once, **one-shot playback** at a world position with attenuation, a **sound class / submix tree** with volumes, **concurrency limits** per sound and globally, and a **listener** that follows the camera.

**1. Crate `engine/audio` → `engine-audio`** at the layer `docs/ARCHITECTURE.md` gives it (add to the workspace and `tools/check-layers/layers.json`). Depends on kira (latest release; say which), glam, thiserror. No `unsafe`.
- `Audio` owns kira's `AudioManager`; `Audio::new(backend)` where the backend is the real device or kira's mock backend (tests, headless, servers). If no device is available at startup, the client runs silent and logs once (a missing sound card never stops the game).
- **Buses** (kira tracks) as an engine enum the game cannot misspell: `Bus::{Master, Music, Sfx, Weapons, Foley, Ui, Voice}` with the tree from MOTION_SOUND (Weapons, Foley and Ui under Sfx). `set_volume(bus, Volume)` with `Volume` a validated newtype (0..=1 linear, converted to dB inside). Ducking stays out of this slice.
- **Sounds:** `SoundBank` loads Ogg Vorbis or WAV files from paths the game passes (decoded on load); `SoundId` is a typed index; a `SoundDef { file, bus, volume, pitch_jitter, max_voices, priority, spatial: Option<Attenuation> }` the game builds from its data. `Attenuation { min_distance, max_distance }`, linear-in-dB falloff between them.
- **Playing:** `audio.play(id, Emit::At(Vec3) | Emit::Ui)`; `audio.set_listener(position, rotation)` once per frame. Spatial sounds use kira's spatial scene (listener + emitter) for panning and attenuation.
- **Voice limits:** per `SoundDef.max_voices` and a global cap (default 32): when full, the lowest-priority, then oldest, voice of that sound is stopped (Unreal's "stop oldest" concurrency rule). Bookkeeping in pre-sized arrays: no allocation per `play` after warm-up in our code (kira's own is measured and reported, not asserted).
- **Pitch jitter** uses a small deterministic RNG seeded at creation (no thread-local RNG, rule 5).

**2. Client wiring (`engine-app`).** The client creates `Audio` at startup (real device on desktop and Android, mock on headless/server). The `Presenter` trait gains one hook with a default no-op: `fn play_audio(&mut self, world: &World, audio: &mut Audio) -> Result<(), AppError>`, called once per frame after the tick and before drawing, plus the listener set from the camera the presenter returns. A settings hook: `Presenter` may report bus volumes; the engine applies only changes.

**3. Placeholder sounds (IMAGINE).** No sound assets exist yet. Add `tools/sfx-gen` (zone `tools`), a tiny deterministic synthesiser that writes WAV files: rifle shot near (noise burst + low thump, 0.25 s), rifle shot far (low-passed, 0.6 s tail), footstep (short filtered thump, 3 variants), hit tick (UI blip), kill confirm (two-tone blip), dry fire click. Output to `games/imagine/content/audio/placeholder/` (WAV is in Git LFS; commit the outputs). They are honest placeholders to be replaced (owner preference); the generator is not a feature of the engine.

**4. IMAGINE events → sounds** in a new module `games/imagine/game/src/audio.rs`, data in `games/imagine/data/sounds.ron` (sound ids → file, bus, volume, voices, attenuation), loaded and validated like `tuning.ron`, embedded on Android:
- player and bot shots: near version within 15 m of the listener, far version beyond;
- footsteps for every grounded moving actor, one per stride distance (data), from the kit's locomotion state;
- hit marker and kill confirm for the local player (Ui bus);
- read from what exists today (`DamageLog`, feedback events, weapon fire state); do not add a new event system here (the per-tick event log of PLAN §3.1 is a separate brief).
- Settings menu: one new row "Volume" (master), saved with the existing settings file.

**Tests:**
- Mock backend: playing a sound starts a voice; voice limit stops the oldest lowest-priority voice; global cap respected; `Volume` rejects NaN and out-of-range; unknown `SoundId` is a typed error.
- `sounds.ron` validation: missing file, bad range → typed error with the file and field.
- IMAGINE headless match of 600 ticks with the mock backend: the expected number of shot and footstep plays for a scripted run, and the same count on a second run (deterministic).
- `tools/sfx-gen` is deterministic (two runs, byte-identical files).

**Do not touch:** `engine/netcode`, `engine/assets`, `tools/cook`, `engine/render`, `engine/physics`, `kits/fps` (other runs are editing them). In `games/imagine/game/src/present.rs` add only the hook call and the volume row.

**Docs in the same change:** `docs/ARCHITECTURE.md` §1 audio row (built: what slice 1 has); `docs/README.md` §2–3 if a new tool or setup step; `games/imagine/docs/PLAN.md` M1.3 status; `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
