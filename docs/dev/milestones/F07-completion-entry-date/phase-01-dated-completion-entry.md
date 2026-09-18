# Phase 1: dated completion entry

**Milestone:** F07 — Completion-entry date
**Status:** done
**Depends on:** none
**Estimated diff:** ~40 lines, most of it tests
**Tags:** language=rust, kind=bugfix, size=xs

## Goal

The server writes the completion entry at the end of every phase. It heads the
entry with a raw epoch, `### Update — ts=1784924570254 (complete,
server-authored)`, while every other Update Log entry uses
`### Update — YYYY-MM-DD HH:MM (…)`. After this phase the server writes
`### Update — 2026-07-24 20:22 (complete, server-authored)`, in UTC, the same
clock the executor's own entries use.

## Pre-flight

1. `cargo test -p rexymcp finalize` → **38 passed** (measured 2026-09-18).
2. `cargo test -p rexymcp-executor format_utc` → **8 passed**.

## Current state

`mcp/src/finalize.rs:100-122`, the header line only:

```rust
fn baseline_entry(result: &PhaseResult, now_ms: u64, code_sha: &str, model: &str) -> String {
    // ...
    format!(
        "### Update — ts={now_ms} (complete, server-authored)\n\n\
         **Summary:** {summary}\n\n\
```

The date helpers already exist, private, in
`executor/src/agent/prompt.rs:31` and `:49`:

```rust
fn format_utc_date(now_ms: u64) -> String { /* -> "YYYY-MM-DD" */ }
fn format_utc_time(now_ms: u64) -> String { /* -> "HH:MM" */ }
```

`mcp` depends on `rexymcp-executor`, and `agent::prompt` is already a `pub mod`.

## Spec

1. In `executor/src/agent/prompt.rs`, change `fn format_utc_date` and
   `fn format_utc_time` to `pub fn`. Change nothing else in that file.
2. In `mcp/src/finalize.rs`, import them:
   ```rust
   use rexymcp_executor::agent::prompt::{format_utc_date, format_utc_time};
   ```
3. In `baseline_entry`, build the header from them:
   ```rust
   let when = format!("{} {}", format_utc_date(now_ms), format_utc_time(now_ms));
   ```
   and change the first line of the `format!` to
   `"### Update — {when} (complete, server-authored)\n\n\`.
   The rest of the entry is unchanged.

**Must NOT:**

- Add a dependency (no `chrono`, `time`, `jiff`) or edit any `Cargo.toml`.
- Copy the civil-from-days arithmetic into `mcp`. Reuse the helpers.
- Use real wall-clock time. `now_ms` is the injected clock; keep using it.
- Rewrite `ts=` headers in existing phase docs under `docs/`. They are history.
- Append ` UTC` or seconds to the header; the format is exactly `YYYY-MM-DD HH:MM`.

## Acceptance criteria

- [ ] `baseline_entry(&result, 1_784_924_570_254, …)` starts with
      `### Update — 2026-07-24 20:22 (complete, server-authored)\n`.
- [ ] No `ts=` appears anywhere in `baseline_entry`'s output.
- [ ] `format_utc_date` / `format_utc_time` are `pub` and otherwise unchanged.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

In `mcp/src/finalize.rs` `mod tests`:

1. **New:** `baseline_entry_heads_with_utc_date_time`. Build the `PhaseResult`
   exactly as `baseline_entry_includes_executor_line_from_model` does, call
   `baseline_entry(&result, 1_784_924_570_254, "abc123", "m")`, assert it
   `starts_with("### Update — 2026-07-24 20:22 (complete, server-authored)\n")`
   and `!entry.contains("ts=")`. Write it first, run it, quote the failure in
   the Update Log, then fix.
2. **Edit:** `finalize_flips_status_and_appends_entry` asserts
   `after.contains("ts=999999")`. `now_ms` 999999 is 1970-01-01 00:16:39 UTC;
   replace it with
   `after.contains("### Update — 1970-01-01 00:16 (complete, server-authored)")`.
3. **Edit:** `golden_roundtrip_flip_then_append_produces_wellformed_doc` uses a
   hand-written `ts=1784924570254` entry literal (it does not call
   `baseline_entry`). Replace `ts=1784924570254` with `2026-07-24 20:22` in the
   `entry` literal and in `expected`, and change its separator check to
   `"by AI executor.\n\n### Update — 2026-07-24 20:22"`.

**Finish condition:** `cargo test -p rexymcp finalize` → **39 passed**;
`cargo test -p rexymcp-executor format_utc` → **8 passed**.

## End-to-end verification

Run `cargo test -p rexymcp baseline_entry_heads_with_utc_date_time -- --nocapture`
and paste the output in a `(end-to-end verification)` entry.

## Authorizations

- Visibility change `fn` → `pub fn` on the two helpers in `prompt.rs`.

## Out of scope

- The `**Notes:**` line of the entry, which cites "M27 phase-03". Leave it.
- The waived `eprintln!` in `executor/src/privacy/egress.rs`.

## Update Log

<!-- entries appended below this line -->

### Update — 2026-09-18 14:41 (started)

Starting implementation. Making `format_utc_date`/`format_utc_time` pub in
`executor/src/agent/prompt.rs` and switching `baseline_entry` in
`mcp/src/finalize.rs` to a dated header, with test updates per the spec's test
plan (red test first).

### Update — 2026-09-18 14:45 (progress)

Wrote the red test `baseline_entry_heads_with_utc_date_time` first and captured
its failure:

```
thread 'finalize::tests::baseline_entry_heads_with_utc_date_time' (263690) panicked at mcp/src/finalize.rs:848:9:
entry header must be the UTC date and time: ### Update — ts=1784924570254 (complete, server-authored)

**Summary:** Phase complete.

**Executor:** m

**Gates:** format=skipped, build=skipped, lint=skipped, test=skipped

**Command output tails:**

(no command output captured)

Then applied the fix: helpers made `pub` in `prompt.rs`, `baseline_entry`
now builds the header via `format_utc_date`/`format_utc_time`, and the two
existing tests with `ts=` literals were updated. `cargo test -p rexymcp
finalize` → 39 passed; `cargo test -p rexymcp-executor format_utc` → 8 passed.

### Update — 2026-09-18 14:50 (end-to-end verification)

```
$ cargo test -p rexymcp baseline_entry_heads_with_utc_date_time -- --nocapture

   Compiling rexymcp v0.9.1 (/home/gpratt/rexyMCP/mcp)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 6.59s
     Running unittests src/main.rs (target/debug/deps/rexymcp-cd922bc1926100ad)

running 1 test
test finalize::tests::baseline_entry_heads_with_utc_date_time ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 716 filtered out; finished in 0.00s

     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-13fb83281f3fe7ad)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s
```

Positive control: the same command run *before* the fix (recorded in the
progress entry above) panicked with the old header
`### Update — ts=1784924570254 (complete, server-authored)`, so the pass above
demonstrably exercises the new code path.

### Update — ts=1789743238937 (complete, server-authored)

**Summary:** Done.

**Summary:** Server-authored completion entries in `mcp/src/finalize.rs` now head themselves `### Update — YYYY-MM-DD HH:MM (complete, server-authored)` (UTC, from the injected `now_ms`) instead of `ts=<epoch-ms>`, reusing the existing `format_utc_date`/`format_utc_time` helpers in `executor/src/agent/prompt.rs`, which were made `pub` per the spec.

**Deviation / adaptation:** none from the spec. One mechanical note — the spec's test-plan sketch placed the new `pub fn` import after the existing `rexymcp_executor::agent::command` import and before `rexymcp_executor::phase`, which `cargo fmt` would have reordered; I kept the spec's group placement but the final tree has `agent::prompt` after `agent::command` (alphabetical within the external-crate group), which is fmt-stable and semantically identical.

**Verification:** Wrote `baseline_entry_heads_with_utc_date_time` red first (captured the `ts=1784924570254` panic in the Update Log), then applied the fix. `cargo test -p rexymcp finalize` → **39 passed**; `cargo test -p rexymcp-executor format_utc` → **8 passed**. Full `cargo test` → 717 + 2 + 1206 + 0, zero failures across all crates. `cargo fmt --all --check`, `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings` all pass.

**E2E:** `cargo test -p rexymcp baseline_entry_heads_with_utc_date_time -- --nocapture` output pasted into an `(end-to-end verification)` entry in the phase doc; positive control is the recorded red run.

**Files changed:** `executor/src/agent/prompt.rs` (2 helpers `pub`), `mcp/src/finalize.rs` (import, header build, 1 new test, 2 test-literal edits), plus the phase doc's status flip + Update Log and the milestone README row flip. `docs/dev/NEXT.md` and the F07 milestone directory came in untracked/dirty before my run (architect's dispatch prep) — "stage everything" swept them into the same commit; flagging in case review wants them split out.

**Status** left `in-progress`; no `(complete)` entry hand-written — the server authors the completion tail. Commit `77d192b`.

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
utside_root ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_exclude_strings_and_comments ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::symbols::tests::references_finds_call_sites ... ok
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
test tools::symbols::tests::references_single_file_path ... ok
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
test tools::symbols::tests::references_respects_max_results ... ok
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

test result: ok. 1206 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.20s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F07-completion-entry-date/README.md` — +1 -1
- `docs/dev/milestones/F07-completion-entry-date/phase-01-dated-completion-entry.md` — +58 -1
- `executor/src/agent/prompt.rs` — +2 -2
- `mcp/src/finalize.rs` — +28 -5

**Commit:** 77d192b04dc7e28f83d96d7cadd5f93583daebd5

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

### Review verdict — 2026-09-18

- **Verdict:** approved_first_try
- **Bounces:** none
- **Executor:** RedHatAI/Qwen3.8-27B-INT4 (local), 54 turns
- **Scope deviations:** none. The executor's commit `77d192b` also swept in the
  architect's uncommitted F07 docs and `NEXT.md`; all belong to this milestone,
  so left as is.
- **Calibration:** none. Note: the `(complete, server-authored)` entry above
  still reads `ts=1789743238937` (= 2026-09-18 14:53 UTC) because the running
  `serve` binary predates this fix; new headers appear once `serve` is rebuilt
  and restarted.
