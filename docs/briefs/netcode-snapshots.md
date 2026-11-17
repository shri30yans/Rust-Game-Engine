# netcode-snapshots: Unreal-style replication (shadow state, changed components, priority, relevancy, dormancy, reliable events), client prediction and reconciliation, remote interpolation (netcode slice 2, PLAN M2.1)

Goal: the engine can keep clients in sync with an authoritative server over a lossy link, with the local player predicted and corrected and everyone else smoothly interpolated, ready to wire into `engine-app` and a game in the next slice. Done means: over the loopback with 150 ms round trip, 5 % loss and 20 ms jitter, a test world of 24 moving players and 200 debris bodies stays in sync (client view of replicated components equals the server's for the same tick, after interpolation delay), each client's downstream stays under 40 KB/s at 30 snapshots per second, a predicted local actor corrects with no visible snap for small errors and snaps cleanly for large ones, and everything is deterministic and allocation-free per tick after warm-up.

Read first: `AGENTS.md`; `docs/ARCHITECTURE.md` §2–4 (online play), `docs/README.md` §7 D6, D7, D13, D24, D36, D54; `games/imagine/docs/PLAN.md` §3.1, §3.4 (Director network rule), §8.3; `engine/netcode/src/*` (slice 1: transport, loopback with link conditions, redundant inputs, codec, handshake); `engine/world/src/{registry,world,resource,transform}.rs` (replication classes and per-component serde ops); `engine/core` (tick, quantisation, state hash). Background reading the design follows, use it when unsure: Glenn Fiedler, "Snapshot Compression" and "State Synchronization" (gafferongames.com); Valve, "Source Multiplayer Networking" and "Latency Compensating Methods" (developer.valvesoftware.com); Overwatch GDC 2017 "Gameplay Architecture and Netcode".

## Design

**Scope:** `engine/netcode` (it may depend on `engine-world`, layer 1). Nothing in `engine/app`, kits or games changes in this brief (other briefs are editing them now); provide the APIs and prove them with tests that build their own small worlds.

**1. Replication, following Unreal's model (D55): per-connection shadow state, changed components sent whole, priority and relevancy, dormancy, a reliable event channel.**

The soak test measured 9,500 B/s per client with the previous fixed 16-bit position codec and 9,332.6 B/s with per-axis bounds-derived precision at 1 mm. In a 256 m arena, positions use 18 bits per axis; postcard varints keep the encoded transform at 28 bytes (unchanged from the previous codec).

Unreal replicates each actor's properties per connection: the server keeps, per connection and per actor, the last state that connection has received ("shadow state"), compares, and sends only the properties that differ, each one whole; lost packets cause the affected properties to be sent again; priority, relevancy and update frequency decide who gets bandwidth; dormant actors are not even considered until woken; RPCs are a separate reliable or unreliable channel. We do the same, at component granularity:
- **Packets.** Each send tick (30 Hz), one datagram per client (`MAX_DATAGRAM_BYTES`) with a packet sequence and the server tick. Clients ack with the newest received sequence plus a 32-bit field for the 32 before it, carried in every input bundle (extend slice 1's `InputBundle`, version bump, keep its redundancy).
- **Shadow state (server, per client, per entity, per replicated component):** the version (server tick of the value) the client is known to have. A component is **dirty for a client** when its current version is newer than the shadow. Each sent packet records which (entity, component, version) it carried in a fixed ring indexed by sequence; when a packet is acked, those versions become the shadow; when a packet is known lost (older than the ack window or not in the ack field), nothing happens: the component is still dirty, so it is sent again. This rule is the whole of reliability for state, with no resend timers.
- **What is written:** for each entity chosen, its dirty components, each one **complete**, encoded with the component's `NetEncode` (postcard by default; a quantised `Transform`: position 1 mm inside a world bound the game gives, smallest-three rotation at 10 bits per component). The client never needs any base version to decode: it applies a component if its version is newer than what it holds for that (entity, component), else ignores it.
- **Replication conditions** (Unreal's `COND_*`), as the component's registered class: `Replicated` (all relevant clients), `OwnerOnly` (only the owner; ownership is a `NetOwner` component defined here), `ServerOnly` (never sent); plus an `initial_only` flag for components sent once at spawn (names, team, cosmetic seed).
- **Priority and relevancy** (Unreal's `NetPriority`, `IsNetRelevantFor`, `NetUpdateFrequency`): per entity a priority and a max update rate, both component data the game sets; relevancy is a callback the game supplies (distance, its spatial grid, "always" for objectives); the engine never decides relevance. Per client, an accumulator grows each tick by priority × time since last sent, only for relevant entities with dirty components; the writer fills the packet from the highest accumulator down. An entity that stops being relevant is destroyed on that client (a close entry), and re-sent whole when it becomes relevant again.
- **Dormancy** (Unreal's net dormancy): an entity can be marked dormant by the game (resting debris, an untouched building element); dormant entities are skipped entirely in the dirty scan until a component changes, which wakes them. This keeps the per-tick cost proportional to what changes.
- **Spawns and despawns:** a spawn is an entity with every component dirty. A despawn is a close entry that is dirty until acked (same rule as components).
- **Events channel** (Unreal's reliable RPCs and multicasts): typed, game-defined events (`NetEvent` trait, postcard) in two kinds. **Reliable ordered** events (destruction "element E broke at tick T", Director stage changes, kills, score) are kept in a per-client queue and included in every packet until acked, with a sequence so the client applies each once and in order. **Unreliable** events (cosmetic hits, sounds) are sent once if there is room. Reliable events go first in each packet; a hard cap per packet keeps state flowing.
- Wire messages are added to `Message` with size caps and fuzz tests, as slice 1 does.

**2. Client application and interpolation (`src/client/`).**
- The client keeps, **per entity**, a short timeline of received states keyed by server tick (fixed capacity); a state older than the newest it holds for that entity is ignored.
- **Remote entities** are shown at `server_tick − interp_delay` (default 2 snapshot intervals, configurable); `Interpolator` returns, for a render time, the two bracketing snapshot states and the blend factor. Positions lerp, rotations slerp; non-interpolable components step. If a snapshot is missing, extrapolate at most 1 interval (configurable) then hold.
- Applying into a `World` goes through the registry ops so replicated components are written exactly as the server sent them.

**3. Prediction and reconciliation (`src/predict.rs`).**
- Generic over a game- or kit-provided trait:
  ```rust
  pub trait Predicted {
      type State: Clone + PartialEq;      // what the server corrects (pose, velocity, movement state)
      type Input: Clone;
      fn capture(world: &World, id: NetId) -> Result<Self::State, NetError>;
      fn restore(world: &mut World, id: NetId, state: &Self::State) -> Result<(), NetError>;
      fn step(world: &mut World, id: NetId, input: &Self::Input) -> Result<(), NetError>;
      fn error(a: &Self::State, b: &Self::State) -> f32;   // metres-equivalent, for smoothing
  }
  ```
- The client keeps a ring of (tick, input, predicted state). When an authoritative state for tick T arrives: compare with the stored prediction for T; if `error ≤ epsilon` drop history up to T; else restore the server state, re-run `step` for every stored input after T, and record a **visual correction offset** that decays over `correction_time` (configurable, default 100 ms) so the render does not snap; above `snap_distance` (default 2 m) snap at once.
- Only the entities the client owns are predicted; the trait is how a kit plugs its character controller in later (the FPS kit's motor step already runs on both sides, PLAN §3.6).

**4. Server side input handling.** The server applies each client's inputs at their tick using slice 1's `InputReceiver`; a missing input repeats the last one (documented) and counts as a loss metric.

**Determinism and allocation:** no wall clock (all time is ticks plus a render alpha passed in); buffers pre-sized from configuration; a counting-allocator test over 600 ticks of the 24-player scene after warm-up.

**Tests:**
- Codec: round trip and fuzz for every new message; quantisation error within its bound.
- Property: for random world changes and random loss and reordering, once the world stops changing and enough packets get through, every relevant entity's components on the client equal the server's (eventual consistency), and no component is ever replaced by an older version.
- Shadow state: an unchanged, acked component is never sent again; a component in a lost packet is sent again; a dormant entity costs nothing until woken.
- Reliable events: every reliable event is applied exactly once and in order under 5 % loss and reordering.
- Starvation: a relevant entity with dirty components is sent within a bound computed from priorities and asserted.
- Bandwidth on the soak scene: report bytes per client per second and per-entity update rates (players and debris) before and after debris goes to rest.
- The soak scene above through `loopback_pair` with `LinkConditions` (150 ms RTT, 5 % loss, 20 ms jitter): sync check, bandwidth per client measured and asserted ≤ 40 KB/s, no allocation.
- Prediction: a test `Predicted` impl (a kinematic point mover); with matching server → no corrections; with a server-side push → one correction, replayed inputs give the right final state, the visual offset decays to zero by `correction_time`; large error snaps.
- Interpolation: bracketing, missing snapshot extrapolation cap, hold.

**Do not touch:** `engine/app`, `engine/world` public API (add only if unavoidable and say why), kits, games, `engine/render`, `engine/ui`, `engine/assets`, `tools/`.

**Docs in the same change:** `docs/ARCHITECTURE.md` §1 netcode row and §4 (what slice 2 built, the numbers), `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
