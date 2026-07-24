# Phase 03: Durable run registry — reap a completed run after a serve restart

**Milestone:** M41 — Serve Liveness & Run Durability
**Status:** done
**Depends on:** none technically; **ordered last** because it closes the failure
mode phase 01 introduces (a serve process that now exits takes its in-memory run
results with it)
**Estimated diff:** ~200 lines (persistence + fallback + tests)
**Tags:** language=rust, kind=feature, size=m

## Goal

A run's terminal `PhaseResult` lives only in the serve process's memory
(`JobRegistry`'s `HashMap`). If the process exits — which, after phase 01, it now
deliberately does when its transport dies — a finished, committed phase becomes
permanently unreapable and `get_run_status` answers `{state:"unknown"}`. Persist
each run's terminal state to disk and have `get_run_status` fall back to it, so a
completed run is reapable from a **fresh** serve process.

`rexymcp status` reported `ended (complete)` correctly throughout the issue #5
incident precisely because it reads on-disk state. This phase gives the MCP
reporting path the same property.

## Architecture references

Read before starting:

- `docs/dev/milestones/M41-serve-liveness/README.md` — the milestone, especially
  § "Why the durable-registry phase is required, not optional".
- `docs/architecture.md` § Status #30 — the async job model (`run_id`,
  `get_run_status`, `stop_phase`) this extends.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**The registry is memory-only.** `mcp/src/jobs.rs:43-48`:

```rust
#[derive(Default)]
pub struct JobRegistry {
    runs: Mutex<HashMap<String, RunEntry>>,
}
```

`publish` (`jobs.rs:70-76`) stores the terminal state in a `watch` channel and
nothing else. `spawn_run` (`jobs.rs:174-193`) calls it from the spawned task.

**The lookup has exactly one input.** `GetRunStatusParams` (`mcp/src/server.rs:55-58`)
carries a `run_id` and nothing else, and `get_run_status_inner`
(`server.rs:95-127`) maps `await_terminal`'s `None` straight to `"unknown"`:

```rust
match registry.await_terminal(&run_id, timeout).await {
    None => GetRunStatusOutput { run_id, state: "unknown".into(), result: None, error: None },
```

That `None` is the only hook the fallback needs.

**The stored JSON is already capped.** `execute_phase_inner` runs
`cap::cap_phase_result` (`server.rs:226`, per-field budget `MAX_FIELD_BYTES =
50_000`, `mcp/src/cap.rs:6`) before the value reaches `RunState::Complete`, so a
record is small enough to write whole. Do not add a second capping layer.

**Where on-disk state already lives:** the session JSONL logs go to
`<repo>/.rexymcp/sessions/` (`executor/src/agent/mod.rs:228`), and telemetry
defaults to `~/.rexymcp/telemetry` (`executor/src/config.rs:888`). Run records
follow the **home** convention, for the reason in task 2.

## Spec

### 1. A terminal run record, and where it goes

Add to `mcp/src/jobs.rs` a serializable record and its path helper:

```rust
/// A run's terminal outcome, persisted so it survives the serve process.
#[derive(Serialize, Deserialize)]
struct RunRecord {
    run_id: String,
    /// "done" | "failed" — mirrors GetRunStatusOutput.state; never "running".
    state: String,
    result: Option<serde_json::Value>,
    error: Option<String>,
    /// Unix millis when the record was written.
    ts: u64,
}
```

One file per run: `<record_dir>/<run_id>.json`. `run_id` is a v4 UUID
(`jobs.rs:150-152`), so it is filename-safe and collision-free — do not hash,
sanitize, or nest it.

**Only terminal states are persisted.** `RunState::Running` is never written; a
missing file means "this process never saw that run finish", which is exactly the
`unknown` answer.

### 2. Give `JobRegistry` an optional record directory

- Add a `record_dir: Option<PathBuf>` field. `JobRegistry::new()` and the `Default`
  impl leave it `None` — a registry with no directory behaves exactly as today, so
  every existing test keeps passing untouched.
- Add `JobRegistry::with_record_dir(dir: PathBuf) -> Self`.
- In `RexyMcpServer::new` (`mcp/src/server.rs:134-141`), build the registry with
  `with_record_dir` when `HOME` is set — `$HOME/.rexymcp/runs` — and fall back to
  `JobRegistry::new()` when it is not. Read `HOME` with `std::env::var_os`, the
  same way `mcp/src/main.rs:559` does.

**Why home and not `<repo>/.rexymcp/runs`:** `get_run_status` receives only a
`run_id`, so a per-repo location would be unlookupable without adding a
`repo_path` parameter and changing every caller (the plugin skills included). A
process-wide directory keyed by UUID makes the fallback a single `read`. Record
this rationale in a comment on `with_record_dir`; it is the kind of decision a
later reader will otherwise try to "correct".

### 3. Write the record when a run goes terminal

In `publish` (`jobs.rs:70-76`), after the existing `send_replace`, write the record
when `record_dir` is `Some` and the state is terminal.

- Write **atomically**: serialize to `<run_id>.json.tmp` in the same directory,
  then `std::fs::rename` onto the final name. A reader must never see a partial
  file.
- Create the directory if absent (`create_dir_all`).
- **Every I/O failure here is best-effort and non-fatal**: the in-memory publish
  has already happened and the live poll must still work. On failure, `eprintln!`
  one `rexymcp: …` line to stderr (never stdout — it is the JSON-RPC transport) and
  carry on. Do not propagate, do not panic, do not `unwrap`.
- `ts` comes from an injected clock, not a bare `Utc::now()` — follow whatever
  clock-injection pattern the surrounding code already uses; if there is none in
  `jobs.rs`, take `now_ms: u64` as a parameter on the internal write helper so the
  tests can pass a fixed value, and have `publish` supply the real time.

### 4. Fall back to disk on an unknown id

In `get_run_status_inner` (`server.rs:95-127`), replace the `None` arm: before
answering `"unknown"`, ask the registry for a persisted record and, if one exists,
return the state it carries (`"done"` with `result`, or `"failed"` with `error`).
Only a genuinely absent record yields `"unknown"`.

Add the reader on `JobRegistry` (e.g. `load_record(&self, run_id: &str) ->
Option<RunRecord>`) so `server.rs` does no filesystem work itself and the fallback
stays testable through the same injectable `record_dir`.

**The read must be bounded and cheap** — one `fs::read` of one file, no directory
scan, no retry, no waiting. A malformed or unparsable file is treated as absent
(`unknown`), not as an error. This preserves the documented `~15 s` ceiling on
`get_run_status`: the fallback runs only after `await_terminal` has already
returned `None`, which it does immediately for an unknown id.

### 5. Bound the directory's growth

Add a pure, clock-injected prune helper — `prune_records(dir, max_age_ms, now_ms)`
— that deletes records whose `ts` is older than `max_age_ms`, and call it
best-effort from `with_record_dir` (i.e. once per serve start, not per write).
Use a 30-day `max_age_ms` constant with a doc comment. Failures are ignored;
pruning must never block startup or fail a serve launch.

Prune by the record's **`ts` field**, not by filesystem mtime — the field is what
the tests can control deterministically.

### 6. Nothing else changes

`RunState`, `await_terminal`, `spawn_run`'s signature, `request_stop`,
`stop_watcher`, and the `execute_phase` handler are untouched. No new MCP tool, no
new tool parameter, no schema change to `GetRunStatusParams` or
`GetRunStatusOutput`.

## Acceptance criteria

- [x] `cargo build` is green.
- [x] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [x] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [x] `cargo test` passes, including every pre-existing `mcp/src/jobs.rs` and
      `mcp/src/server_tests.rs` test **unmodified** (the `None` record dir keeps
      today's behavior).
- [x] A terminal `publish` with a record dir set writes exactly one
      `<run_id>.json`, and no `.tmp` file survives.
- [x] A `Running` publish writes nothing.
- [x] `get_run_status_inner` returns `{state:"done", result:…}` for a `run_id` that
      is absent from the in-memory map but present on disk.
- [x] `get_run_status_inner` still returns `{state:"unknown"}` when neither memory
      nor disk knows the id, and when the on-disk file is unparsable.
- [x] A failed record write does not change the in-memory answer — the in-memory
      `send_replace` happens first and unconditionally; the write is best-effort
      and only logs.

## Test plan

Unit tests in the existing `#[cfg(test)] mod tests` block in `mcp/src/jobs.rs`, and
alongside the existing `get_run_status_inner` tests for the server-side ones. Every
test uses a `tempfile::TempDir` for the record dir and a **fixed** `now_ms` — no
real clock, no `$HOME`, no network.

- `publish_terminal_writes_run_record` in `mcp/src/jobs.rs` — insert, publish
  `Complete`, assert `<dir>/<run_id>.json` exists, parses, and carries
  `state == "done"` plus the result JSON.
- `publish_running_writes_nothing` in `mcp/src/jobs.rs` — insert only; assert the
  directory contains no file for the id. The negative case for task 3.
- `publish_failed_writes_error_record` in `mcp/src/jobs.rs` — assert
  `state == "failed"` and the error string round-trips.
- `record_write_leaves_no_tmp_file` in `mcp/src/jobs.rs` — after a terminal
  publish, assert no `.tmp` entry remains in the dir (pins the rename, not a
  partial write).
- `registry_without_record_dir_writes_nothing` in `mcp/src/jobs.rs` — a
  `JobRegistry::new()` publish must not create any file even when a dir exists
  nearby; pins the opt-in default.
- `load_record_returns_none_for_unparsable_file` in `mcp/src/jobs.rs` — write
  `not json` to `<dir>/<uuid>.json`, assert `load_record` is `None` (and does not
  panic).
- `get_run_status_falls_back_to_disk_for_unknown_id` in the server tests — a
  registry whose map has never seen the id but whose record dir holds a `done`
  record returns `{state:"done"}` with the result. **This is the phase's headline
  test** — it is the restart scenario, minus the restart.
- `get_run_status_unknown_when_neither_memory_nor_disk` in the server tests —
  still `"unknown"`.
- `prune_records_deletes_only_old_records` in `mcp/src/jobs.rs` — two records with
  fixed `ts` values straddling the cutoff at a fixed `now_ms`; assert exactly the
  old one is gone.

**Mutation self-check before you finish:** temporarily make the `None` arm of
`get_run_status_inner` skip the disk lookup, and confirm
`get_run_status_falls_back_to_disk_for_unknown_id` fails; then restore. Report the
observed failure in your Update Log. (Do not commit the mutation.)

## End-to-end verification

The restart scenario is the point of the phase, so prove it against the real
binary and quote the output. `$HOME/.rexymcp/runs` is the live directory; use a
scratch `HOME` so you do not disturb the developer's real one.

1. Show the record appears for a real run. Build, then start `serve` under a
   scratch `HOME`, drive one `execute_phase` through it (or, if driving a full
   phase is impractical in your session, state that plainly and instead run
   `rexymcp run-phase` and show the record helper writing and reading back a
   record via a focused `cargo test` run — do not fabricate a session).
2. Show the fallback survives a restart: with the run's record on disk, **kill the
   serve process**, start a fresh one against the same scratch `HOME`, and issue a
   `get_run_status` for that `run_id`. Expect `{"state":"done", …}`, not
   `{"state":"unknown"}`. Quote the literal JSON both times.

If you cannot complete step 1 hermetically within your session, say so explicitly
and hand step 2 to the reviewer rather than skipping it silently — an unverified
restart claim is the one thing this phase cannot ship without.

## Authorizations

None. No new dependencies — `serde`, `serde_json`, `uuid`, and `tempfile` (dev) are
already in `mcp/Cargo.toml`. No edits to `Cargo.toml` or `docs/architecture.md`.
Files you may edit: `mcp/src/jobs.rs`, `mcp/src/server.rs`, `mcp/src/server_tests.rs`.

## Out of scope

- The `serve` shutdown path (phase 01) and the `bash` stdin fix (phase 02).
- Persisting **`running`** state, a live index of in-flight runs, or any
  cross-process cancellation (`stop_phase` against another process's run). Records
  are terminal-only.
- A single-instance guard, or any duplicate-serve detection. Separate bug; this
  phase incidentally softens it, which is not a licence to scope it.
- Adding `repo_path` (or any parameter) to `GetRunStatusParams`, or otherwise
  changing the MCP tool schemas.
- Reading or reconciling `<repo>/.rexymcp/sessions/` JSONL logs to reconstruct a
  `PhaseResult` — a much larger idea, and unnecessary once the terminal record is
  written directly.
- A CLI subcommand to list or inspect run records. Note it in "Notes for review" if
  you think it is worth having; do not build it.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-07-24 15:55 (complete)

**Summary:** Implemented **directly by the architect** at the user's request (no
dispatch, no `PhaseRun`). `JobRegistry` gained an `Option<PathBuf> record_dir`
(`None` by default, so `new()`/`Default` stay purely in-memory and every
pre-existing test is untouched) plus `with_record_dir`, which also runs the
one-per-serve-start prune. `publish` now clones the state, publishes in memory
**first**, then best-effort writes a `RunRecord` via write-to-`.tmp` + `rename`;
every I/O failure logs one `rexymcp:` line to stderr and returns.
`get_run_status_inner`'s `None` arm consults `load_record` before answering
`unknown`. `RexyMcpServer::new` anchors the dir at `$HOME/.rexymcp/runs`, falling
back to in-memory when `HOME` is unset. `prune_records` judges age by the record's
own `ts` against an injected `now_ms`, with `RECORD_MAX_AGE_MS` at 30 days.

**Acceptance criteria:** all ticked above.

**Commands:** `cargo build` green; `cargo clippy --all-targets --all-features -- -D
warnings` clean; `cargo test` **673 bin + 1053 lib passed, 0 failed, 2 ignored**;
`cargo fmt --all --check` clean after `rustfmt --edition 2024` on the three touched
files.

**End-to-end verification — the restart scenario, with real processes:**

A scratch `HOME` and a scratch git repo whose config points at an unreachable
executor (`http://127.0.0.1:1`), driven over real MCP stdio:

```
serve A: execute_phase -> run_id=f1eb0a4d-e3b5-474b-8571-fd7b80768efa
serve A: get_run_status -> {"run_id": "f1eb0a4d-…", "state": "failed"}
record on disk: exists=True size=202B
serve A: killed
serve B (fresh process): get_run_status -> state='failed'
  full response: {"error": "backend: Request failed: error sending request for url (http://127.0.0.1:1/chat/completions)", "run_id": "f1eb0a4d-…", "state": "failed"}
serve B: unknown id -> state='unknown'
RESULT: PASS
```

Serve B **never saw that run** and still reaped its terminal state, error string
intact; an unknown id still answers `unknown` from the same process.

The `done` path — the one that carries a `result` payload rather than an error
string — verified separately against a fresh serve reading a `done`-shaped record
it never wrote:

```
fresh serve, run it never saw -> {"result": {"diff": "+42 -3", "phase": "phase-01",
  "status": "complete", "turns": 7}, "run_id": "aaaaaaaa-…", "state": "done"}
RESULT: PASS
```

**Mutation self-check:** replacing the `None` arm's disk lookup with the old
straight-to-`unknown` answer fails both
`get_run_status_falls_back_to_disk_for_unknown_id` and
`get_run_status_falls_back_to_disk_for_failed_run`; restored, suite green, mutation
not committed.

**Files changed:**
- `mcp/src/jobs.rs` — `RunRecord`, `record_dir`, `with_record_dir`, `write_record`,
  `load_record`, `record_path`, `now_ms`, `prune_records`, `RECORD_MAX_AGE_MS`.
- `mcp/src/server.rs` — disk fallback in `get_run_status_inner`; `$HOME`-anchored
  record dir in `RexyMcpServer::new`.
- `mcp/src/server_tests.rs` — three fallback tests.

**New tests:** `publish_terminal_writes_run_record`,
`publish_failed_writes_error_record`, `publish_running_writes_nothing`,
`record_write_leaves_no_tmp_file`, `registry_without_record_dir_writes_nothing`,
`load_record_returns_none_for_unparsable_file`,
`load_record_returns_none_for_absent_run`, `prune_records_deletes_only_old_records`,
`prune_records_ignores_unparsable_and_foreign_files` (all `mcp/src/jobs.rs`);
`get_run_status_falls_back_to_disk_for_unknown_id`,
`get_run_status_falls_back_to_disk_for_failed_run`,
`get_run_status_unknown_when_neither_memory_nor_disk` (`mcp/src/server_tests.rs`).

**Notes for review:**

- **The live `done`-through-a-real-LLM run was not performed.** The E2E above
  proves the machinery across a real process boundary using a `failed` run (fast,
  hermetic, no LLM) and proves the `done` payload shape through a fresh serve
  reading a record it never wrote. What is *not* directly demonstrated is a real
  Qwen-backed phase completing, being persisted, and being reaped after a restart —
  that will happen on the next real dispatch, and is worth an eyeball then.
- `publish` now clones `RunState` to keep the value after `send_replace`. The
  cloned payload is the already-capped `PhaseResult` JSON
  (`cap::cap_phase_result`, ≤50 KB/field), so this is not a large copy — but it is
  one copy per terminal publish where there was none.
- `prune_records` deliberately leaves unparsable and non-`.json` files alone rather
  than cleaning the directory, so a corrupted record is visible for debugging
  instead of silently vanishing.

### Review verdict — 2026-07-24

- **Verdict:** approved_first_try
- **Bounces:** none
- **Executor:** Claude Code (direct) — architect-implemented, self-review; the
  two-process E2E and the mutation check are the independent evidence.
- **Scope deviations:** none. No `repo_path` parameter, no schema change, no
  persistence of `running`, no CLI subcommand, no session-log reconstruction.
- **Calibration:** none. Unlike phase-01, every predicted output in this phase doc
  was either executed as written or explicitly restated — the E2E section's own
  escape hatch ("say so plainly and hand step 1 to the reviewer") was used as
  intended for the live-LLM half.
