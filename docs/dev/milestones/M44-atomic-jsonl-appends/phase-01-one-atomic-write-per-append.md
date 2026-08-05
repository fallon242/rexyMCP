# Phase 01: one atomic write per telemetry append

**Milestone:** M44 — Atomic JSONL Appends
**Status:** review
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

### Update — 2026-08-05 15:52 (started)

**Executor:** claude-opus-4-5-20251101
**Status:** started

### Update — ts=1785945610674 (complete, server-authored)

**Summary:** **Summary + Notes for review:**

Built `append_stamped<T>` — a single private generic helper that serializes a record, stamps `schema_version`, appends the newline to the serialized line, and issues exactly one `write_all` call. All four public append functions (`append`, `append_review`, `append_architect_activity`, `append_architect_ledger`) now delegate to it with unchanged signatures. Added `append_is_atomic_under_concurrent_appenders` test (8 threads × 250 iterations) that asserts zero malformed lines and exactly 2000 lines total.

**End-to-end verification:**
- Fixed code: `cargo test -p rexymcp-executor append_is_atomic` — **passed** (0 malformed, 2000 lines).
- Mutation (restored two-write form): **failed** with `left: 436` malformed lines — the test is live and catches the race.
- All 1062 tests pass; `cargo test -p rexymcp-executor telemetry` — 69 passed, confirming no regression across all four record types.
- `grep -c 'write_all' executor/src/store/telemetry.rs` = 11 (1 in the append path at `:222`, 10 in test setup code — all correct).
- No readers were changed. No existing tests were modified.

**Executor:** Qwen/Qwen3.6-27B-FP8

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
   Compiling rexymcp-executor v0.9.1 (/home/matt/src/rexyMCP/executor)
   Compiling rexymcp v0.9.1 (/home/matt/src/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.48s


LINT
    Checking rexymcp-executor v0.9.1 (/home/matt/src/rexyMCP/executor)
    Checking rexymcp v0.9.1 (/home/matt/src/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.39s


TEST
s::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::symbols::tests::references_finds_call_sites ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::finds_python_function_and_class ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test governor::verifier::tests::verify_rust_returns_checked_empty_on_clean_code ... ok
test governor::verifier::tests::capture_baseline_dedupes_by_project_root ... ok
test governor::verifier::tests::verify_rust_returns_checked_with_errors_on_broken_code ... ok
test governor::verifier::tests::capture_baseline_skips_unsupported_files ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1062 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.12s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests src/main.rs (target/debug/deps/rexymcp-4e85b51f198fbe9f)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-4bde71b966d323ae)
     Running unittests src/lib.rs (target/debug/deps/executor-c1650299697d7408)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/M44-atomic-jsonl-appends/README.md` — +1 -1
- `docs/dev/milestones/M44-atomic-jsonl-appends/phase-01-one-atomic-write-per-append.md` — +6 -1
- `executor/src/store/telemetry.rs` — +71 -49

**Commit:** cccdc9f808a7b1e27120f04ae4cb2dea547c9dd3

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).
