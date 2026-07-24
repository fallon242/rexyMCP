# Phase 01: Capture `created_cache_tokens` + disjoint `input_tokens`

**Milestone:** M39 — Executor Cache Accounting
**Status:** review
**Depends on:** none (first phase of M39)
**Estimated diff:** ~40 lines (a ~10-line parser change + tests)
**Tags:** language=rust, kind=bugfix, size=s

## Goal

The vLLM backend now surfaces prefix-cache tokens in every chat response's
`usage.prompt_tokens_details` (after the human enabled
`--enable-prompt-tokens-details`). The cache **read** (`cached_tokens`) is already
parsed; the cache **write** (`created_cache_tokens`, a vLLM extension) is dropped
because the parser hardcodes `cache_write_tokens: 0`. Capture it, and correct the
`input_tokens` arithmetic so the three input classes stay **disjoint** and sum to
`prompt_tokens`.

This is the whole code change for M39. Everything downstream (the `PhaseRun`
telemetry fields, `scope_costs`, `scope_report` pricing, the M38 discount ledger)
already consumes `cache_write_tokens` — it has just been receiving `0`.

## Architecture references

Read before starting:

- `docs/dev/milestones/M39-executor-cache-accounting/README.md` — the milestone,
  including the live-probe findings this phase implements and the modeling caveat.
- `docs/architecture.md` § Status #39 — design summary.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read this entire phase doc before touching any code.
3. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**The single choke point** is `parse_openai_usage`
(`executor/src/ai/backends/openai.rs:11-28`). Both the non-streaming and the
streaming (`openai.rs:314`) paths route their `usage` object through it, so this
one function is the only site to change. Here it is verbatim:

```rust
pub(crate) fn parse_openai_usage(u: &serde_json::Map<String, Value>) -> TokenBreakdown {
    let total_prompt = u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let output_tokens = u
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let cache_read = u
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    TokenBreakdown {
        input_tokens: total_prompt.saturating_sub(cache_read),
        output_tokens,
        cache_read_tokens: cache_read,
        cache_write_tokens: 0,
    }
}
```

**`TokenBreakdown`** (`executor/src/ai/types.rs:44-53`) already carries the field:

```rust
pub struct TokenBreakdown {
    #[serde(default)] pub input_tokens: u32,
    #[serde(default)] pub output_tokens: u32,
    #[serde(default)] pub cache_read_tokens: u32,
    #[serde(default)] pub cache_write_tokens: u32,
}
```

**What the live vLLM actually returns** (captured 2026-07-24 against `brain:8000`,
Qwen3.6-27B-FP8, with `--enable-prefix-caching --enable-prompt-tokens-details`).
A cold call (prompt newly cached) and a warm call (same prefix, cache hit):

```json
// cold — 1728 of the 3017 prompt tokens were written to cache
"usage": {"prompt_tokens": 3017, "completion_tokens": 2,
          "prompt_tokens_details": {"cached_tokens": 0, "created_cache_tokens": 1728, "multimodal_tokens": null}}

// warm — 1728 of the 3017 prompt tokens were read from cache
"usage": {"prompt_tokens": 3017, "completion_tokens": 2,
          "prompt_tokens_details": {"cached_tokens": 1728, "created_cache_tokens": 0, "multimodal_tokens": null}}
```

Note: `cached_tokens` (read) and `created_cache_tokens` (write) were **mutually
exclusive** in every observed call — a token is either freshly created in the
cache or read from it, not both in one request. `multimodal_tokens` is irrelevant
and must be ignored.

## Spec

### 1. Parse `created_cache_tokens` into `cache_write_tokens`

In `parse_openai_usage`, read `prompt_tokens_details.created_cache_tokens` with
the **same optional-chaining shape** the existing `cached_tokens` read uses
(`.get(...).and_then(...).and_then(|v| v.as_u64()).unwrap_or(0) as u32`), and put
it in `cache_write_tokens`. When the field is absent (LM Studio, Ollama, older
vLLM, or a details block that omits it), it must default to `0` — the `unwrap_or(0)`
gives that for free. **Do not** rename or restructure the existing `cached_tokens`
read.

### 2. Make the three input classes disjoint

`prompt_tokens` is the *whole* prompt and already includes both the cached-read
and the newly-created-cache tokens. So `input_tokens` (the fresh, uncached,
non-cache-creating remainder) must subtract **both**:

```
input_tokens = prompt_tokens - cache_read - cache_write
```

Worked against the fixtures above:

- Warm: `3017 - 1728 - 0 = 1289` fresh input; `cache_read = 1728`; `cache_write = 0`.
  Sum `1289 + 1728 + 0 = 3017 = prompt_tokens`. ✓
- Cold: `3017 - 0 - 1728 = 1289` fresh input; `cache_read = 0`; `cache_write = 1728`.
  Sum `1289 + 0 + 1728 = 3017 = prompt_tokens`. ✓

Use a **saturating** subtraction that cannot underflow if a malformed backend
reports `cache_read + cache_write > prompt_tokens`. `u32::saturating_sub` is not
enough on its own for two subtractions — chain them
(`total_prompt.saturating_sub(cache_read).saturating_sub(cache_write)`) so the
result clamps to `0` rather than wrapping. It must **never panic**.

### 3. Nothing else changes

`output_tokens`, `cache_read_tokens`, and the function signature stay as they are.
No downstream file needs editing — `TokenBreakdown` already has the field and the
telemetry/pricing path already reads it. If you find yourself editing
`types.rs`, `metrics.rs`, `telemetry.rs`, `costs.rs`, or config, stop: that is out
of scope and a sign of a wrong turn.

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes (existing parser tests included — they must stay green
      unchanged, since neither carries `created_cache_tokens`).
- [ ] `parse_openai_usage` sets `cache_write_tokens` from
      `prompt_tokens_details.created_cache_tokens`, defaulting to `0` when absent.
- [ ] `input_tokens == prompt_tokens - cache_read - cache_write`, clamped to `0`
      (never panics) when the two exceed `prompt_tokens`.
- [ ] For any parsed usage, `input_tokens + cache_read_tokens + cache_write_tokens
      == prompt_tokens` whenever `cache_read + cache_write <= prompt_tokens`.

## Test plan

Add unit tests in the existing `#[cfg(test)] mod tests` block in
`executor/src/ai/backends/openai.rs`, matching the style of the two present tests
(`openai_parses_cached_tokens_from_details`,
`openai_parses_zero_cache_when_details_absent`) — build the usage object with
`serde_json::json!({...}).as_object().cloned().unwrap()` and call
`parse_openai_usage`. Pin **behavior** (the field values and the sum identity),
not exact wording:

- `openai_parses_created_cache_tokens_as_cache_write` — the **cold** fixture
  (`prompt_tokens: 3017`, `cached_tokens: 0`, `created_cache_tokens: 1728`):
  assert `cache_write_tokens == 1728`, `cache_read_tokens == 0`,
  `input_tokens == 1289`, and `total() == 3017`.
- `openai_warm_call_reads_cache_not_writes` — the **warm** fixture
  (`cached_tokens: 1728`, `created_cache_tokens: 0`): assert `cache_read_tokens
  == 1728`, `cache_write_tokens == 0`, `input_tokens == 1289`.
- `openai_input_plus_cache_classes_equal_prompt_tokens` — for at least the cold
  and warm fixtures, assert `input_tokens + cache_read_tokens + cache_write_tokens
  == prompt_tokens` (the disjointness invariant, stated as one assertion).
- `openai_created_cache_tokens_absent_is_zero` — a details block with
  `cached_tokens` present but **no** `created_cache_tokens` key: assert
  `cache_write_tokens == 0` and the read still works. (Portability: LM Studio /
  Ollama / older vLLM.) This is the **negative** case — it must fail if the new
  read is written to `unwrap`/panic on a missing key instead of defaulting.
- `openai_cache_over_report_clamps_input_to_zero` — a malformed fixture where
  `cached_tokens + created_cache_tokens > prompt_tokens` (e.g. `prompt_tokens:
  100`, `cached_tokens: 80`, `created_cache_tokens: 40`): assert the call does
  **not** panic and `input_tokens == 0`.

**Mutation self-check before you finish:** temporarily change the production
`input_tokens` to subtract only `cache_read` (the old behavior) and confirm the
cold-fixture and disjointness tests **fail**; then restore. A test that passes
against the old arithmetic is not pinning the fix. (Do not commit the mutation.)

## End-to-end verification

**Hermetic boundary — do NOT hit the network.** Tests must not call `brain:8000`
or any endpoint (STANDARDS: no real network). Your end-to-end proof stays inside
the process: the fixtures above are the *exact* JSON vLLM emits, so a unit test
that feeds them through `parse_openai_usage` **is** the end-to-end parse of a real
backend response. If you want to exercise the streaming path too, feed a usage
chunk through the same parser — but a live network call is out of bounds and will
fail the hermeticity gate.

Quote, in your Update Log, the `cargo test` output for the new tests and the
before/after of your mutation self-check (the failing assertion when
`input_tokens` subtracts only `cache_read`).

*(The live-network confirmation — that a real dispatched phase now records
non-zero `cache_read_tokens` in its `PhaseRun` and that `rexymcp costs` prices
them — is the milestone's exit criterion and is run by the **reviewer** at
approval, not by you. It cannot be done hermetically, so it is not your task.)*

## Authorizations

None. No new dependencies. No edits outside
`executor/src/ai/backends/openai.rs`. No edits to `docs/architecture.md`.

## Out of scope

- Any edit to `TokenBreakdown` (`types.rs`), the telemetry schema
  (`telemetry.rs`), pricing (`metrics.rs`), `costs.rs`, or config rates — all
  already consume `cache_write_tokens`; this phase only makes the parser *produce*
  it.
- The `[models] cache_read_per_mtok` / `cache_creation_per_mtok` **values** or the
  discount's use of architect-vs-executor rates — a pricing-model question logged
  as the M39 modeling caveat, not a code task.
- Sourcing cache stats from vLLM `/metrics` — moot; the per-request `usage` now
  carries the data.
- `multimodal_tokens` — ignore it.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-07-24 18:31 (started)

**Executor:** model executor

Implemented `parse_openai_usage` to capture `created_cache_tokens` and made the three input classes disjoint.
### Update — ts=1784918215872 (complete, server-authored)

**Summary:** **Summary + Notes for review:**

I updated `parse_openai_usage` in `executor/src/ai/backends/openai.rs` to capture `prompt_tokens_details.created_cache_tokens` as `cache_write_tokens` and changed `input_tokens` to subtract both `cache_read` and `cache_write` via chained `saturating_sub`, making the three input classes (fresh, cached-read, cache-created) disjoint and summing to `prompt_tokens`. Added 5 unit tests covering the cold fixture, warm fixture, disjointness invariant, absent-field defaulting to 0, and over-report clamping to 0. Mutation self-check confirmed the cold and disjointness tests fail with the old single-subtraction arithmetic, proving the tests pin the fix. All gates pass: `cargo fmt --all --check`, `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` (1050 passed, 2 ignored). Only `openai.rs` was edited for code — no downstream files touched. The `created_cache_tokens` literal appears in 8 places across the file (1 production read + 7 test references), confirmed by grep.

**Executor:** Qwen/Qwen3.6-27B-FP8

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
   Compiling rexymcp-executor v0.9.1 (/home/matt/src/rexyMCP/executor)
   Compiling rexymcp v0.9.1 (/home/matt/src/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.29s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s


TEST
:update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::symbols::tests::references_exclude_strings_and_comments ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::symbols::tests::kind_filter_returns_only_matching_kind ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::symbols::tests::finds_python_function_and_class ... ok
test tools::symbols::tests::no_symbols_returns_advisory_error ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test governor::verifier::tests::verify_rust_returns_checked_empty_on_clean_code ... ok
test governor::verifier::tests::capture_baseline_dedupes_by_project_root ... ok
test governor::verifier::tests::verify_rust_returns_checked_with_errors_on_broken_code ... ok
test governor::verifier::tests::capture_baseline_skips_unsupported_files ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1050 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.09s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.08s
     Running unittests src/main.rs (target/debug/deps/rexymcp-4e85b51f198fbe9f)
     Running unittests src/lib.rs (target/debug/deps/executor-c1650299697d7408)
   Doc-tests executor

```

**Files changed:**
- `docs/dev/milestones/M39-executor-cache-accounting/README.md` — +1 -1
- `docs/dev/milestones/M39-executor-cache-accounting/phase-01-capture-cache-write.md` — +7 -1
- `executor/src/ai/backends/openai.rs` — +130 -2

**Commit:** d3e087ce7dd212cf41c32ed895bfa014d1d81e5a

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

