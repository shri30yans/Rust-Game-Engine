# net-play-1: one command path, a server loop and a client loop, the lobby on the wire (PLAN M2, D13, D55, D57)

Goal: an engine game can be played by several people over a network, joining through a lobby. Done means:
1. Every game feeds player intent through **one path**: the presenter turns input into the game's `Command`. Offline play is a standalone session with one local player, using that same path.
2. `NetServer` runs a room and a match for many clients over any server transport. It handles the handshake, lobby, joins and leaves, commands, ticks and snapshots.
3. `NetClient` connects, takes part in the lobby, sends commands, predicts its own pawn and interpolates everyone else.
4. A headless test plays a lobby and a match with a server and two clients over the loopback hub at 150 ms RTT and 5% loss.
5. Sandbox and IMAGINE still play offline exactly as before, and the four gates pass.

Read first:
- `AGENTS.md`;
- `docs/README.md` §7 D13, D24, D26, D55, D57;
- `docs/ARCHITECTURE.md` §0–4;
- `engine/netcode/src/{protocol,transport,loopback,handshake,input,predict,client}.rs`, `engine/netcode/src/snapshot/`, `engine/netcode/src/quic/server.rs`;
- `engine/sessions/src/{lobby,room/mod}.rs`;
- `engine/app/src/{game,app,client,headless}.rs`;
- `games/imagine/game/src/{present,sim}.rs` and `kits/fps/src/command.rs`.

Unreal's model, which we follow: `UNetDriver`/`UNetConnection`, `AGameModeBase::PostLogin`/`Logout`/`RestartPlayer`, `APlayerController` possessing a pawn and sending moves (`ServerMove`), `ClientRestart` telling a client its pawn, and `NM_Standalone` for offline play.

## Design

### engine-netcode

**Built already (Claude, 2026-11-12):** everything in this section is on main: `ServerTransport`, `LoopbackHub`, `Message::Session`, `Message::Possess`, `Message::channel()`, and `NetError::ServerFull`. Use it as is; the work left is engine-app and the games.

- **`Message::Session(Vec<u8>)`.** Opaque session bytes (the app puts lobby messages in them), reliable channel only, capped by `MAX_SESSION_BYTES = 16_384` in `cap_for_message`. Netcode does not depend on `engine-sessions` (sessions is the higher layer).
- **`Message::Possess { pawn: NetId }`.** The server tells a client which replicated entity it controls; `NetId` already serialises. Reliable. Re-sent after a resume.
- **`ServerTransport` trait** in `transport.rs`:
  - `recv(&mut self, out: &mut Vec<u8>) -> Option<ServerEvent>`;
  - `send(&mut self, ClientId, Channel, &[u8]) -> Result<(), NetError>`;
  - `disconnect(&mut self, ClientId) -> Result<(), NetError>`.

  Move `ClientId`, `ServerEvent` and `CloseReason` from `quic` into `transport.rs`; `quic` re-exports them so its tests still compile. `QuicServer` implements the trait; its inherent methods become the trait's (one API, no duplicates).
- **`LoopbackHub`** in `loopback.rs`: the server side of N loopback links.
  - `LoopbackHub::new(max_clients)`; `hub.connect(conditions) -> LoopbackEnd` (the client end, which already implements `Transport`).
  - `hub.advance_to(tick)` advances every link; it implements `ServerTransport`, reporting `Connected` on the first delivery and `Disconnected` when a client end is dropped.
  - Deterministic: same seed and conditions give the same deliveries.

### engine-app

- **`Game` gains:**
  - `type Command: netcode::InputPayload`: one tick of one player's intent;
  - `type Settings: Clone + Serialize + DeserializeOwned + PartialEq + Debug`: the lobby settings `S`;
  - `fn join(&self, world: &mut World, player: PlayerId, name: &PlayerName) -> Result<NetId, AppError>` (PostLogin plus RestartPlayer: spawn the pawn and return it);
  - `fn leave(&self, world: &mut World, player: PlayerId) -> Result<(), AppError>`;
  - `fn command(&self, world: &mut World, player: PlayerId, command: &Self::Command) -> Result<(), AppError>` (apply this tick's intent to the player's pawn; a dead or missing pawn ignores it).

  `PlayerId` is a new engine-app newtype over the session `SeatId`.

  `NetGame: Game` adds the online-only parts:
  - `type Predicted: netcode::Predicted<Input = Self::Command>`;
  - `fn replication(&self, world: &World, viewer: PlayerId, entity: NetId) -> EntityReplication`;
  - `fn protocol(&self) -> GameProtocol`.

  The sandbox implements only `Game`.
- **`Presenter` splits `apply_input`** into:
  - `fn command(&mut self, input: &Input, look: ViewAngles) -> Result<Self::Command, AppError>` (pure: input to intent);
  - `fn before_ticks(&mut self, world: &mut World) -> Result<(), AppError>` (default no-op: dev reloads such as IMAGINE's tuning and live scene).

  `Presenter` gets `type Command`, and `ClientApp<G, P>` requires `P: Presenter<Command = G::Command>`.
- **Standalone mode (offline).** `ClientApp` joins `PlayerId::LOCAL` once at start through `game.join`. Each tick it applies the presenter's command with `game.command(world, LOCAL, &cmd)`, then steps. This replaces writing the command into the world from the presenter: one path. The `Advance` of several ticks reuses the frame's command for each tick (today's behaviour).
- **`NetServer<G: NetGame, T: ServerTransport>`** (new `engine/app/src/net_server.rs`), driven by `tick(&mut self) -> Result<ServerTick, AppError>`, deterministic, no wall clock:
  1. Drain transport events. A new connection runs the netcode handshake (`ServerHandshake`); a mismatch is refused with its reason. Then `Session` bytes decode to `LobbyRequest<G::Settings>` and go to a `LobbyHost<ClientId, G::Settings, OsTokens>` (inject the `TokenSource` so tests use a counter). Lobby replies are encoded into `Session` messages.
  2. On `RoomEvent::MatchStarted`, every seated human and bot gets `game.join`, which returns its pawn. Each human client gets `Possess`. A later seat (join in progress, resume) joins or is re-possessed at once. A freed seat calls `game.leave`.
  3. Commands. `Inputs` bundles go through `InputReceiver` per client. `InputRepeater` covers a missed tick with the last command (Unreal repeats the last move). Bots get their command from the game's own systems, as today.
  4. `app.step()`.
  5. Per client, a `SnapshotWriter` (one per connection, created at possess) writes with `G::replication` and sends unreliable. Reuse buffers: no per-tick allocation after warm-up.
  6. `RoomEvent::MatchEnded` (from `end_match`, called by the game through a returned flag or an API the game drives) leaves the pawns in place until `return_to_lobby`.

  A `run_dedicated(server, tick_rate)` helper paces `tick()` in real time on the calling thread (sleep until the next tick boundary, report overruns). It is the only place that reads the clock.
- **`NetClient<G: NetGame, T: Transport>`** (new `engine/app/src/net_client.rs`):
  - `connect(transport, protocol)`;
  - `lobby(&self) -> Option<&RoomView<G::Settings>>`; `request(LobbyRequest<G::Settings>)`;
  - `membership()`; `resume_token()`;
  - `tick(&mut self, world: &mut World, command: G::Command) -> Result<(), AppError>`. While possessed, it pushes the command into `InputHistory`, sends the bundle (unreliable), predicts with `PredictionHistory<G::Predicted>`, reads snapshots into `SnapshotBuffer`, acks them, reconciles the pawn, and applies interpolated states (`interpolate`, delay from `InterpolationConfig`) to every other replicated entity.

  The client never runs server rules. It runs only its pawn's predicted step and the interpolation. `ClientApp` gets a `Session` enum `Standalone | Online(NetClient<..>)` behind the same per-tick command path. The windowed online client is in scope only as far as compiling and a headless test; the lobby screens are a later brief.
- **Errors:** new `AppError` variants for net and session failures carry the underlying `NetError` / `SessionError`. A frame error in online mode follows the C4 rules (skip, do not quit).

### Games
- **Sandbox:** `Command` = its existing input type with `InputPayload` (or a small serde struct); `join` spawns or returns the existing player; `Settings = ()`.
- **IMAGINE:** `Command` = `kit_fps::MoveCommand` with serde.
  - Decoding goes through `MoveCommand::new`: `#[serde(try_from = "RawMoveCommand")]`, so a malicious client cannot send a non-finite or over-long axis.
  - `join` maps a `PlayerId` to an actor slot. Today the player is slot 0, spawned in setup: keep that for `LOCAL` and spawn others into free slots.
  - `Settings` = a small struct (bot count, arena id); `NetGame` is implemented with a `Predicted` impl for its character in kit-fps (capture/restore the body pose, velocity and locomotion state; step = the kit's movement for one actor).
  - Offline play must hash identically to before this change for the same inputs (determinism test).

### Tests
- **netcode:** `Session` and `Possess` caps and round-trips. `LoopbackHub` delivers deterministically and reports connect and disconnect. `QuicServer` through `ServerTransport` passes the existing QUIC tests.
- **engine-app headless match:** a test game in `engine/app/tests/` with a trivial character (move a kinematic point by its command) and `Predicted`, plus a server and two clients over `LoopbackHub` at 150 ms RTT, 20 ms jitter and 5% loss. The clients:
  - say hello, join the lobby, ready up, and the host starts;
  - get `Possess`, then run 600 ticks of scripted commands.

  Asserts:
  - each client's predicted pawn ends within 1 cm of the server's;
  - the remote pawn interpolated on each client is within 0.5 m of the server's position at the interpolation tick;
  - a client that drops mid-match and resumes with its token gets the same seat and pawn back;
  - a client with the wrong `GameProtocol` is refused;
  - no allocation per server tick after warm-up (counting allocator, as in `no_alloc` tests), with the bound documented.
- **Standalone:** sandbox and IMAGINE headless tests unchanged. An IMAGINE test shows the offline state hash after N scripted ticks is identical to the hash from main before this change (record it first).

**Do not touch:** `engine/render`, `engine/ui`, `engine/audio`, `engine/assets`, `tools/` (except fixing compile breaks the trait change causes). Other runs edit them.

**Docs in the same change:**
- `docs/ARCHITECTURE.md` §1 (app, netcode rows) and §4 (how a match runs online);
- a new decision in `docs/README.md` §7 (one command path; standalone is a session with one local player; Possess; ServerTransport);
- `docs/briefs/README.md` status `review`;
- `games/imagine/docs/` where IMAGINE's input path is described.

## Decisions after Codex's first stop (2026-11-12)
1. **Auto fire is simulation, not input.** `MoveCommand`'s fire flag becomes `Trigger { Released, Held, OnTarget }`. `OnTarget` fires in the tick only when the weapon's own hitscan ray first hits a living actor of another team. kit-fps resolves it for the commanding actor, in the same tick and before the weapon step. IMAGINE's `autofire.rs` is deleted, and its presenter maps the Auto fire mode to `OnTarget` from input and settings alone. This keeps `Presenter::command` pure and makes the server decide (Unreal: gameplay decisions are authoritative).
2. **`LoopbackHub`:** the API on main is the reference (`new(max_clients, seed)`, separate conditions per direction).

## Decision: match travel (2026-11-12, found by imagine-menus)
A match must be built from the room's settings when the countdown ends, offline (IMAGINE's local lobby) and online (`NetServer`). Today `App::new` runs `Game::setup` once. Follow Unreal's travel (`ServerTravel`/`OpenLevel`): the world is torn down and a new one is built from the map and the mode options.
1. **`Game::setup(&self, world, schedule, settings: &Self::Settings)`.** Setup builds the world for these settings (arena, bot count, mode). `App::new(game, settings, config, profile)` takes the first settings. A game's front end (menu backdrop) is just settings the game chooses, so there is no separate code path.
2. **`App::travel(&mut self, settings: &G::Settings) -> Result<(), AppError>`** builds a fresh `World` and `Schedule` and runs `setup`. It swaps them in only on success, so a failed travel keeps the old world running. The world seed is derived deterministically from the app seed and a `travel_epoch: u32` that `travel` increments. The tick counter stays monotonic across travels: it never resets, so no tick number means two things.
3. **Client side.** `ClientApp` sees the epoch change and calls a new `Presenter::on_travel(&mut self, world: &World) -> Result<(), AppError>` (default no-op), where the presenter drops every per-world cache (NetId maps, render primitives, HUD state). The extractor clears its previous transforms so nothing interpolates across worlds. Standalone: the game's flow calls travel through one method on `ClientApp` (e.g. `travel(settings)`), never by reaching into `App`.
4. **Online.**
   - On `RoomEvent::MatchStarted`, `NetServer` travels with the room's settings, then joins every seat (`game.join`) and sends `Possess`.
   - Clients travel when the lobby view they receive enters `InMatch` with the room's settings.
   - Snapshots carry the travel epoch (add it to the snapshot header in `engine-netcode`; this is the one netcode change allowed). A client never applies a snapshot from another epoch. Test it: a snapshot from the old world arriving after travel is dropped.
5. **Tests:**
   - travel replaces the world and keeps the tick monotonic;
   - a failed setup leaves the old world untouched;
   - two travels with the same settings give the same state hash;
   - the headless online test runs lobby → match → results → lobby → second match on the same connections.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
