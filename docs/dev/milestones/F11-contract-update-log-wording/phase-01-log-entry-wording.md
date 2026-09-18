# Phase 1: log entry wording

**Milestone:** F11 — Contract Update Log wording
**Status:** review
**Depends on:** none
**Estimated diff:** ~40 lines
**Tags:** language=rust, kind=docs, size=xs

## Goal

Make four replacements in `executor/templates/executor_contract.md` so the
contract stops saying the started entry is the executor's only Update Log
entry, and requires an `(end-to-end verification)` entry. Pin both with tests.

## Pre-flight

1. `cargo test -p rexymcp-executor contract` → **11 passed** (measured 2026-09-18).
2. Full `cargo test` → **727 / 2 / 1210 passed**.

## Spec

Make exactly these four replacements. Every other line stays byte-identical.
Keep each phrase the tests check on a single line — `contains` does not match
across a line break.

### 1. Step 2 (line 78)

Replace:

```
   into the completion entry. This is the only Update Log entry you write.
```

with:

```
   into the completion entry. You also write progress and blocker entries
   (steps 3-4) and one end-to-end entry (step 5); never a `(complete)` entry.
```

### 2. Step 5 (lines 82-85)

Replace:

```
   `{TEST_COMMAND}`) and confirm they pass. The loop re-runs them as the final
   gate set; a failing gate sends the feedback back to you to fix.
```

with:

```
   `{TEST_COMMAND}`) and confirm they pass. The loop re-runs them as the final
   gate set; a failing gate sends the feedback back to you to fix.
   Then append a `### Update — YYYY-MM-DD HH:MM (end-to-end verification)`
   entry holding the pasted output of the phase's End-to-end verification
   commands. Your Summary does not replace this entry.
```

### 3. Step 7 (lines 91-93)

Replace:

```
   the only doc change present at this point is that start flip and your started
   entry). Use a conventional-commit message. Do **not** flip the status to
```

with:

```
   the only doc changes present at this point are that start flip and your
   Update Log entries). Use a conventional-commit message. Do **not** flip the status to
```

### 4. Completion checklist (after the pinned-count line)

Replace:

```
[ ] Every pinned count in your Summary is a pasted result line, not a restatement.
```

with:

```
[ ] Every pinned count in your Summary is a pasted result line, not a restatement.
[ ] The Update Log has your started entry and an `(end-to-end verification)` entry with pasted output.
```

**Must NOT:**

- Add any `{…}` text other than the existing `{TEST_COMMAND}` in replacement 2.
  `placeholder_set_is_exactly_the_four_authorized` fails on any other
  curly-brace word.
- Edit any other part of the contract, `WORKFLOW.md`, or `STANDARDS.md`.
- Change existing tests.

## Acceptance criteria

- [ ] The assembled contract does not contain `This is the only Update Log entry you write`.
- [ ] It does not contain `the only doc change present at this point is`.
- [ ] It contains `Your Summary does not replace this entry.`
- [ ] It contains `The Update Log has your started entry and an \`(end-to-end verification)\` entry`.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

Add to `mod tests` in `executor/src/agent/contract.rs`, shaped like the
existing `contract_does_not_ask_executor_to_name_itself`:

```rust
    #[test]
    fn contract_does_not_ask_executor_to_name_itself() {
        let commands = CommandConfig::default();
        let output = assemble_executor_contract(&commands);
        assert!(
            !output.contains("naming yourself"),
            "contract must not instruct the executor to name itself"
        );
        assert!(
            output.contains("Do **not** name yourself or a model"),
            "contract must tell the executor not to name itself or a model"
        );
    }
```

1. `contract_does_not_limit_executor_to_one_log_entry` — asserts
   `!output.contains("This is the only Update Log entry you write")` and
   `!output.contains("the only doc change present at this point is")`.
2. `contract_requires_end_to_end_entry` — asserts
   `output.contains("Your Summary does not replace this entry.")` and
   `output.contains("The Update Log has your started entry and an `(end-to-end verification)` entry")`
   (use a raw string `r#"…"#` for the second, it contains backticks).

**Write test 1 first, run it, and quote its failure in the Update Log before
editing the contract.**

**Finish condition:** `cargo test -p rexymcp-executor contract` → **13 passed**;
full `cargo test` → **727 / 2 / 1212 passed**. Paste the `test result:` lines.

## End-to-end verification

Run `cargo test -p rexymcp-executor contract -- --nocapture` and paste its
output in a `### Update — YYYY-MM-DD HH:MM (end-to-end verification)` entry.

## Authorizations

- Editing `executor/templates/executor_contract.md` as specified.

## Out of scope

- Anything else in the contract.

## Update Log

<!-- entries appended below this line -->

### Update — 2026-09-18 17:38 (started)

Implementing the four contract wording replacements from the spec.
Wrote the two new tests first; `contract_does_not_limit_executor_to_one_log_entry`
and `contract_requires_end_to_end_entry` failed as expected against the unedited
contract:

```
test agent::contract::tests::contract_does_not_limit_executor_to_one_log_entry ... FAILED
test agent::contract::tests::contract_requires_end_to_end_entry ... FAILED

failures:

---- agent::contract::tests::contract_does_not_limit_executor_to_one_log_entry stdout ----

thread 'agent::contract::tests::contract_does_not_limit_executor_to_one_log_entry' (678599) panicked at executor/src/agent/contract.rs:190:9:
contract must not limit the executor to a single Update Log entry
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

---- agent::contract::tests::contract_requires_end_to_end_entry stdout ----

thread 'agent::contract::tests::contract_requires_end_to_end_entry' (678601) panicked at executor/src/agent/contract.rs:204:9:
contract must state the Summary does not replace the end-to-end entry
 (x2)
failures:
    agent::contract::tests::contract_does_not_limit_executor_to_one_log_entry
    agent::contract::tests::contract_requires_end_to_end_entry

test result: FAILED. 11 passed; 2 failed; 0 ignored; 0 measured; 1209 filtered out; finished in 0.00s
```

Then applied the four spec replacements to `executor/templates/executor_contract.md`;
both new tests now pass (13 total in the `contract` filter).

### Update — 2026-09-18 17:38 (end-to-end verification)

`cargo test -p rexymcp-executor contract -- --nocapture`:

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.09s
     Running unittests src/lib.rs (target/debug/deps/executor-090557a76b736c2e)

running 13 tests
test agent::contract::tests::contract_requires_end_to_end_entry ... ok
test agent::contract::tests::contract_contains_resuming_a_phase ... ok
test agent::contract::tests::contract_omits_lint_fix ... ok
test agent::contract::tests::contract_does_not_ask_executor_to_name_itself ... ok
test agent::contract::tests::contract_does_not_limit_executor_to_one_log_entry ... ok
test agent::contract::tests::contract_requires_pasting_pinned_counts ... ok
test agent::contract::tests::output_starts_with_contract_preamble ... ok
test agent::contract::tests::placeholder_set_is_exactly_the_four_authorized ... ok
test agent::prompt::tests::system_prompt_order_is_contract_then_standards_then_phase_doc ... ok
test agent::prompt::tests::system_prompt_includes_substituted_contract ... ok
test agent::prompt::tests::assembles_system_prompt_in_contract_standards_phase_order ... ok
test agent::contract::tests::unset_command_renders_as_unconfigured_sentinel ... ok
test agent::contract::tests::substitutes_all_four_commands_when_set ... ok

test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 1209 filtered out; finished in 0.00s
```

### Update — 2026-09-18 17:45 (complete, server-authored)

**Summary:** All done. Working tree clean, everything committed.

**Summary**

Phase 1 of F11 — log entry wording. Made the four exact replacements in `executor/templates/executor_contract.md` so the contract no longer claims the started entry is the executor's "only Update Log entry": step 2 now says the executor also writes progress/blocker entries and one end-to-end entry (never a `(complete)` entry); step 5 now requires a `### Update … (end-to-end verification)` entry with pasted output and states the Summary doesn't replace it; step 7 now references "doc changes … your Update Log entries"; and the completion checklist gained a line requiring the started entry plus the end-to-end verification entry. Added two tests in `executor/src/agent/contract.rs` (`contract_does_not_limit_executor_to_one_log_entry`, `contract_requires_end_to_end_entry`) — written first, and their failures against the unedited contract are quoted in the started Update Log entry.

**Verification:** `cargo fmt --all --check`, `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings` all clean. `cargo test -p rexymcp-executor contract -- --nocapture` → `test result: ok. 13 passed; 0 failed` (pinned 13). Full `cargo test` → `test result: ok. 727 passed`, `2 passed`, `1212 passed` (pinned 727/2/1212). E2E output captured to a file and pasted into an `(end-to-end verification)` Update Log entry.

**Notes for review:** No deviations from the spec; both new tests are the only test changes, no existing tests touched, and no new `{…}` placeholders added (the `placeholder_set_is_exactly_the_four_authorized` test still passes).

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s


TEST
onexistent_path ... ok
test tools::symbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::symbols::tests::references_exclude_strings_and_comments ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1212 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.20s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F11-contract-update-log-wording/README.md` — +1 -1
- `docs/dev/milestones/F11-contract-update-log-wording/phase-01-log-entry-wording.md` — +61 -1
- `executor/src/agent/contract.rs` — +30 -0
- `executor/templates/executor_contract.md` — +8 -3

**Commit:** 90b57f8e5e17cfde06d7440c153c5b7cb3e8e29e

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).
