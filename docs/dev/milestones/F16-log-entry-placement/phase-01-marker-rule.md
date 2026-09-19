# Phase 1: marker rule

**Milestone:** F16 — Log entry placement
**Status:** done
**Depends on:** none
**Estimated diff:** ~15 lines
**Tags:** language=rust, kind=docs, size=xs

## Goal

Add one sentence to the executor contract saying new Update Log entries go at
the end of the file, below the `<!-- entries appended below this line -->`
marker. Pin it with a test.

**Verified by the architect 2026-09-19** in a scratch worktree at `212ab13`:
format and clippy clean, suite **734 / 2 / 1221**, `contract` **14**. With the
pre-phase contract restored, the new test fails.

## Pre-flight

1. `cargo test -p rexymcp-executor contract` → **13 passed**.
2. Full `cargo test` → **734 / 2 / 1220 passed**.

## Spec

Apply these replace blocks exactly; each "Replace" text occurs once.
Keep the new contract sentence on a single line — the test's `contains` does
not match across a line break.

### `executor/src/agent/contract.rs`

**1.** Replace:

```rust
    }

    #[test]
    fn contract_requires_pasting_pinned_counts() {
```

with:

```rust
    }

    #[test]
    fn contract_places_entries_below_the_marker() {
        let commands = CommandConfig::default();
        let output = assemble_executor_contract(&commands);
        assert!(
            output.contains(
                "below the `<!-- entries appended below this line -->` marker — never above it"
            ),
            "contract must say where new Update Log entries go"
        );
    }

    #[test]
    fn contract_requires_pasting_pinned_counts() {
```

### `executor/templates/executor_contract.md`

**2.** Replace:

```markdown

The Update Log is **append-only**. Never edit prior entries.

### Resuming a phase
```

with:

```markdown

The Update Log is **append-only**. Never edit prior entries.
Add every new entry at the end of the file, below the `<!-- entries appended below this line -->` marker — never above it.

### Resuming a phase
```

**Must NOT:**

- Add any `{…}` text to the contract
  (`placeholder_set_is_exactly_the_four_authorized` fails on it).
- Change any other contract wording.

## Acceptance criteria

- [ ] The assembled contract contains
      `below the \`<!-- entries appended below this line -->\` marker — never above it`.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

**Finish condition:** `cargo test -p rexymcp-executor contract` → **14 passed**;
full `cargo test` → **734 / 2 / 1221 passed**. Paste the `test result:` lines.

**Put your own Update Log entries at the very end of this file.** The marker
text also appears inside this doc's Spec code blocks; do not anchor an edit on
it — append after the last line.

## End-to-end verification

Paste `cargo test -p rexymcp-executor contract_places -- --nocapture` in a
`### Update — YYYY-MM-DD HH:MM (end-to-end verification)` entry.

## Authorizations

- `executor/templates/executor_contract.md` and `executor/src/agent/contract.rs` as specified.

## Out of scope

- Any other contract change.

## Update Log

<!-- entries appended below this line -->

### Update — 2026-09-19 05:19

Started F16 phase-01 (marker rule): flipping status to in-progress, then adding the
"entries go below the `<!-- entries appended below this line -->` marker — never above
it" sentence to the executor contract template and pinning it with
`contract_places_entries_below_the_marker`.

### Update — 2026-09-19 05:19 (end-to-end verification)

Pinned literal landed in the template (grep):

```
executor/templates/executor_contract.md:117:Add every new entry at the end of the file, below the `<!-- entries appended below this line -->` marker — never above it.
```

End-to-end verification — `cargo test -p rexymcp-executor contract_places -- --nocapture`:

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.19s
     Running unittests src/lib.rs (target/debug/deps/executor-090557a76b736c2e)

running 1 test
test agent::contract::tests::contract_places_entries_below_the_marker ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1230 filtered out; finished in 0.00s
```

### Update — 2026-09-19 05:25 (complete, server-authored)

**Summary:** **Summary**

Built F16 phase-01 (marker rule) exactly as specified: added the single contract sentence — `Add every new entry at the end of the file, below the <!-- entries appended below this line --> marker — never above it.` — to `executor/templates/executor_contract.md` right after the append-only line, and pinned it with the new `contract_places_entries_below_the_marker` test in `executor/src/agent/contract.rs` (the template line was the only insertion point; no `{…}` placeholders added, no other wording changed).

**Verification:**
- Grep of the pinned literal: `executor/templates/executor_contract.md:117:Add every new entry at the end of the file, below the \`<!-- entries appended below this line -->\` marker — never above it.`
- `cargo test -p rexymcp-executor contract` → `test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 1217 filtered out` (matches the pinned 14)
- Full `cargo test` → `734 passed; 0 failed; 0 ignored` (bin), `2 passed` (doc-tests), `1221 passed; 0 failed; 10 ignored` (integration) = 734 / 2 / 1221, matching the pinned finish condition. (I pasted the per-target `test result:` lines via a grep filter, so the numbers are the suite's own, not reconstructed.)
- `cargo fmt --all --check`, `cargo build`, and `cargo clippy --all-targets --all-features -- -D warnings` all pass clean.
- End-to-end: `cargo test -p rexymcp-executor contract_places -- --nocapture` → `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1230 filtered out` (pasted in the end-to-end Update Log entry).

**Notes for review:** No deviations from the spec. Status is left at `in-progress` and no `(complete)` entry was written — the server authors the completion tail. Working tree is clean; commit `93f3ea1` includes all four touched files.

One observation worth flagging (not fixed, out of scope): the spec's own phase doc is the first live instance of the new rule, and the phase doc itself notes the marker text also appears inside its Spec code blocks — I appended the new entries after the last line of the file (below the marker), per the rule.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.22s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s


TEST
s_path_outside_root ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::references_python_identifier ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1221 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.19s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F16-log-entry-placement/README.md` — +1 -1
- `docs/dev/milestones/F16-log-entry-placement/phase-01-marker-rule.md` — +28 -1
- `executor/src/agent/contract.rs` — +12 -0
- `executor/templates/executor_contract.md` — +1 -0

**Commit:** 93f3ea18a5d6b8dc719396f25b81e635f501b143

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

### Review verdict — 2026-09-19

- **Verdict:** approved_first_try
- **Bounces:** none
- **Executor:** RedHatAI/Qwen3.8-27B-INT4 (local), 32 turns
- **Scope deviations:** none — both files equal the spec applied to `9402cc1`, byte for byte (`93f3ea1`).
- **Verification:** gates 734 / 2 / 1221; `contract` 14. Mutation: the pre-phase contract fails `contract_places_entries_below_the_marker`. This run's own entries all sit below the real marker (line 120) with four copies of the marker text earlier in the Spec left untouched. First live log carrying F15's `phase_doc` event (session `6aae1b61`).
- **Calibration:** the started entry's heading has no `(started)` label — nit.
