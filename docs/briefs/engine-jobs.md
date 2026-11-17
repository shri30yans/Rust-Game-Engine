# engine-jobs: the job pool and the background pool (build step 2, D25)

Goal: a new crate `engine/jobs` (`engine-jobs`, lib name `jobs`) that is the only crate naming rayon. Done means: ordered chunked parallel helpers give bit-identical results with 0, 1 and many workers, the background pool runs file-style work off the frame path, and all four gates pass.

Read first: `AGENTS.md`, `docs/ARCHITECTURE.md` §3, `docs/README.md` §7 D25, `engine/core/src/lib.rs` (house style for headers, errors and docs).

## Design

**Layering.** Layer 0, no dependency on any other engine crate. Add `{ "pattern": "engine/jobs", "layer": 0 }` to `tools/check-layers/layers.json` and `"engine/jobs"` to the workspace `members` in the root `Cargo.toml`. `engine/core` is also layer 0, so `engine-jobs` must not depend on it and it must not depend on `engine-jobs` (the checker forbids same-layer edges). Dependencies: `rayon` (add to `[workspace.dependencies]` at the newest stable 1.x), `thiserror.workspace = true`. Dev: `proptest`, `stats_alloc` (both already in the workspace; if `stats_alloc` is not a workspace dependency yet, copy the version used by `engine/physics`).

**Files** (every file has a `//!` header stating its job and invariants; every public item has docs):

`src/lib.rs`: re-exports and the crate header.

`src/error.rs`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("failed to build the worker pool with {workers} workers: {reason}")]
    PoolBuild { workers: usize, reason: String },
    #[error("chunk size must be at least 1")]
    ZeroChunkSize,
    #[error("output slice has {got} slots but {expected} chunks need slots")]
    OutputLength { expected: usize, got: usize },
}
```

`src/chunk.rs`:
```rust
/// Fixed chunk length in items. Never derived from the worker count (D25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkSize(NonZeroUsize);
impl ChunkSize { pub fn new(items: usize) -> Result<Self, JobError>; pub fn get(self) -> usize; }
/// Number of chunks `len` items split into: ceil(len / chunk). Pure; use `div_ceil`.
pub fn chunk_count(len: usize, chunk: ChunkSize) -> usize;
```

`src/pool.rs`: `pub struct JobPool`, a private enum `Backend { Inline, Rayon(rayon::ThreadPool) }`.
- `JobPool::new(workers: usize) -> Result<Self, JobError>`: `0` gives `Inline` (no threads at all); otherwise a rayon pool with exactly `workers` threads, thread names `job-worker-{index}`, no pinning, default stack size.
- `pub fn workers(&self) -> usize`.
- `pub fn for_each_chunk_mut<T, F>(&self, items: &mut [T], chunk: ChunkSize, f: F) where T: Send, F: Fn(usize, &mut [T]) + Sync`: splits `items` into consecutive chunks of `chunk.get()` items (the last may be shorter), calls `f(chunk_index, chunk_items)` once per chunk. Inline backend runs chunks in index order on the calling thread; rayon backend uses `pool.install(|| items.par_chunks_mut(n).enumerate().for_each(...))`. Chunk boundaries are identical in both.
- `pub fn map_chunks<T, R, F>(&self, items: &[T], chunk: ChunkSize, out: &mut [R], f: F) -> Result<(), JobError> where T: Sync, R: Send, F: Fn(usize, &[T]) -> R + Sync`: writes `f(chunk_index, chunk_items)` into `out[chunk_index]`. Returns `JobError::OutputLength` when `out.len() != chunk_count(items.len(), chunk)`. The caller combines `out` in index order on one thread; this crate offers no reduce and no `sum` (D25 bans float reductions across threads).
- `pub fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R`: runs `f` so that rayon parallel iterators used inside it (Rapier's `parallel` feature) use this pool. Inline backend just calls `f()`. Doc comment says why it exists.
- No global pool is ever created or used; `rayon::ThreadPoolBuilder::build_global` and global `par_iter` calls are forbidden in this crate. Use `ThreadPoolBuilder::new().num_threads(workers).thread_name(...).build()` and map its error to `PoolBuild` (keep the message via `to_string()`).

`src/background.rs`: the background pool (file loading, decompression, shader building; never waited on by a frame).
- `pub struct BackgroundPool`, built with `BackgroundPool::new(workers: usize) -> Result<Self, JobError>` using `std::thread::Builder` (names `background-{index}`), a single shared `std::sync::mpsc` channel of boxed jobs behind a `Mutex<Receiver>` that only the worker threads touch (this is the one place a `Mutex` is allowed: it is not simulation state, and the doc says so).
- `pub fn spawn<T: Send + 'static>(&self, job: impl FnOnce() -> T + Send + 'static) -> Task<T>`.
- `pub struct Task<T>` holds an `mpsc::Receiver<T>`; `pub fn try_take(&mut self) -> Result<Option<T>, JobError>` is non-blocking: `Ok(Some(v))` once, `Ok(None)` while running, and `Err(JobError::TaskLost)` if the worker dropped the sender without sending (add the variant `#[error("the background task ended without a result")] TaskLost`). After a value was taken, further calls return `Err(JobError::TaskLost)`'s sibling `TaskTaken` (add `#[error("the task result was already taken")] TaskTaken`); track this with a `taken: bool` field. Provide `pub fn wait(self) -> Result<T, JobError>` for tests and tools (blocking).
- With `workers == 0` the job runs inline inside `spawn` and its result is ready at once (this is the web build, where there are no threads; the doc comment says so). That is a designed mode, not a fallback.
- Dropping the pool closes the channel and joins all workers (implement `Drop`); a job already running finishes. A panicking job must not take the pool down: run each job through `std::panic::catch_unwind(AssertUnwindSafe(job))`; on panic the task's sender is dropped so the caller sees `TaskLost`, and the worker keeps serving. Do not swallow silently: log nothing here (no logging dependency), the `TaskLost` error is the signal.

**Decisions already made** (do not change): rayon, one fork-join pool plus a separate background pool, fixed-size chunks, results combined by the caller in index order, inline mode for 0 workers, no thread pinning, no global pool.

**Deleted/untouched:** nothing exists yet; do not touch other crates, the physics crate does not adopt `install` in this brief.

## Tests

`tests/determinism.rs` (proptest, no allocator): for random `Vec<f32>` (length 0..5000, values in a sane range) and chunk sizes 1..300, compute per-chunk sums with `map_chunks` and combine them in index order; assert the combined `f32` bits are identical for pools with 0, 1, 2 and 7 workers. Also `for_each_chunk_mut` writing `f(chunk_index, item)` into every item gives identical output across the same four pools, and chunk indices seen are exactly `0..chunk_count`, each once (record them in per-chunk slots, not a shared Vec).
`tests/api.rs`: `ChunkSize::new(0)` errors; `chunk_count` for 0, 1, exact multiple, remainder; `map_chunks` with a wrong `out` length returns `OutputLength` with the right numbers; empty input is fine; `install` returns the closure's value and, on a rayon backend, `rayon::current_num_threads()` inside it equals `workers()` (depend on rayon in dev-dependencies for this single assertion); `workers()` for 0 and 3; background: a job's result arrives (`wait`), `try_take` returns `None` before a gated job is released (use a `std::sync::mpsc` channel the job blocks on, never `sleep`) and `Some` after; a panicking job gives `TaskLost` and a following job on the same pool still succeeds; `workers == 0` runs inline (the closure sets an `AtomicBool` before `spawn` returns); dropping the pool with queued jobs does not hang (test with a timeout-free structure: it must simply return).
`tests/no_alloc.rs`: ONE `#[test]` in the file (the allocator counter is process-wide). Warm the pool once, then measure `for_each_chunk_mut` and `map_chunks` over a pre-sized slice: the Inline pool must show exactly 0 allocations and 0 reallocations. For the rayon pool measure first; if it is 0, assert 0; if rayon allocates, assert `allocations <= N` and `bytes_allocated <= B` where N and B are the measured values rounded up to the next multiple of 8 allocations / 64 bytes, add a comment naming the cause, and report the numbers in your final message (the owner will record a decision, do not edit README.md §7 for it). Reallocations stay 0.

## Docs (same change)

- `docs/ARCHITECTURE.md` §1 `jobs` row State: `built`; `docs/README.md` §2: add `engine/jobs` to the built list. Leave `docs/briefs/README.md` statuses to the reviewer.

## Hard rules (a review fails on any of these)

No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`, no panicking indexing, no `as` numeric casts in non-test code. No `#[allow(...)]` anywhere in non-test code. No `unsafe`. No swallowed errors (`let _ =`, `.ok()`, `unwrap_or*`, empty `Err(_)` arms). No stubs, TODOs, dead code or duplicate implementations. No `sleep` in tests, no `#[ignore]`. Test files may use `unwrap`/`expect` with a module-level `#![allow(clippy::unwrap_used, clippy::expect_used)]`.

Do not touch: other crates, other files except `Cargo.toml` (workspace members and the `rayon` dependency), `tools/check-layers/layers.json`, and the two doc lines above.

## Gates

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p check-layers
```

Run `cargo test -p engine-jobs` 15 times before finishing; all must pass.
