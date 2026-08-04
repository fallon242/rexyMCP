# Phase 03: skip unchanged ledger appends

**Milestone:** M43 — Dashboard Idle CPU
**Status:** review
**Depends on:** phase-02 (done — `telemetry::read_all`, which this phase uses to
read the current ledger state in one pass)
**Estimated diff:** ~180 lines
**Tags:** language=rust, kind=bugfix, size=m

## Goal

Stop `rexymcp serve` appending ~143 identical ledger records to
`phase_runs.jsonl` every 60 seconds. Harvest re-derives every bucket from the
whole transcript corpus and appends all of them regardless of whether anything
changed; `fold_ledger` then throws away all but the newest per key at read time.
Append only the buckets that actually differ from what the store already holds.

## Architecture references

Read before starting:

- `docs/dev/milestones/M43-dashboard-idle-cpu/README.md` § "The three multiplied
  factors", factor 3 — the write amplification this phase fixes.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture reference above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

`harvest()` (`mcp/src/harvest.rs:214`) builds one `ArchitectLedger` per
`(session_id, model, skill)` accumulator and appends **every** one
(`mcp/src/harvest.rs:307–332`):

```rust
    // Build ledger records from accumulators, sorted for deterministic output
    let mut total_messages = 0usize;
    let mut total_records = 0usize;
    for (key, acc) in accum {
        let ledger = ArchitectLedger {
            record: ARCHITECT_LEDGER_RECORD_TAG.to_string(),
            project_id: _project_id.clone(),
            session_id: key.0,
            model: key.1,
            skill: key.2,
            tokens: ArchitectTokens { /* … */ },
            cache_creation_5m: acc.cache_creation_5m,
            cache_creation_1h: acc.cache_creation_1h,
            messages: acc.messages,
            last_ts: acc.last_ts,
        };
        if let Err(e) = append_architect_ledger(&telemetry_dir, &ledger) {
            eprintln!("warning: failed to append ledger record: {}", e);
        }
        total_messages += acc.messages as usize;
        total_records += 1;
    }
```

Nothing consults the existing store first. The sweep inside `serve`
(`mcp/src/sweep.rs:142`) calls this every 60 s whenever any transcript's mtime
moved — which is continuously while the architect works — so one edited session
re-appends all ~143 buckets. That is how the store reached 103 MB / 278,836 lines
of which 278,226 are ledger records folding down to ~143.

`fold_ledger` (`executor/src/store/telemetry.rs:666`) defines the identity that
matters — **last write wins on a four-part key**:

```rust
pub fn fold_ledger(ledgers: Vec<ArchitectLedger>) -> Vec<ArchitectLedger> {
    use std::collections::HashMap;
    let mut latest: HashMap<(Option<String>, String, String, String), usize> = HashMap::new();
    let mut out: Vec<ArchitectLedger> = Vec::new();
    for l in ledgers {
        let key = (
            l.project_id.clone(),
            l.session_id.clone(),
            l.model.clone(),
            l.skill.clone(),
        );
        // …replace in place if the key was seen, else push…
    }
    out
}
```

`ArchitectLedger` derives `PartialEq` (`executor/src/store/telemetry.rs:603`), so
"has this bucket changed?" is a plain `==`. `schema_version` is **not** a struct
field — it is injected at the write boundary (`:698`) — so it does not
participate in the comparison, which is what you want.

`HarvestOutcome` (`mcp/src/harvest.rs:27`):

```rust
pub struct HarvestOutcome {
    pub path: PathBuf,
    pub messages: usize,
    pub duplicates: usize,
    pub sessions: usize,
    pub records: usize,
}
```

## Spec

### 1. Read the current ledger state once, before the append loop

In `harvest()` (`mcp/src/harvest.rs`), immediately before the
`for (key, acc) in accum` loop, read what the store already holds and index it by
`fold_ledger`'s key:

```rust
    // What the store already holds, folded to one record per key. Appending a
    // record identical to the folded state is pure write amplification: the
    // reader would discard it immediately.
    let existing: std::collections::HashMap<
        (Option<String>, String, String, String),
        ArchitectLedger,
    > = fold_ledger(
        telemetry::read_all(&store_path)
            .map(|s| s.ledgers)
            .unwrap_or_default(),
    )
    .into_iter()
    .map(|l| {
        (
            (
                l.project_id.clone(),
                l.session_id.clone(),
                l.model.clone(),
                l.skill.clone(),
            ),
            l,
        )
    })
    .collect();
```

Use `telemetry::read_all` (added in phase 02) rather than
`read_architect_ledger` — one pass, no `serde_json::Value` round-trip. Read from
**`store_path`**, the exact path the appends target, so a `--telemetry-path`
override is honored on both sides.

A read error must **not** abort the harvest: `.unwrap_or_default()` yields an
empty map, and an empty map means "nothing matches, append everything" — the
current behavior. Degrading to today's behavior on an unreadable store is the
correct failure direction.

### 2. Skip appends whose record is unchanged

Inside the loop, after building `ledger`, compare against the folded state and
append only on a difference:

```rust
        let key = (
            ledger.project_id.clone(),
            ledger.session_id.clone(),
            ledger.model.clone(),
            ledger.skill.clone(),
        );
        if existing.get(&key) == Some(&ledger) {
            total_messages += acc.messages as usize;
            total_unchanged += 1;
            continue;
        }
        if let Err(e) = append_architect_ledger(&telemetry_dir, &ledger) {
            eprintln!("warning: failed to append ledger record: {}", e);
        }
        total_messages += acc.messages as usize;
        total_records += 1;
```

Three things this must get right:

- **Compare the whole record, not just key presence.** `existing.contains_key(&key)`
  would skip a bucket whose token totals grew — silently losing every update after
  the first. The comparison is `== Some(&ledger)`.
- **`total_messages` stays unconditional.** It counts messages *processed*, not
  records written. `harvest_is_idempotent` (`mcp/src/harvest.rs:578`) asserts
  `outcome2.messages == 1` on a second run over unchanged fixtures; that test must
  pass **unmodified**. If you find yourself editing it, stop and file a blocker.
- **`record` must be set on the candidate before comparing.** The store's records
  deserialize with `record == "architect_ledger"`; the candidate sets the same at
  `harvest.rs:311`. If the candidate had `record: String::new()`, every comparison
  would fail and nothing would ever be skipped — a silent no-op fix. Pin this with
  the test below.

### 3. Report what was skipped

Add a field to `HarvestOutcome` (additive — `harvest()` is its only construction
site):

```rust
    /// Buckets whose record was byte-identical to the store's folded state and
    /// therefore not appended.
    pub unchanged: usize,
```

Then surface it in the two places that report a harvest:

- `mcp/src/sweep.rs:144` — the liveness marker written to `sweep_state.json`:
  ```rust
  let outcome = format!(
      "{} new / {} unchanged / {} msgs",
      o.records, o.unchanged, o.messages
  );
  ```
  No test asserts this string (only the `"no change"` and
  `"skipped: no transcript dir"` outcomes are pinned, at `sweep.rs:338` and
  `:351`), so extending it is safe.
- `mcp/src/main.rs:1084` — the `rexymcp harvest` CLI printout. Add the unchanged
  count to the existing line; keep the existing fields.

## Acceptance criteria

- [ ] `cargo build` succeeds.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff. (Fix with `rustfmt <file>` on
      touched files only — never `cargo fmt --all`.)
- [ ] `cargo test` passes, with `harvest_is_idempotent` **unmodified**.
- [ ] A second consecutive `rexymcp harvest` over an unchanged transcript corpus
      appends **zero** bytes — verified end-to-end below.

## Test plan

In `mcp/src/harvest.rs`'s existing `#[cfg(test)] mod tests` (reuse
`make_config` / `write_fixture` / the `TempDir` style already there):

- `harvest_skips_appending_unchanged_records` — harvest twice over one unchanged
  fixture; assert the second outcome has `records == 0` and `unchanged == 1`, and
  that the store's **line count is identical** before and after the second run.
  The line-count assertion is the one that would catch a comparison that never
  matches.
- `harvest_appends_when_a_bucket_changes` — harvest, then append a *second*
  message to the same session fixture, then harvest again; assert the second
  outcome has `records == 1` and `unchanged == 0`, and that folding the store
  yields the **summed** totals. This is the negative case for §2's
  "compare the whole record" rule — a key-presence-only check passes the first
  test and fails this one.
- `harvest_appends_everything_into_an_empty_store` — first harvest into a fresh
  `TempDir` appends all buckets and reports `unchanged == 0`.
- `harvest_candidate_carries_the_record_tag` — after one harvest, read the store
  back and assert every ledger record has `record == ARCHITECT_LEDGER_RECORD_TAG`.
  Guards the silent-no-op failure mode in §2.

Determinism: no `sleep`, no wall clock, no new crate. The fixtures already carry
fixed timestamps — keep using them.

## End-to-end verification

The artifact is the running binary's write behavior against a real transcript
corpus. Verify with a **scratch copy** of the store so the real one is never
mutated.

```bash
cargo build --release
SP=$(mktemp -d)
head -60000 ~/.rexymcp/telemetry/phase_runs.jsonl > "$SP/store.jsonl"
TX=~/.claude/projects/-home-matt-src-rexyMCP

before=$(wc -l < "$SP/store.jsonl")
target/release/rexymcp harvest --config rexymcp.toml --transcript-dir "$TX" \
  --telemetry-path "$SP/store.jsonl"
mid=$(wc -l < "$SP/store.jsonl")
target/release/rexymcp harvest --config rexymcp.toml --transcript-dir "$TX" \
  --telemetry-path "$SP/store.jsonl"
after=$(wc -l < "$SP/store.jsonl")

echo "before=$before after-first=$mid after-second=$after"
echo "second harvest appended $((after - mid)) lines"
```

The **second** harvest must append **0 lines** (`after == mid`), and its printout
must report `0` new with a non-zero unchanged count. The first harvest may append
records — the scratch store is a 60k-line prefix, so some buckets legitimately
differ from the full corpus. Quote the literal `before=… after-first=… after-second=…`
line and the second harvest's printout in the completion Update Log.

> **Measurement discipline (M43 phases 01–02 lesson, twice burned).** State the
> result as a **difference you observe in one session** — here, `after - mid`
> lines — not as an absolute number carried in from elsewhere. And assert the
> thing you measured actually ran: if the harvest command errors, `after == mid`
> is also true, and a broken command would read as a perfect pass. Check the
> command's exit status and that its printout appeared before believing the zero.

## Authorizations

None. No new dependency, no `Cargo.toml` edit. Touches `mcp/src/harvest.rs`,
`mcp/src/sweep.rs`, and the `Commands::Harvest` printout in `mcp/src/main.rs`.

## Out of scope

- **Compacting the existing 103 MB store.** This phase stops the *growth*; it does
  not reclaim what is already written. Compaction rewrites the user's telemetry
  file and so carries a data-migration surface that deserves its own review —
  it is **phase 06**. Do not add a compaction pass, and do not rewrite or truncate
  `phase_runs.jsonl` here.
- **Auto-compaction inside the sweep.** Same reason, more so: a background process
  rewriting the store unattended is exactly the shape that loses data.
- **Changing `fold_ledger`'s key or last-write-wins semantics.** The dedup identity
  is the contract this phase reads; leave it alone.
- **The `schema_version` divergence** (phase 05) and **the render path** (phase 04).
- **Making the added store read conditional or incremental.** This phase adds one
  full read of the store per harvest (≈150 ms against today's 103 MB file, once
  per 60 s sweep tick) in exchange for stopping ~53 KB/minute of appends. That
  trade is deliberate and it improves further once phase 06 shrinks the file.
  If the read shows up as a problem, that is a finding for a later phase, not a
  reason to complicate this one.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-04 19:59 (started)

**Executor:** Claude (executor)

Implemented skip-unchanged-ledger-appends: `harvest()` now reads the current store via `telemetry::read_all`, folds it, and compares each candidate ledger against the folded state before appending. Added `unchanged` field to `HarvestOutcome`, updated sweep and CLI printouts, and added 4 new tests.

### Update — ts=1785874180566 (complete, server-authored)

**Summary:** **Summary + Notes for review:**

I implemented skip-unchanged-ledger-appends in `harvest()`: before the append loop, the store is read via `telemetry::read_all`, folded to one record per `(project_id, session_id, model, skill)` key, and each candidate ledger is compared with `==` against the folded state. Identical buckets are skipped entirely, adding `unchanged` to `HarvestOutcome`. The sweep liveness marker and CLI printout now surface the skip count.

All 4 new tests pass (`harvest_skips_appending_unchanged_records`, `harvest_appends_when_a_bucket_changes`, `harvest_appends_everything_into_an_empty_store`, `harvest_candidate_carries_the_record_tag`), and `harvest_is_idempotent` was left unmodified.

**End-to-end verification:** Against a 60k-line scratch copy of the real store, the second consecutive harvest appended **0 lines** and reported `0 ledger records (145 unchanged, 7341 duplicates skipped)`. Literal output: `before=60000 after-first=60000 after-second=60000`, confirming `after == mid`.

**Executor:** Qwen/Qwen3.6-27B-FP8

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s


TEST
ending_task_to_active ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::symbols::tests::no_symbols_returns_advisory_error ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test governor::verifier::tests::verify_rust_returns_checked_empty_on_clean_code ... ok
test governor::verifier::tests::capture_baseline_dedupes_by_project_root ... ok
test governor::verifier::tests::verify_rust_returns_checked_with_errors_on_broken_code ... ok
test governor::verifier::tests::capture_baseline_skips_unsupported_files ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1061 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.10s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.08s
     Running unittests src/main.rs (target/debug/deps/rexymcp-4e85b51f198fbe9f)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-4bde71b966d323ae)
     Running unittests src/lib.rs (target/debug/deps/executor-c1650299697d7408)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/M43-dashboard-idle-cpu/README.md` — +1 -1
- `docs/dev/milestones/M43-dashboard-idle-cpu/phase-03-skip-unchanged-ledger-appends.md` — +7 -1
- `mcp/src/harvest.rs` — +204 -0
- `mcp/src/main.rs` — +2 -1
- `mcp/src/sweep.rs` — +4 -1

**Commit:** acae94ee95ae827cf24ca4aa8e29920bc5e25c16

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).
