# Phase 1: dated completion entry

**Milestone:** F07 — Completion-entry date
**Status:** in-progress
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
