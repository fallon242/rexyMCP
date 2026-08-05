# Phase 01: one atomic write per telemetry append

**Milestone:** M44 — Atomic JSONL Appends
**Status:** todo
**Depends on:** none
**Estimated diff:** ~130 lines (a shared helper replacing four copies, plus one concurrency test)
**Tags:** language=rust, kind=bugfix, size=s

## Goal

A telemetry append currently issues **two** writes — the JSON payload, then the
newline — on an `O_APPEND` handle, so a second appender can land between them and
splice two records onto one line. Make each append a single buffered write, and
prove it with a test that **reproduces the race** rather than inspecting the shape
of the code.

## Architecture references

Read before starting:

- `docs/architecture.md` §44 (M44 — Atomic JSONL appends) — this milestone; states
  the defect, the 209 corrupt lines in the real store, and why phase 02 (reader
  visibility) is separate.
- `docs/architecture.md` §35 (M35, design fork 4) — established the append-only
  store and the `schema_version` write boundary these functions implement.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom, including **§1.1 "An end-to-end
   verification must prove it is live"**.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

Four functions in `executor/src/store/telemetry.rs` append to
`<telemetry_dir>/phase_runs.jsonl`. Their bodies are **byte-identical** apart from
the record type:

| Function | Line |
| --- | --- |
| `append` (`PhaseRun`) | `:195` |
| `append_review` (`PhaseReview`) | `:392` |
| `append_architect_activity` (`ArchitectActivity`) | `:553` |
| `append_architect_ledger` (`ArchitectLedger`) | `:689` |

Here is `append` in full — the other three differ only in the type of the second
parameter and the variable name:

```rust
pub fn append(telemetry_dir: &Path, run: &PhaseRun) -> std::io::Result<PathBuf> {
    use std::io::Write;

    std::fs::create_dir_all(telemetry_dir)?;
    let path = telemetry_dir.join("phase_runs.jsonl");
    let mut value = serde_json::to_value(run).map_err(std::io::Error::other)?;
    value["schema_version"] = TELEMETRY_SCHEMA_VERSION.into();
    let line = serde_json::to_string(&value).map_err(std::io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(path)
}
```

**The defect is the last two lines.** `O_APPEND` makes each individual write
atomic with respect to the file offset, but there are *two* of them, so the
sequence is not atomic as a whole. Another appender writing between them produces
`{...A...}{...B...}\n` — two objects on one line, with A's newline effectively
donated to B.

The concurrent writers are routine, not hypothetical: the sweep inside `rexymcp
serve` re-appends the ledger every 60 s (`mcp/src/sweep.rs`) while a finishing
phase run appends its `PhaseRun` and the architect appends a `review`. The real
store carries **209 such lines** in one contiguous band.

**Every reader currently hides this** — `:223`, `:421`, `:585`, `:721` all do
`.filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())`, so a
corrupted line is skipped in silence. **Making that visible is phase 02, not this
phase.** Do not change any reader here.

## Spec

### 1. Add a private shared helper

The bug is one mistake copied four times, so the fix should exist in exactly one
place. Add this private function to `executor/src/store/telemetry.rs`, near the
existing `append` (before it is fine):

```rust
/// Serialize `record`, stamp `schema_version` at the write boundary, and append
/// it to `<telemetry_dir>/phase_runs.jsonl` as **one** buffered write.
///
/// The payload and its trailing newline are built into a single buffer and
/// issued as one `write_all`, so a concurrent appender on the same `O_APPEND`
/// file cannot land between a record and its newline. Writing them as two
/// separate calls is what produced the spliced lines this milestone exists to
/// fix.
///
/// Residual, documented rather than solved: `write_all` will issue more than one
/// `write` syscall if the kernel returns a short count. For regular files of
/// this size on Linux that does not occur in practice, and the alternative
/// (raw `write` with a manual retry loop that cannot retry safely under
/// `O_APPEND`) is worse.
fn append_stamped<T: serde::Serialize>(
    telemetry_dir: &Path,
    record: &T,
) -> std::io::Result<PathBuf> {
    use std::io::Write;

    std::fs::create_dir_all(telemetry_dir)?;
    let path = telemetry_dir.join("phase_runs.jsonl");
    let mut value = serde_json::to_value(record).map_err(std::io::Error::other)?;
    value["schema_version"] = TELEMETRY_SCHEMA_VERSION.into();
    let mut line = serde_json::to_string(&value).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(path)
}
```

Note `line.push('\n')` before the single `write_all` — that is the entire fix.

### 2. Delegate all four public functions to it

Replace each of the four bodies with a delegation. Keep the **public signatures
and doc comments exactly as they are** (adjust only wording that claims two
writes, if any does). For example:

```rust
pub fn append(telemetry_dir: &Path, run: &PhaseRun) -> std::io::Result<PathBuf> {
    append_stamped(telemetry_dir, run)
}
```

Do the same for `append_review`, `append_architect_activity`, and
`append_architect_ledger`. All four must go through the helper — a fix applied to
three of four leaves the race live.

Every existing test must pass **unmodified**. If one needs editing, stop and
report it: these functions' observable behavior is not supposed to change.

### 3. The race-reproduction test

This is the part that matters, and it is why this phase is not a one-line
drive-by. Add `append_is_atomic_under_concurrent_appenders` to the
`#[cfg(test)] mod tests` block in `executor/src/store/telemetry.rs`.

**There is no `thread::spawn` anywhere in this repo's tests**, so here is the
pattern to use — `std::thread` only, no new dependency:

```rust
    #[test]
    fn append_is_atomic_under_concurrent_appenders() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry_dir = dir.path().to_path_buf();

        const THREADS: usize = 8;
        const PER_THREAD: usize = 250;

        let mut handles = Vec::new();
        for _ in 0..THREADS {
            let d = telemetry_dir.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..PER_THREAD {
                    append(&d, &sample()).unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let path = telemetry_dir.join("phase_runs.jsonl");
        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

        // Every line must be exactly one JSON object.
        let mut malformed = 0usize;
        for l in &lines {
            if serde_json::from_str::<serde_json::Value>(l).is_err() {
                malformed += 1;
            }
        }
        assert_eq!(malformed, 0, "spliced/unparseable lines found");
        assert_eq!(
            lines.len(),
            THREADS * PER_THREAD,
            "every append must produce exactly one line"
        );
    }
```

Three things about this test, all deliberate:

- **It is one-sided.** Under the fixed code it can never fail, so it will not
  flake in CI. Under the two-write code it fails with very high probability. That
  asymmetry is the point: the assertion is an invariant, not a coin flip.
- **The line-count assertion carries as much weight as the parse check.** A
  splice destroys *two* lines' worth of framing but yields one line, so the count
  catches cases the parse check might not.
- **No `sleep`, no RNG, no wall-clock** — STANDARDS § Testing. Contention comes
  from thread count and iteration count alone.

**If the mutation (§ End-to-end) does not reproduce the splice, raise `THREADS`
and `PER_THREAD` — do NOT weaken the assertions.** The counts above should
produce it comfortably; they are a starting point, not a ceiling.

## Acceptance criteria

- [ ] `cargo fmt --all --check`, `cargo build`, `cargo clippy --all-targets
      --all-features -- -D warnings`, and `cargo test` all pass.
- [ ] All four append functions delegate to the single helper —
      `grep -c 'write_all' executor/src/store/telemetry.rs` shows the append path
      has exactly **one** `write_all` remaining (other `write_all` uses elsewhere
      in the file, if any, are unrelated and may stay).
- [ ] No existing test was modified. `git diff` over the test module shows
      additions only.
- [ ] Public signatures of all four functions unchanged.
- [ ] Test `append_is_atomic_under_concurrent_appenders` passes.
- [ ] **The mutation goes red** — see End-to-end verification. This is the
      criterion that decides the phase.
- [ ] No reader was changed (phase 02 owns that).

## Test plan

- `append_is_atomic_under_concurrent_appenders` in
  `executor/src/store/telemetry.rs` — 8 threads × 250 `append` calls into one
  `TempDir` store; asserts zero unparseable lines and exactly 2000 lines. Full
  body given in § Spec 3.
- The **existing** append tests (`append_stamps_schema_version` and the
  round-trip/read tests for reviews, activities, and ledgers) are the regression
  net for the delegation refactor. They must pass unmodified — that is what
  demonstrates the helper preserved behavior for all four record types.

No new test is needed per record type for the atomicity fix: all four now share
one code path, and the existing per-type tests prove each still serializes,
stamps, and round-trips.

## End-to-end verification

This phase ships a library-internal change with no CLI surface, so there is no
binary to drive. The **mutation is the verification**, and per STANDARDS § 1.1 it
is what proves the test is live rather than decorative.

**Run this and quote the output in the completion Update Log:**

1. With the fix in place, run `cargo test -p rexymcp-executor append_is_atomic`
   and quote the passing result.
2. **Mutate the helper back to the two-write form** — replace the single
   `write_all` with the original pair:

   ```rust
       // MUTATION: restore the pre-fix two-write form
       file.write_all(line.trim_end().as_bytes())?;
       file.write_all(b"\n")?;
   ```

3. Re-run the same test. It **must fail**. Quote the failure, including the
   assertion message and the malformed/line counts it reports.
4. Restore the fix and confirm the full `cargo test` is green again.

If step 3 passes instead of failing, the test is not exercising the race —
increase `THREADS`/`PER_THREAD` and repeat until it fails reliably (try it three
times to be sure it is not intermittent). **Do not proceed by weakening the
assertions, and do not report the phase complete on a green suite alone** — a
green suite is exactly what the pre-fix code also produced.

**Also confirm the real-store shape is unchanged**: after the fix, run
`cargo test -p rexymcp-executor telemetry` and quote the count. Every existing
telemetry test passing unmodified is the evidence that the delegation refactor
did not alter behavior for any of the four record types.

## Authorizations

- [ ] May add dependencies: **No.** `std::thread` is in the standard library and
      `tempfile` is already a dev-dependency.
- [ ] May touch `docs/architecture.md`: **No.**

Everything else: None.

## Out of scope

- **Any reader change.** The four `filter_map(... .ok())` sites at `:223`, `:421`,
  `:585`, `:721` stay exactly as they are. Making malformed lines visible is
  **phase 02**, and it carries a design decision this phase must not pre-empt.
- **Repairing the 209 existing corrupt lines**, or recovering the ~418 ledger
  records inside them. M43's compaction already dropped them from the live store
  and its backup retains them.
- **Changing the record types, the `schema_version` constant, or the stamping
  semantics.** The helper must stamp exactly as the four functions do today.
- **Introducing a file lock, a mutex, or a write queue.** One buffered write is
  the fix; adding a lock would be a different (and heavier) design, and it is not
  authorized.
- **`fsync`/durability.** Not this defect. The store has never fsynced and this
  phase does not change that.
- **Touching `mcp/src/sweep.rs`** or anything else that *calls* these functions.
  The fix is entirely inside the append path.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
