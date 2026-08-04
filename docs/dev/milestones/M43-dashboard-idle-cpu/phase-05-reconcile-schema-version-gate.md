# Phase 05: reconcile the `schema_version` gate divergence

**Milestone:** M43 — Dashboard Idle CPU
**Status:** review
**Depends on:** phase-02 (introduced `read_all`), phase-04 (done)
**Estimated diff:** ~90 lines (≈10 production, the rest test fixtures)
**Tags:** language=rust, kind=bugfix, size=s

## Goal

The dashboard and `rexymcp costs` disagree on this project's totals by 2.4×,
because one reader gates `PhaseRun` records on `schema_version` and the other
does not. This phase makes them agree by **adding the gate** to the ungated
side. `docs/architecture.md` §35 already decided the direction — pre-M35 records
go dark — so the dashboard is the outlier, and this phase brings it in line.

## Architecture references

Read before starting:

- `docs/architecture.md` §35 (M35 — Metrics & Cost Overhaul, design fork 4) —
  states telemetry is version-gated and that **backward compatibility is
  deliberately waived: "pre-M35 records go dark."** This is the decision this
  phase implements. Do not revisit it.
- `docs/architecture.md` §43 (M43 — Dashboard idle CPU) — the milestone this
  phase belongs to.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom — including **§1.1 "An end-to-end
   verification must prove it is live"**, which this phase's End-to-end
   verification section is written against.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

There are two readers of `PhaseRun` records and they filter differently.

**The gated one** — `executor/src/store/telemetry.rs:214`, used by `rexymcp
costs`, `rexymcp runs`, `scorecard`, `profile`, and the MCP server:

```rust
/// Read all `PhaseRun` records from a store file. Records with a missing or
/// non-current `schema_version` are skipped (pre-M35 retirement).
pub fn read(path: &Path) -> std::io::Result<Vec<PhaseRun>> {
    // ...
        .filter(|v| {
            v.get("schema_version").and_then(serde_json::Value::as_u64)
                == Some(TELEMETRY_SCHEMA_VERSION as u64)
        })
    // ...
}
```

**The ungated one** — the `""` (no-discriminator) arm of `read_all`,
`executor/src/store/telemetry.rs:797`, which the dashboard uses via
`load_data` (`mcp/src/dashboard/mod.rs:95`):

```rust
            "" => {
                if let Ok(r) = serde_json::from_str::<PhaseRun>(line) {
                    records.runs.push(r);
                }
            }
```

Note there is **no** `head.schema_version == TELEMETRY_SCHEMA_VERSION` check
here, unlike the two arms directly above it (`ARCHITECT_LEDGER_RECORD_TAG` at
`:783` and `ARCHITECT_ACTIVITY_RECORD_TAG` at `:790`), which both have it.

Phase 02 preserved this divergence deliberately and documented it on the field
it produces (`executor/src/store/telemetry.rs:736`):

```rust
    /// Every line that deserializes as a `PhaseRun`, with **no**
    /// `schema_version` gate. This matches the dashboard's `read_phase_runs`
    /// and deliberately does **not** match `read` (`:214`), which gates.
    /// The divergence is real and pre-existing (it makes the dashboard and
    /// `rexymcp costs` disagree); reconciling it is M43 phase-05, NOT this phase.
    pub runs: Vec<PhaseRun>,
```

**The measured effect**, taken from the live store at
`/home/matt/.rexymcp/telemetry/phase_runs.jsonl` on 2026-08-04: of the
`PhaseRun` lines in the store, **357 are unversioned** (pre-M35) and **183 carry
`schema_version: 1`**. The ungated reader picks up both populations; the gated
one picks up only the second. On this project that is a 2.4× disagreement in
runs and executor token totals.

**Which surfaces are affected.** `StoreRecords.runs` feeds only
`costs::scope_costs` in `load_data` (`mcp/src/dashboard/mod.rs:103` and `:116`),
which produces `DashboardData::project_costs` and `milestone_costs` — the
dashboard's Budget panel. `milestone_costs` is additionally scoped to the active
milestone directory, so in practice it is already unaffected (pre-M35 runs
belong to closed milestones). **The visible change is the Budget panel's Project
column dropping to match `rexymcp costs`.** That drop is the intended outcome of
this phase, not a regression.

## Spec

1. **Gate the `PhaseRun` arm of `read_all`** — in
   `executor/src/store/telemetry.rs`, the `""` match arm (`:797`), add the same
   `schema_version` check the ledger and activity arms already carry, so the arm
   reads:

   ```rust
            "" => {
                if head.schema_version == TELEMETRY_SCHEMA_VERSION
                    && let Ok(r) = serde_json::from_str::<PhaseRun>(line)
                {
                    records.runs.push(r);
                }
            }
   ```

   Use the `let`-chain form shown above — it is the idiom the two arms directly
   above already use (edition 2024). `RecordHead::schema_version` is
   `#[serde(default)]`, so a line with no `schema_version` field parses as `0`
   and fails the check, which is exactly the pre-M35 case.

2. **Update the `StoreRecords::runs` doc comment** — in the same file (`:736`),
   replace the "no `schema_version` gate … reconciling it is M43 phase-05"
   paragraph with a statement of the new behavior. It must say that `runs` is
   gated on `schema_version == TELEMETRY_SCHEMA_VERSION` and is therefore
   identical to `read` (`:214`), and reference the pre-M35 retirement. Do not
   leave the old text or a "was previously ungated" note — the comment describes
   what the code does now.

3. **Fix the `write_phase_run_line` test helper** — in the same file (`:1983`),
   it currently serializes `sample()` directly, which produces a line with **no**
   `schema_version` (the field lives at the write boundary in `append`, not on
   the `PhaseRun` struct). Change it to go through `append(dir, &sample())` so
   the line it writes is stamped, and add a sibling helper
   `write_legacy_phase_run_line(dir: &Path)` that writes the unstamped form (the
   current body) for negative-case tests. Note `append` uses `OpenOptions`
   append mode while the current helper uses `std::fs::write` (truncating) —
   check the three call sites (`:2053`, `:2069`, `:2116`) still express what
   they mean once writes accumulate rather than overwrite.

4. **Invert the gate test** — `read_all_runs_are_not_schema_version_gated`
   (`:2065`) asserts the old behavior and must be replaced by
   `read_all_runs_are_schema_version_gated`, which seeds an unstamped run via
   the new legacy helper and asserts `records.runs.is_empty()`.

5. **Extend the cross-reader equivalence test** —
   `read_all_matches_per_type_readers_on_the_same_file` (`:2114`) currently
   compares only `activities` and `ledgers` against their per-type readers. Add
   the runs comparison: `records.runs.len()` must equal `read(&path).unwrap().len()`
   on the same file. Seed the fixture with **both** a stamped and an unstamped
   run so the assertion is non-trivial (with only stamped runs, both counts
   would agree even if the gate were missing).

6. **Stamp the dashboard test fixtures that must still count** — in
   `mcp/src/dashboard/mod.rs`, the raw JSON run fixtures at `:400`, `:403`,
   `:440`, `:443`, and `:479` are unstamped string literals and will now be
   filtered out, breaking their tests. Add `"schema_version":1,` to each so the
   tests keep asserting what they were written to assert. **Leave `legacy_run`
   at `:446` unstamped** — it is a deliberate negative case.

7. **Strengthen the legacy negative case** — the `legacy_run` fixture at `:446`
   is currently excluded because it has no `project_id`, so it would still pass
   if the gate did nothing. Give it the test's own `this_pid` as its
   `project_id` while leaving it unstamped, and update the comment at `:445` and
   the two assertion messages at `:464`/`:468` to say the record is excluded by
   the `schema_version` gate. The totals asserted (`1000` / `500`) must not
   change — that is the point: a project-matching record still does not count.

## Acceptance criteria

Verifiable conditions — each one checkable by running a command or reading a file.

- [ ] `cargo test -p rexymcp-executor telemetry` passes.
- [ ] `cargo test -p rexymcp load_data` passes.
- [ ] `cargo test` passes with no test deleted and no test `#[ignore]`d.
- [ ] Test `read_all_runs_are_schema_version_gated` passes.
- [ ] Test `read_all_matches_per_type_readers_on_the_same_file` passes with the
      runs comparison added, against a fixture containing both a stamped and an
      unstamped run.
- [ ] Test `load_data_project_savings_excludes_other_projects` passes with the
      legacy fixture carrying a matching `project_id`.
- [ ] `grep -n 'M43 phase-05' executor/src/store/telemetry.rs` returns nothing
      (the deferral note is gone because the work is done).
- [ ] Against the real store, the dashboard's Budget-panel **Project executor**
      figure equals `rexymcp costs`' **Project Executor** figure. See End-to-end
      verification for the required positive control.

## Test plan

Concrete tests to write — names + what they assert. Hermetic (`TempDir` only),
no real store, no network.

- `read_all_runs_are_schema_version_gated` in
  `executor/src/store/telemetry.rs` — seeds one unstamped `PhaseRun` line via
  `write_legacy_phase_run_line`, asserts `read_all(...).runs.is_empty()`.
  Replaces `read_all_runs_are_not_schema_version_gated`.
- `read_all_matches_per_type_readers_on_the_same_file` (extended) — seeds one
  stamped run, one unstamped run, one activity, one ledger, one review; asserts
  `records.runs.len() == read(&path).unwrap().len()` **and** that the shared
  count is `1`, not `2`. Asserting only equality would pass if both readers were
  broken in the same direction.
- `load_data_project_savings_excludes_other_projects` (strengthened) — the
  legacy fixture now carries the test's own `project_id`, so it is excluded on
  the gate alone. Asserted totals stay `1000` / `500`.
- The remaining `load_data_*` tests are fixture-only edits and must pass
  **unmodified in their assertions** — if an assertion value needs changing to
  make a test green, that is a signal the gate is filtering something it
  shouldn't; stop and report it rather than adjusting the expected number.

## End-to-end verification

This phase changes a number the user reads off a running binary, so a hermetic
test cannot close it. Verify against the real store, and follow **STANDARDS
§1.1** — the check must be able to fail.

**The measurement.** Build the phase-05 binary. Then, in one session:

1. Run `rexymcp costs --config rexymcp.toml` and record the **Project /
   Executor** figure.
2. Render `rexymcp dashboard --repo .` and read the Budget panel's **Project
   Executor** figure. It is a TUI, so use the harness phase 04 established: a
   **detached `tmux` pane sized 200×50**, then `tmux capture-pane -p` to read it
   back. Do **not** use `script` — it provides no terminal size, ratatui draws
   nothing, and every grep then fails identically whether the code works or not.
3. The two figures must be equal.

**The positive control (required).** Equality on its own is not evidence — two
readers that both returned nothing would also be equal. Run the *same* two steps
against the **phase-04 binary** (`git stash` or a build from commit `688d81e`)
and show that there the two figures **differ**. That A/B in one session is what
proves the harness can detect the divergence, which is what makes the phase-05
equality meaningful.

Quote all four numbers in the completion Update Log — pre-fix costs, pre-fix
dashboard, post-fix costs, post-fix dashboard.

**Do not pin absolute token totals as the criterion.** The store grows between
sessions; this milestone has already been bitten twice by criteria stated as
absolute thresholds against a drifting baseline. The criterion is the
**relationship** (equal after, unequal before), not any particular figure.

**Confirm the subject.** Check the dashboard pid via `/proc/<pid>/comm`, confirm
it was alive at capture time, and confirm both commands exited `0`.

## Authorizations

- [ ] May touch `docs/architecture.md`: **No.** §35 already states the decision
      this phase implements; nothing needs to change there.
- [ ] May add dependencies: **No.**

Everything else: None.

## Out of scope

- **Compacting or rewriting the store.** That is phase 06. This phase does not
  delete, rewrite, back up, or migrate a single line of
  `phase_runs.jsonl` — it only changes which lines a reader *ignores*.
- **Backfilling `schema_version` onto the 357 legacy records.** This was
  considered and rejected by the user on 2026-08-04 in favor of honoring §35 as
  written. Do not add a stamping pass.
- **Changing `read` (`:214`), `read_reviews`, `read_architect_activities`, or
  `read_architect_ledger`.** They are already correct; this phase moves the
  outlier to them, not the reverse.
- **Re-tuning any dashboard panel that now shows a smaller number.** The Budget
  Project column dropping is the intended result. Do not add a "legacy runs
  excluded" caption, footnote, or toggle — if that turns out to be wanted, it is
  a separate phase.
- **Widening to the `.bak*` files** in the telemetry dir. They are not read by
  anything; leave them alone.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-04 22:50 (started)

**Executor:** Claude (Sonnet 4.5)

Implementing schema_version gate on the `PhaseRun` arm of `read_all` to reconcile the dashboard and `rexymcp costs` divergence.

### Update — ts=1785884626439 (complete, server-authored)

**Summary:** All verification commands passed and the working tree is clean.

---

**Summary + Notes for review:**

Added `schema_version == TELEMETRY_SCHEMA_VERSION` gate to the `""` (PhaseRun) arm of `read_all` in `executor/src/store/telemetry.rs`, matching the pattern already used by the ledger and activity arms. Updated the `StoreRecords::runs` doc comment to describe the new gated behavior. Fixed the `write_phase_run_line` test helper to go through `append()` (stamping lines) and added `write_legacy_phase_run_line` for negative-case tests. Inverted `read_all_runs_are_not_schema_version_gated` to `read_all_runs_are_schema_version_gated`. Extended the cross-reader equivalence test with a runs comparison asserting count == 1 against both a stamped and unstamped fixture. Stamped 5 dashboard test fixtures with `"schema_version":1` and strengthened the legacy negative case by giving it a matching `project_id` so exclusion is purely by the gate.

**End-to-end verification (positive control):**
- Phase-04 binary (commit 688d81e): `rexymcp costs` Project Executor = **$1459.29**, dashboard Budget Project Executor = **$3460.33** — **unequal** (2.4× divergence, as expected)
- Phase-05 binary: `rexymcp costs` Project Executor = **$1459.29**, dashboard Budget Project Executor = **$1459.29** — **equal**
- Dashboard process confirmed alive via `/proc/<pid>/comm` showing `rexymcp`, both commands exited 0.

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
_single_file_path ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::symbols::tests::references_exclude_strings_and_comments ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::finds_python_function_and_class ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test governor::verifier::tests::verify_rust_returns_checked_empty_on_clean_code ... ok
test governor::verifier::tests::capture_baseline_dedupes_by_project_root ... ok
test governor::verifier::tests::capture_baseline_skips_unsupported_files ... ok
test governor::verifier::tests::verify_rust_returns_checked_with_errors_on_broken_code ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1061 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.09s


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
- `docs/dev/milestones/M43-dashboard-idle-cpu/phase-05-reconcile-schema-version-gate.md` — +7 -1
- `executor/src/store/telemetry.rs` — +31 -15
- `mcp/src/dashboard/mod.rs` — +10 -8

**Commit:** d14e649cfcfd288acd3fbedf4bbabef149881acd

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).
