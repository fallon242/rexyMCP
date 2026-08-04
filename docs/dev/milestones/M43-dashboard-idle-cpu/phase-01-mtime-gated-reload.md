# Phase 01: mtime-gated dashboard reload

**Milestone:** M43 — Dashboard Idle CPU
**Status:** in-progress
**Depends on:** none
**Estimated diff:** ~140 lines
**Tags:** language=rust, kind=bugfix, size=s

## Goal

Stop `rexymcp dashboard` from re-reading and re-parsing its input files on every
500 ms tick when nothing has changed. Compute a cheap fingerprint of the files
`load_data` reads, and only call `load_data` when that fingerprint moves. On this
repo this takes sustained idle CPU from 59 % of a core to ~0 %.

## Architecture references

Read before starting:

- `docs/dev/milestones/M43-dashboard-idle-cpu/README.md` — the measurements this
  phase is derived from, and why the other refresh-path costs are out of scope.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture reference above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

`mcp/src/dashboard/event_loop.rs:29` opens the refresh loop and calls `load_data`
unconditionally, every iteration:

```rust
    loop {
        spinner_tick = spinner_tick.wrapping_add(1);

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let data = load_data(
            repo,
            session,
            telemetry_dir,
            project_id.as_deref(),
            architect,
        );
```

…and at the bottom of the same loop body it blocks for at most 500 ms
(`event_loop.rs:64`):

```rust
        if event::poll(Duration::from_millis(500))?
```

`load_data` (`mcp/src/dashboard/mod.rs:46`) reads exactly two things from disk:

- `<telemetry_dir>/phase_runs.jsonl` — three separate times (`mod.rs:53`,
  `mod.rs:59`, `mod.rs:66`). On this repo that file is 103 MB / 278,836 lines and
  each full pass costs ~200 ms.
- the resolved session log, via `status::load_records(repo, session)`
  (`mod.rs:77`, `mod.rs:120`), which resolves through
  `status::resolve_session_log` (`mcp/src/status.rs:385`).

It also walks `docs/dev/milestones` via `resolve_milestone` / `resolve_milestone_dir`.
That walk is **measured negligible** and is not what this phase is about — see
Out of scope.

The signature you are gating:

```rust
pub fn load_data(
    repo: &Path,
    session: Option<&str>,
    telemetry_dir: Option<&Path>,
    project_id: Option<&str>,
    architect: &rexymcp_executor::config::ArchitectConfig,
) -> DashboardData
```

`resolve_session_log` is already `pub` and returns the concrete path:

```rust
pub fn resolve_session_log(repo: &Path, session: Option<&str>) -> Result<PathBuf, String>
```

`mcp/src/dashboard/mod.rs` currently imports only `use std::path::Path;` and
already has `use crate::status::{self, StatusSummary};`.

## Spec

### 1. Add `DataFingerprint` to `mcp/src/dashboard/mod.rs`

Directly above `load_data`, add a stamp type and the function that computes it.
Both are `pub(crate)` — `event_loop` is the only consumer.

```rust
/// Cheap change-detection stamp for the files `load_data` reads. Comparing two
/// stamps costs a `read_dir` plus a handful of `stat` calls; reloading costs a
/// full read + parse of a telemetry store that reaches hundreds of megabytes on
/// long-lived projects.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DataFingerprint {
    /// Resolved session log: path, byte length, mtime. `None` when no log
    /// resolves (no sessions dir, or no log matching the `session` needle).
    session: Option<(PathBuf, u64, SystemTime)>,
    /// `<telemetry_dir>/phase_runs.jsonl`: byte length, mtime. `None` when
    /// telemetry is unconfigured or the file does not exist yet.
    telemetry: Option<(u64, SystemTime)>,
}

/// Stamp the current state of `load_data`'s inputs. Stats only — never reads
/// or parses file contents.
pub(crate) fn fingerprint(
    repo: &Path,
    session: Option<&str>,
    telemetry_dir: Option<&Path>,
) -> DataFingerprint {
    // ...
}
```

Rules for the body:

- The **session** component: `status::resolve_session_log(repo, session).ok()`,
  then `std::fs::metadata`, then `(path, meta.len(), meta.modified().ok()?)`. Any
  step failing collapses the whole component to `None` — a `None` that later
  becomes `Some` is itself a change, so nothing is lost.
- The **telemetry** component: `telemetry_dir.map(|d| d.join("phase_runs.jsonl"))`,
  then the same `metadata` / `len` / `modified` treatment.
- Include the resolved **path** in the session component. With `session: None`
  the dashboard follows the *newest* log (`status::find_latest_session_log`,
  `mcp/src/status.rs:280`), so a new run starting a new log file must invalidate
  the cache even if the new file happens to share a length with the old one.
- Do **not** include the telemetry path — it is fixed for the process lifetime.
- Do **not** hash or read contents. The point is that this is cheap.

No `.unwrap()` / `.expect()` anywhere; use `ok()` / `let ... else` / `?` inside a
small helper, per the error model in `docs/dev/STANDARDS.md`.

Add the two imports `mod.rs` is missing: `std::path::PathBuf` and
`std::time::SystemTime`.

### 2. Gate the reload in `mcp/src/dashboard/event_loop.rs`

Prime the data once **before** the loop, then reload inside the loop only when the
fingerprint moves. Use this exact shape — it needs no `Option`, no `clone`, and no
unwrap:

```rust
    let mut fp = crate::dashboard::fingerprint(repo, session, telemetry_dir);
    let mut data = load_data(
        repo,
        session,
        telemetry_dir,
        project_id.as_deref(),
        architect,
    );

    loop {
        spinner_tick = spinner_tick.wrapping_add(1);

        let now_ms = /* unchanged */;

        let next_fp = crate::dashboard::fingerprint(repo, session, telemetry_dir);
        if next_fp != fp {
            fp = next_fp;
            data = load_data(
                repo,
                session,
                telemetry_dir,
                project_id.as_deref(),
                architect,
            );
        }
```

Everything downstream in the loop body (`data.records.len()` vs
`prev_record_count`, `data.summary.ended`, the `render_dashboard(&data, ...)`
call) stays **byte-for-byte unchanged** — `data` is still an owned
`DashboardData` in scope, it is simply not rebuilt every tick.

Do **not** also gate `terminal.draw`. The frame must still be drawn every tick so
the spinner animates and terminal resizes are honored. Rendering is measured free
(README § "Scope explicitly rejected on evidence"); the load is not.

Note on `prev_record_count`: it is initialized to `0` before the loop and stays
there. With the priming load above, the first iteration sees
`data.records.len() > 0` and sets `follow = true`, which it already is. Leave that
logic alone.

### 3. Tests in `mcp/src/dashboard/mod.rs`

Add to the existing `#[cfg(test)] mod tests` block at the bottom of the file (the
one holding `load_data_returns_error_when_no_sessions_dir` etc.). Being in-module,
the tests can read `DataFingerprint`'s private fields directly.

Reuse the existing helpers in that block — `sessions_dir(dir.path())` and the
`TempDir` setup pattern from `load_data_carries_raw_records`.

**These tests must not sleep and must not add a crate.** Filesystem mtime
granularity is not guaranteed to distinguish two writes inside one test, so every
positive assertion below is driven by a **length** or **path** change, never by
mtime alone. Do not attempt to test "mtime changed but length did not" — that
would require a `sleep` (banned by `docs/dev/STANDARDS.md`) or a `filetime`
dependency (not authorized).

## Acceptance criteria

- [ ] `cargo build` succeeds.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff. (Fix formatting with
      `rustfmt mcp/src/dashboard/mod.rs mcp/src/dashboard/event_loop.rs` only —
      never `cargo fmt --all`.)
- [ ] `cargo test` passes, including the six new tests below.
- [ ] `load_data` is called from exactly two places in `event_loop.rs`: the
      priming call before the loop and the gated call inside the `if`.
- [ ] The idle-CPU measurement under § End-to-end verification reports **≤ 2 %**,
      and its actual output is quoted in the completion Update Log.

## Test plan

All in `mcp/src/dashboard/mod.rs`'s `mod tests`.

- `fingerprint_is_stable_across_calls_when_nothing_changes` — write one session
  log and one `phase_runs.jsonl`; assert `fingerprint(...) == fingerprint(...)`
  for two consecutive calls with no intervening write.
- `fingerprint_changes_when_telemetry_grows` — stamp, append a line to
  `phase_runs.jsonl`, stamp again, assert the two differ.
- `fingerprint_changes_when_session_log_grows` — stamp, append a record to the
  session log, stamp again, assert the two differ.
- `fingerprint_changes_when_session_needle_selects_a_different_log` — write two
  logs (`session-phase-01-aaa.jsonl`, `session-phase-02-bbb.jsonl`); assert
  `fingerprint(repo, Some("aaa"), _) != fingerprint(repo, Some("bbb"), _)`.
  Asserts the resolved **path** is part of the stamp. (Selection is by explicit
  needle, not by mtime, so this is deterministic.)
- `fingerprint_session_is_none_without_sessions_dir` — empty `TempDir`, no
  telemetry; assert `.session.is_none()`.
- `fingerprint_telemetry_is_none_when_file_absent` — pass a `telemetry_dir` that
  exists but holds no `phase_runs.jsonl`; assert `.telemetry.is_none()`, and
  assert it becomes `Some` once the file is written (the None→Some transition is
  itself an invalidation).

## End-to-end verification

The artifact this phase ships is a running binary's CPU behavior; unit tests on
the fingerprint cannot observe it. Measure the real thing.

Build release, run the dashboard against this repo (whose telemetry store is the
103 MB file), leave it untouched, and sample `/proc/<pid>/stat`:

```bash
cargo build --release
script -qec "target/release/rexymcp dashboard --repo ." /dev/null >/dev/null 2>&1 &
sleep 3
PID=$(pgrep -f "rexymcp dashboard" | head -1)
read -r _ _ _ _ _ _ _ _ _ _ _ _ _ U1 S1 _ < /proc/$PID/stat
sleep 10
read -r _ _ _ _ _ _ _ _ _ _ _ _ _ U2 S2 _ < /proc/$PID/stat
echo "idle CPU: $(( (U2-U1+S2-S1)/10 ))%"
kill $PID
```

Fields 14/15 of `/proc/<pid>/stat` are `utime`/`stime` in clock ticks (100/s), so
the sum over a 10 s window divided by 10 is the percentage of one core.

The same command on the unmodified tree reports `idle CPU: 59%`. It must report
**≤ 2 %** after this phase. Quote the literal output line in the completion
Update Log.

Then confirm the TUI still works: with the dashboard open, run a command that
appends to the session log or wait for the sweep to append to
`phase_runs.jsonl`, and confirm the panels update — the gate must not freeze a
live dashboard. Say so explicitly in the Update Log.

## Authorizations

None. This phase adds no dependency, touches no `Cargo.toml`, and modifies only
`mcp/src/dashboard/mod.rs` and `mcp/src/dashboard/event_loop.rs`.

## Out of scope

Do **not** do any of the following, even though you will see them and they are
real:

- **The three-times-per-load parse of `phase_runs.jsonl`** (`mod.rs:53`,
  `mod.rs:59`, `mod.rs:66`) and the `from_str::<Value>` → `from_value::<T>` double
  parse in `executor/src/store/telemetry.rs:576` and `:712`. That is **phase 02**.
  Do not touch `executor/src/store/telemetry.rs` in this phase.
- **The unbounded growth of `phase_runs.jsonl`** and the sweep that causes it
  (`mcp/src/sweep.rs`). That is **phase 03**.
- **The `resolve_milestone` / `resolve_milestone_dir` double directory walk**
  (`mod.rs:80`, `mod.rs:81`). Measured negligible — it sits inside the 0 % idle
  reading with telemetry removed. Leave it.
- **Memoizing or gating the render path** (wrapping, `highlight.rs`, the session
  log re-read inside `load_data`). Also measured negligible. `terminal.draw` keeps
  running every tick.
- **Changing the 500 ms poll interval.** The interval is not the problem; the work
  done per interval is.
- **Any TUI/behavior change** — panels, key bindings, follow/scroll semantics,
  the spinner. A user must not be able to tell this phase landed except by the
  fan noise.

Note adjacent issues in "Notes for review" rather than fixing them.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-04 14:52 (started)

**Executor:** Claude (Sonnet 4.5)

Added `DataFingerprint` struct and `fingerprint()` function to `mcp/src/dashboard/mod.rs`, gated `load_data` calls in `event_loop.rs` to only reload when the fingerprint changes, and added 6 unit tests.
