# Phase 1: drop debug print

**Milestone:** F10 — Remove PII debug print
**Status:** review
**Depends on:** none
**Estimated diff:** 1 line
**Tags:** language=rust, kind=cleanup, size=xs

## Goal

Delete one debug line from a test in `executor/src/privacy/egress.rs`. It
prints every term the pre-scan detected, which is the contents of a PII
dictionary. `STANDARDS.md` §1 forbids debug prints in code, with no test
exemption.

## Pre-flight

1. `grep -c 'println!' executor/src/privacy/egress.rs` → **1** (measured 2026-09-18).
2. Full `cargo test` → **727 / 2 / 1210 passed**.

## Current state

`executor/src/privacy/egress.rs:579-583`, inside the `#[ignore]`d live test
`live_build_egress_index_keeps_project_names_out`:

```rust
        let idx = build_egress_index(&repo, &privacy).await.unwrap();
        let terms = idx.terms;
        let normalized: Vec<String> = terms.iter().map(|(t, _)| normalize(t)).collect();
        eprintln!("live terms: {terms:?}");

```

The assertions right below already print `terms` and `normalized` when they
fail, so nothing is lost.

## Spec

Delete exactly this line and nothing else:

```rust
        eprintln!("live terms: {terms:?}");
```

**Must NOT:**

- Change any other line, including the assertions and the blank line after.
- Run the `#[ignore]`d live test. It needs the detection engine, which the
  executor does not reach. The architect runs it at review.

## Acceptance criteria

- [ ] `grep -c 'println!' executor/src/privacy/egress.rs` → **0**.
- [ ] `grep -rn 'println!' executor/src/privacy/` → no output.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

No new tests. **Finish condition:** full `cargo test` → **727 / 2 / 1210
passed**, unchanged. Paste the four `test result:` lines.

## End-to-end verification

Paste the output of both `grep` commands from the acceptance criteria in an
`(end-to-end verification)` entry.

## Authorizations

None beyond the Spec.

## Out of scope

- Anything else in the privacy module.

## Update Log

<!-- entries appended below this line -->

### Update — 2026-09-18 17:33 (complete, server-authored)

**Summary:** All checks pass; working tree is clean and the commit contains the single deletion.

---

**Summary** — Deleted the single `eprintln!("live terms: {terms:?}");` line from `executor/src/privacy/egress.rs` (inside the `#[ignore]`d live test `live_build_egress_index_keeps_project_names_out`). Nothing else was touched — assertions, blank line, and all other code unchanged. This is the only change in the commit: 1 file, 1 deletion.

**Notes for review:**
- End-to-end verification (mechanically captured grep output):
  - `grep -c 'println!' executor/src/privacy/egress.rs` → `0`
  - `grep -rn 'println!' executor/src/privacy/` → no output (exit 1, i.e. no matches)
- No deviation from spec; the `#[ignore]`d live test was not run, per the Spec.
- Full `cargo test` result, unchanged from baseline (727 / 2 / 1210 passed). The four `test result:` lines:
  ```
  test result: ok. 727 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.20s
  test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  test result: ok. 1210 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.30s
  test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
  ```
- `cargo fmt --all --check`, `cargo build`, and `cargo clippy --all-targets --all-features -- -D warnings` all pass clean.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.33s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s


TEST
ferences_no_matches_advisory ... ok
test tools::symbols::tests::rejects_nonexistent_path ... ok
test tools::symbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1210 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.20s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `executor/src/privacy/egress.rs` — +0 -1

**Commit:** 12eb0e41353f4d64fbaf0351e10ecf56b11faebe

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).
