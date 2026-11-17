# netcode-loopback: `netcode` crate, slice 1 (transport, loopback, input frames, protocol)

Goal: a new engine crate `netcode` (package `engine-netcode`) with the transport abstraction, a deterministic in-process loopback transport that simulates latency, jitter, loss and reordering, the tick-stamped `InputFrame` with redundancy, and a versioned, size-capped wire codec. No sockets, no snapshots, no prediction yet (later slices).

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §2 and §4, `engine/core/src/lib.rs`, `engine/core/src/tick.rs`, `engine/core/src/rng.rs`, `engine/core/src/quantise.rs`, `engine/world/src/net_id.rs`.

## Design

Layering: `netcode` depends only on `engine-core`, `serde`, `postcard`, `thiserror` (+ `proptest` dev). Add it to workspace `members` and `[workspace.dependencies]` as `engine-netcode = { path = "engine/netcode" }`. Add `{ "pattern": "engine/netcode", "layer": 2 }` to `tools/check-layers/layers.json`. It must not depend on `world`, `physics`, `render` or `platform`.

Platform rule (D24, ARCHITECTURE §0): netcode serves every game, so it knows no game's inputs or ids. Input contents are a type the game (or a kit) supplies; the shooter input (move, yaw, pitch, fire, jump…) belongs to `kits/fps_controller` later, not here. Tests define their own small payload type.

Files:
- `engine/netcode/src/lib.rs`: module header (job + invariants: no wall clock, no OS entropy, time advances only through `Tick`, every decode is bounded), re-exports.
- `engine/netcode/src/error.rs`: `NetError` (thiserror): `MessageTooLarge { kind, size, cap }`, `Decode`, `VersionMismatch { ours, theirs }`, `WrongGame { ours, theirs }`, `PayloadTooLarge { size, cap }`, `ChannelClosed`, `InvalidLinkConditions(&'static str)`.
- `engine/netcode/src/protocol.rs`:
  - `pub struct ProtocolVersion(u16)` with `pub const CURRENT: ProtocolVersion` (the engine's wire format).
  - `pub struct GameId(u64)` and `pub struct GameVersion(u32)`: handed in by the game; `pub struct GameProtocol { id: GameId, version: GameVersion }`. A client of another game or game version is refused (rule 7).
  - `pub enum Channel { Reliable, Unreliable }`.
  - `pub enum Message<I: InputPayload> { Hello { version: ProtocolVersion, game: GameProtocol, client_nonce: u64 }, Welcome { version: ProtocolVersion, client_index: ClientIndex }, Reject { reason: RejectReason }, Inputs(InputBundle<I>) }` with serde derives. `RejectReason` enum: `VersionMismatch`, `WrongGame`, `ServerFull`.
  - `pub struct ClientIndex(u8)`.
  - Size caps per message kind as named constants (`MAX_HELLO_BYTES`, `MAX_INPUTS_BYTES`, ...) and `pub const MAX_DATAGRAM_BYTES: usize = 1200`.
- `engine/netcode/src/codec.rs`: `pub fn encode<I: InputPayload>(msg: &Message<I>, buf: &mut Vec<u8>) -> Result<(), NetError>` (clears and reuses `buf`, rejects if over the cap for that kind) and `pub fn decode<I: InputPayload>(bytes: &[u8]) -> Result<Message<I>, NetError>` (rejects input longer than `MAX_DATAGRAM_BYTES` before parsing). postcard.
- `engine/netcode/src/input.rs`:
  - `pub trait InputPayload: Serialize + DeserializeOwned + Clone + PartialEq + Debug { const MAX_BYTES: usize; fn validate(&self) -> Result<(), NetError>; }`: what the game sends each tick. `MAX_BYTES` bounds its encoded size (checked in `encode`); `validate` runs on every decoded payload so a game can reject impossible values.
  - `pub struct InputFrame<I: InputPayload> { tick: Tick, payload: I }`, fields private, built by `InputFrame::new(tick, payload)` which calls `validate`.
  - Tests use a payload like `struct TestInput { move_x: i8, buttons: u8 }` defined in the test files, never in `src/`.
  - `pub struct InputBundle<I>` holding the newest frame plus up to `INPUT_REDUNDANCY = 4` previous frames, ticks strictly descending and contiguous. Built only by `InputHistory::bundle()`.
  - `pub struct InputHistory<I>` (ring buffer, capacity `INPUT_HISTORY = 64`, pre-allocated): `push(InputFrame<I>) -> Result<(), NetError>` (ticks must strictly increase), `bundle() -> Option<InputBundle>`.
  - `pub struct InputReceiver` on the server side: `accept(&InputBundle<I>) -> impl Iterator<Item = InputFrame<I>>`, which yields only frames newer than the last accepted tick, in tick order (dedupes the redundancy).
- `engine/netcode/src/transport.rs`: `pub trait Transport { fn send(&mut self, channel: Channel, bytes: &[u8]) -> Result<(), NetError>; fn recv(&mut self, out: &mut Vec<u8>) -> Result<Option<Channel>, NetError>; }`. `recv` writes one payload into `out` (cleared first) and returns its channel, or `None` when nothing is ready.
- `engine/netcode/src/loopback.rs`:
  - `pub struct LinkConditions { latency_ticks: u32, jitter_ticks: u32, loss_per_mille: u16, reorder_per_mille: u16 }` with a validating constructor (`loss`/`reorder` ≤ 1000) and `LinkConditions::PERFECT`.
  - `pub fn loopback_pair(a_to_b: LinkConditions, b_to_a: LinkConditions, seed: u64) -> (LoopbackEnd, LoopbackEnd)`. No shared mutable state (no `Rc<RefCell>`, no `Arc<Mutex>`): each end owns a `std::sync::mpsc::Sender` to the other end and a `Receiver` from it, so ends are `Send` and a server host can move one to its own thread. The **sending** end applies the link conditions for its direction (its own seeded `SimRng`, one per direction, seeded from `seed` and the direction): it decides drop, jitter and reorder, computes the delivery tick, and sends `(deliver_at: Tick, channel, bytes)`. The **receiving** end drains its `Receiver` into a pending list ordered by `(deliver_at, send sequence)` and releases entries whose `deliver_at <= now` (its own `advance_to` clock).
  - `LoopbackEnd: Transport` plus `pub fn advance_to(&mut self, now: Tick)`, which sets the time that delivery is checked against. Packets carry a delivery tick = send tick + latency + seeded jitter. Unreliable packets may be dropped or reordered by a seeded `SimRng`. **Reliable packets are never dropped and are delivered in send order** (the loopback models an already-reliable stream).
  - Determinism: same seed + same sends + same `advance_to` calls → identical delivery sequence.
- `engine/netcode/src/handshake.rs`: pure state machines `ClientHandshake` and `ServerHandshake { capacity: u8 }` that consume and produce `Message`s (no I/O): the server is built with its `GameProtocol`; engine version check, then game id and version check → `Welcome` or `Reject` (`WrongGame` when either differs).

Tests (`engine/netcode/tests/`):
- `codec.rs`: proptest round-trip for every `Message` variant; oversized messages rejected with `MessageTooLarge`; proptest that `decode` on arbitrary bytes (0..2048 long) never panics.
- `input.rs`: a payload whose `validate` fails is rejected by `InputFrame::new` and by `decode`; a payload over `MAX_BYTES` is rejected with `PayloadTooLarge`; `InputHistory` rejects non-increasing ticks; `InputReceiver` dedupes redundant frames and yields each tick exactly once under random bundle loss (proptest over loss patterns).
- `loopback.rs`: PERFECT delivers everything at send tick + 0; latency delays by exactly N ticks; reliable channel is ordered and lossless under 50 % configured loss; unreliable loss rate over 10 000 packets within ±2 % of configured; same seed → same sequence, different seed → different sequence.
- `handshake.rs`: version mismatch → `Reject(VersionMismatch)`; capacity reached → `Reject(ServerFull)`; a different `GameId` or `GameVersion` → `Reject(WrongGame)`; happy path assigns increasing `ClientIndex`.
- A counting-allocator test (as in `engine/world/tests/no_alloc.rs`) that steady-state `encode` of `Inputs` into a reused buffer does not allocate.

Do not touch any other crate's source.

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```
Rules reminder (review will grep for these): no `unwrap`/`expect`/`panic!`/`#[allow]` in non-test code, no `unwrap_or*`/`.ok()`/`let _ =` that hide an error, no numeric `as` casts that can truncate, no silent fallbacks. Tests may use `unwrap`/`expect` with a module-level `#![allow(clippy::unwrap_used, clippy::expect_used)]` in test files only.
