# netcode-quic: the real network transport (QUIC via quinn) with a multi-client server endpoint (PLAN M2.3, D13)

Goal: two machines can talk over the internet with the same `Transport` the loopback tests use. Done means: (1) `QuicClient` and `QuicServer` in `engine-netcode` implement the existing `Transport` contract (reliable ordered channel on a QUIC stream, unreliable channel on QUIC datagrams) without blocking the game thread; (2) the server accepts many clients, each a `ClientId` with its own `Transport` view; (3) the slice 1 handshake (protocol version, content hash) runs over it and refuses mismatches; (4) a test runs a real UDP connection on localhost, exchanges inputs and snapshot packets, and survives a client disconnect and reconnect; the four gates pass.

Read first: `AGENTS.md`; `docs/README.md` §7 D6, D13, D24, D55; `docs/ARCHITECTURE.md` §2–4; `engine/netcode/src/{transport,loopback,protocol,handshake,codec}.rs`; quinn's documentation for datagrams (`Connection::send_datagram`, `max_datagram_size`) and streams.

## Design

Follow Unreal's split (D55): a **net driver** owns the socket and the connections (`UNetDriver`, one `UNetConnection` per client), and the replication code above it never sees the socket. Ours: `QuicServer` is the net driver, a `ClientId` names a connection, and everything above uses `Transport`.

- **Threading:** quinn is async. Run one tokio runtime (current-thread or 1–2 workers) on a dedicated I/O thread owned by the endpoint object; the game thread exchanges packets with it through bounded lock-free queues (crossbeam `ArrayQueue` or the channel type already in the workspace) of pre-allocated byte buffers. The game thread never awaits, never blocks, and never allocates per packet after warm-up (buffer pool). A full queue drops unreliable packets (counted) and is an error for reliable ones (the connection is closed with a reason).
- **Channels:** `Channel::Unreliable` → QUIC datagram; payloads over `max_datagram_size()` are a typed error (the snapshot writer already caps at `MAX_DATAGRAM_BYTES`; assert the QUIC limit is at least that at connect time and fail clearly if not). `Channel::Reliable` → one long-lived bidirectional stream per connection with length-prefixed frames (u16 length, capped by the protocol's per-message caps).
- **Server:** `QuicServer::bind(addr, ServerConfig)`; `poll_events() -> impl Iterator<Item = ServerEvent>` with `Connected(ClientId)`, `Disconnected(ClientId, Reason)`; `send(client, channel, bytes)`, `recv() -> Option<(ClientId, Channel, &[u8])>` into a reused buffer. `ClientId` is an index plus a generation (a reconnect gets a new id). Max clients from config; extra connections are refused with a reason.
- **Client:** `QuicClient::connect(addr, ClientConfig)` returns at once; state `Connecting | Connected | Closed(Reason)` polled each frame; implements `Transport`.
- **Security for now:** self-signed certificate generated at server start (rcgen) and the client pins the server's certificate hash given out of band (the dev server prints it; the lobby will hand it over later). No "accept any certificate" mode in release builds; a `dev-insecure` feature for local testing only, compiled out by default.
- **Time:** idle timeout and keep-alive from config; no wall clock inside the simulation (the transport may use real time; the game sees only packets).
- **Platform:** desktop and Android build; wasm is out of scope here (WebTransport later, D13), so gate quinn behind `cfg(not(target_arch = "wasm32"))` and keep the `Transport` trait available on every target.

**Tests:**
- Localhost: server + two clients, handshake, 1,000 unreliable and 200 reliable messages each way; reliable arrive complete and in order; unreliable arrive or are counted dropped; oversize datagram → typed error.
- Handshake mismatch (protocol version, content hash) → refused with the reason on both sides.
- Disconnect: dropping a client yields `Disconnected` on the server within the idle timeout; reconnect gets a new `ClientId`.
- Max clients: the extra one is refused.
- Counting allocator on the game-thread side: steady send/recv of snapshot-sized packets allocates nothing after warm-up.
- Existing loopback tests unchanged.

**Do not touch:** anything outside `engine/netcode` and the workspace `Cargo.toml` dependency list. Other runs are editing `engine/app`, `kits/`, `games/`, `engine/render`, `engine/assets`.

**Docs in the same change:** `docs/ARCHITECTURE.md` §1 netcode row and §4; `docs/README.md` §7 D13 (what was built; the certificate approach); `docs/briefs/README.md` status `review`.

## Gates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
