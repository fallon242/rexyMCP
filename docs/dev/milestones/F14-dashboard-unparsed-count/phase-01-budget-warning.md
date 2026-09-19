# Phase 1: budget warning

**Milestone:** F14 — Dashboard unparsed count
**Status:** review
**Status:** in-progress
**Estimated diff:** ~60 lines, half tests
**Tags:** language=rust, kind=feature, size=xs

## Goal

`load_data` already reads the store with `read_all`, whose `unparsed` count
(F12) says how many telemetry lines no longer parse. Carry it into
`DashboardData` and show `Unreadable telemetry: N` in the Budget panel, after
the "Top skill" line, only when N > 0.

**Verified by the architect 2026-09-19** in a scratch worktree at `65917e6`:
format and clippy clean, suite **732 / 2 / 1220**, `dashboard::` **183**. Each
new test fails when its part of the fix is removed (count dropped in
`load_data`; line never shown; line shown at zero).

## Pre-flight

1. `cargo test -p rexymcp dashboard::` → **180 passed**.
2. Full `cargo test` → **729 / 2 / 1220 passed**.

## Spec

Apply these replace blocks exactly; each "Replace" text occurs once.
Blocks 3–6 add `unparsed_records` to the four `DashboardData` literals in
`load_data` — there are exactly **4**.

### `mcp/src/dashboard/mod.rs`

**1.** Replace:

```rust
    pub arch_cache_5m: u64,
    pub arch_cache_1h: u64,
}

```

with:

```rust
    pub arch_cache_5m: u64,
    pub arch_cache_1h: u64,
    /// `StoreRecords::unparsed` — telemetry lines that no longer parse. Shown in
    /// the Budget panel when non-zero, the same count `rexymcp costs` prints.
    pub unparsed_records: usize,
}

```

**2.** Replace:

```rust
        .map(|dir| telemetry::read_all(&dir.join("phase_runs.jsonl")).unwrap_or_default())
        .unwrap_or_default();
    let phase_runs: Vec<PhaseRun> = store.runs;

```

with:

```rust
        .map(|dir| telemetry::read_all(&dir.join("phase_runs.jsonl")).unwrap_or_default())
        .unwrap_or_default();
    let unparsed_records = store.unparsed;
    let phase_runs: Vec<PhaseRun> = store.runs;

```

**3.** Replace:

```rust
                        arch_cache_5m,
                        arch_cache_1h,
                    }
                }
```

with:

```rust
                        arch_cache_5m,
                        arch_cache_1h,
                        unparsed_records,
                    }
                }
```

**4.** Replace:

```rust
                    arch_cache_5m,
                    arch_cache_1h,
                },
            }
```

with:

```rust
                    arch_cache_5m,
                    arch_cache_1h,
                    unparsed_records,
                },
            }
```

**5.** Replace:

```rust
                        arch_cache_5m: 0,
                        arch_cache_1h: 0,
                    }
                }
```

with:

```rust
                        arch_cache_5m: 0,
                        arch_cache_1h: 0,
                        unparsed_records,
                    }
                }
```

**6.** Replace:

```rust
                    arch_cache_5m: 0,
                    arch_cache_1h: 0,
                },
            }
```

with:

```rust
                    arch_cache_5m: 0,
                    arch_cache_1h: 0,
                    unparsed_records,
                },
            }
```

**7.** Replace:

```rust
    }

    #[test]
    fn load_data_reads_project_architect_tokens_from_ledger() {
```

with:

```rust
    }

    #[test]
    fn load_data_counts_unparsed_telemetry_lines() {
        let dir = TempDir::new().unwrap();
        let sessions = sessions_dir(dir.path());
        std::fs::create_dir_all(&sessions).unwrap();
        let telemetry_dir = dir.path().join("telemetry");
        std::fs::create_dir_all(&telemetry_dir).unwrap();
        // A current-schema ledger line missing `session_id`, plus a non-JSON line.
        std::fs::write(
            telemetry_dir.join("phase_runs.jsonl"),
            "{\"record\":\"architect_ledger\",\"schema_version\":1,\"model\":\"m\"}\nnot json\n",
        )
        .unwrap();

        let data = load_data(
            dir.path(),
            None,
            Some(&telemetry_dir),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
            &default_architect_cfg(),
        );
        assert_eq!(data.unparsed_records, 2, "both unusable lines are counted");
    }

    #[test]
    fn load_data_reads_project_architect_tokens_from_ledger() {
```

### `mcp/src/dashboard/render.rs`

**8.** Replace:

```rust
}

/// Render the dashboard into a three-panel header band (Session · Budget ·
/// Compactions) above a body (Activity wide-left · Files right), or a
```

with:

```rust
}

/// Budget-panel warning when the telemetry store holds lines that no longer
/// parse — the same count `rexymcp costs` prints. Hidden at zero.
fn unparsed_line(n: usize) -> Option<Line<'static>> {
    (n > 0).then(|| {
        Line::from(Span::styled(
            format!("  Unreadable telemetry: {n}"),
            Style::new().fg(Color::Yellow),
        ))
    })
}

/// Render the dashboard into a three-panel header band (Session · Budget ·
/// Compactions) above a body (Activity wide-left · Files right), or a
```

**9.** Replace:

```rust
        budget.push(line);
    }

    let context = reclaim_lines(&data.summary);
```

with:

```rust
        budget.push(line);
    }
    if let Some(line) = unparsed_line(data.unparsed_records) {
        budget.push(line);
    }

    let context = reclaim_lines(&data.summary);
```

**10.** Replace:

```rust
    }

    // --- visible_offset tests ---

```

with:

```rust
    }

    #[test]
    fn unparsed_line_shows_count() {
        let line = unparsed_line(3).expect("non-zero count renders a line");
        let text = format!("{line}");
        assert!(text.contains("Unreadable telemetry: 3"), "got: {text}");
    }

    #[test]
    fn unparsed_line_hidden_when_zero() {
        assert!(unparsed_line(0).is_none(), "zero must render nothing");
    }

    // --- visible_offset tests ---

```

**Must NOT:**

- Re-read the telemetry file; use the `store` `load_data` already has.
- Show the line at 0, or anywhere but the Budget panel.
- Run `cargo fmt --all`. If needed, `rustfmt --edition 2024` on the two files.

## Acceptance criteria

- [ ] `load_data` on a store with one field-less ledger line and one non-JSON
      line returns `unparsed_records == 2`.
- [ ] `unparsed_line(3)` renders `Unreadable telemetry: 3`; `unparsed_line(0)`
      is `None`.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

The three tests are in blocks 7 and 10 (block 10 holds two). **Finish condition:**

- `cargo test -p rexymcp dashboard::` → **183 passed**
- Full `cargo test` → **732 / 2 / 1220 passed**

Paste the `test result:` lines.

## End-to-end verification

In a `### Update — YYYY-MM-DD HH:MM (end-to-end verification)` entry, paste the
output of `cargo test -p rexymcp unparsed -- --nocapture`.

## Authorizations

- `mcp/src/dashboard/mod.rs` and `mcp/src/dashboard/render.rs` as specified.

## Out of scope

- The scorecard, `runs`, and the per-type telemetry readers.

## Update Log

<!-- entries appended below this line -->

### Update — 2026-09-19 04:32 (started)

Implementing phase 1: carry `StoreRecords::unparsed` from `load_data` into
`DashboardData` as `unparsed_records`, and render `Unreadable telemetry: N` in
the Budget panel after the "Top skill" line (only when N > 0). Adding the three
tests per the spec's replace blocks.

### Update — 2026-09-19 04:35 (end-to-end verification)

`cargo test -p rexymcp unparsed -- --nocapture`:

```
running 5 tests
test dashboard::render::tests::unparsed_line_hidden_when_zero ... ok
test dashboard::render::tests::unparsed_line_shows_count ... ok
test costs::tests::format_costs_omits_unparsed_line_when_zero ... ok
test costs::tests::format_costs_reports_unparsed_records ... ok
test dashboard::tests::load_data_counts_unparsed_telemetry_lines ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 727 filtered out; finished in 0.00s
```

Grep proving the new Budget-panel literal landed in `render.rs`:

```
$ grep -n "Unreadable telemetry: " mcp/src/dashboard/render.rs
219:            format!("  Unreadable telemetry: {n}"),
459:        assert!(text.contains("Unreadable telemetry: 3"), "got: {text}");
```

(The `costs.rs` hits use the distinct string `Unreadable telemetry records:`.)

Gate results (separate invocations):

```
cargo fmt --all --check          → clean (exit 0)
cargo build                      → Finished `dev` profile, 0 warnings
cargo clippy --all-targets --all-features -- -D warnings → clean (exit 0)
cargo test -p rexymcp dashboard:: → test result: ok. 183 passed; 0 failed
cargo test (full)                → 732 / 2 / 1220 passed
```
`DashboardData` as `unparsed_records`, and render `Unreadable telemetry: N` in
the Budget panel after the "Top skill" line (only when N > 0). Adding the three
tests per the spec's replace blocks.

### Update — 2026-09-19 04:40 (complete, server-authored)

**Summary:** All spec blocks applied and verified. Working tree is clean, committed in one conventional commit.

**Summary + Notes for review**

Implemented F14 phase 1: `load_data` now captures `store.unparsed` into a new `DashboardData::unparsed_records` field (set in all four literals, `mod.rs`), and the Budget panel renders a new `unparsed_line()` helper as `  Unreadable telemetry: N` (yellow) immediately after the "Top skill" line, only when N > 0 (`render.rs`). All three new tests added per the spec's replace blocks.

Verification:
- `cargo fmt --all --check` clean; `cargo build` zero warnings; `cargo clippy --all-targets --all-features -- -D warnings` clean.
- `cargo test -p rexymcp dashboard::` → `test result: ok. 183 passed; 0 failed` (matches the pinned 183).
- Full `cargo test` → `test result: ok. 732 passed`, `ok. 2 passed`, `ok. 1220 passed` (matches 732 / 2 / 1220).
- End-to-end `cargo test -p rexymcp unparsed -- --nocapture` → `test result: ok. 5 passed; 0 failed` (pasted in the Update Log).

Notes: the literal grep confirms the new `Unreadable telemetry: {n}` lives at `render.rs:219`; the pre-existing `costs.rs` uses the distinct `Unreadable telemetry records:` string, so no collision. Status left at `in-progress`; no `(complete)` entry written — completion bookkeeping left to the server.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.50s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s


TEST
ymbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::references_python_identifier ... ok
test tools::symbols::tests::references_exclude_substring ... ok
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
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::symbols::tests::references_single_file_path ... ok
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
test tools::bash::tests::times_out_advisory_failure ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1220 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.19s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F14-dashboard-unparsed-count/README.md` — +1 -1
- `docs/dev/milestones/F14-dashboard-unparsed-count/phase-01-budget-warning.md` — +45 -0
- `mcp/src/dashboard/mod.rs` — +32 -0
- `mcp/src/dashboard/render.rs` — +26 -0

**Commit:** 3ed61b9499a7e49784a8fc5a7b1c350828c950bb

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).
