# Phase 1: fork milestone ids

**Milestone:** F08 — Architect tokens by milestone
**Status:** in-progress
**Depends on:** none
**Estimated diff:** ~50 lines, most of it tests
**Tags:** language=rust, kind=bugfix, size=xs

## Goal

Every phase run is stored with the milestone directory it came from, so
milestone-scoped costs can find it. Today only `M<n>-…` directories count; a
run from `docs/dev/milestones/F07-completion-entry-date/` is stored with
`milestone_id: None`. All 24 fork (`F`) runs in the telemetry store have
`None`. After this phase, any directory named one ASCII uppercase letter then a
digit (`M17-…`, `F07-…`) is a milestone id.

## Pre-flight

1. `cargo test -p rexymcp milestone_id_from_path` → **0 passed** (no tests
   exist; measured 2026-09-18).
2. `cargo test -p rexymcp` unit-test line → **717 passed**.

## Current state

`mcp/src/runner.rs:178-193`, called once at `runner.rs:296`
(`milestone_id: milestone_id_from_path(inp.phase_doc_path)`):

```rust
/// Derive the milestone directory slug from a phase-doc path.
/// `…/milestones/M17-dashboard-polish-3/phase-09.md` → `Some("M17-dashboard-polish-3")`.
/// Returns `None` when the immediate parent does not look like `M<n>-…`.
fn milestone_id_from_path(path: &Path) -> Option<String> {
    let dir_name = path.parent()?.file_name()?.to_str()?;
    let rest = dir_name.strip_prefix('M')?;
    let has_num = rest
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false);
    if has_num {
        Some(dir_name.to_string())
    } else {
        None
    }
}
```

## Spec

Replace the function (doc comment and body) with exactly this. Keep it
private; the signature and the call site do not change.

```rust
/// Derive the milestone directory slug from a phase-doc path.
/// `…/milestones/M17-dashboard-polish-3/phase-09.md` → `Some("M17-dashboard-polish-3")`.
/// `…/milestones/F07-completion-entry-date/phase-01.md` → `Some("F07-completion-entry-date")`.
/// Returns `None` unless the immediate parent starts with one ASCII uppercase
/// letter followed by a digit.
fn milestone_id_from_path(path: &Path) -> Option<String> {
    let dir_name = path.parent()?.file_name()?.to_str()?;
    let mut chars = dir_name.chars();
    let is_milestone = chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars.next().is_some_and(|c| c.is_ascii_digit());
    is_milestone.then(|| dir_name.to_string())
}
```

**Must NOT:**

- Hard-code `'M'` and `'F'` as a list. The rule is any ASCII uppercase letter.
- Touch telemetry records, `costs.rs`, or `profile.rs`. No backfill of existing runs.
- Add a dependency or a regex.

## Acceptance criteria

- [ ] `F07-completion-entry-date` and `M17-dashboard-polish-3` parents both
      return `Some(<dir name>)`.
- [ ] Every negative case in the Test plan returns `None`.
- [ ] The call site at `runner.rs:296` is unchanged.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

Add to the existing `#[cfg(test)] mod tests` in `mcp/src/runner.rs` (starts at
line 602; it already has `use super::*;` in scope — check before adding
imports). Use `Path::new(...)` literals; no filesystem.

1. `milestone_id_from_path_accepts_upstream_m_prefix` —
   `docs/dev/milestones/M17-dashboard-polish-3/phase-09.md` →
   `Some("M17-dashboard-polish-3".to_string())`.
2. `milestone_id_from_path_accepts_fork_f_prefix` —
   `docs/dev/milestones/F07-completion-entry-date/phase-01-dated-completion-entry.md`
   → `Some("F07-completion-entry-date".to_string())`. **Write this one first,
   run it, and quote the failure in the Update Log before changing the function.**
3. `milestone_id_from_path_rejects_non_milestone_dirs` — each returns `None`:
   - `docs/dev/milestones/phase-01.md` (parent is `milestones`)
   - `docs/dev/Misc-notes/phase-01.md` (`M` then a letter)
   - `docs/dev/f07-lower/phase-01.md` (lowercase letter)
   - `docs/dev/7-numeric/phase-01.md` (digit first)
   - `docs/dev/M/phase-01.md` (letter, nothing after)
   - `phase-01.md` (no parent directory)

**Finish condition:** `cargo test -p rexymcp milestone_id_from_path` →
**3 passed**; the `rexymcp` unit-test line → **720 passed**.

## End-to-end verification

Run `cargo test -p rexymcp milestone_id_from_path -- --nocapture` and paste
the output in a `(end-to-end verification)` entry.

## Authorizations

None beyond the Spec.

## Out of scope

- Backfilling `milestone_id` on the 24 stored F runs.
- The architect ledger (phase 02).

## Update Log

<!-- entries appended below this line -->

### Update — 2026-09-18 15:09 (started)

**By:** Claude Opus 4.6 (headless executor)

Starting phase 01. Pre-flight verified per spec: `cargo test -p rexymcp milestone_id_from_path` → 0 passed; `cargo test -p rexymcp` → 717 passed. Plan: (1) add test #2 (F-prefix) first, run it, and quote the failure; (2) replace `milestone_id_from_path` in `mcp/src/runner.rs` per the spec; (3) add tests #1 and #3.

### Update — 2026-09-18 15:09 (test-first failure, as required by spec)

Added only test #2 (`milestone_id_from_path_accepts_fork_f_prefix`) before touching the function and ran it. Quoted failure:

```text
running 1 test
test runner::tests::milestone_id_from_path_accepts_fork_f_prefix ... FAILED

failures:

---- runner::tests::milestone_id_from_path_accepts_fork_f_prefix stdout ----

thread 'runner::tests::milestone_id_from_path_accepts_fork_f_prefix' (328457) panicked at mcp/src/runner.rs:618:9:
assertion `left == right` failed
  left: None
 right: Some("F07-completion-entry-date")
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
 (x2)
failures:
    runner::tests::milestone_id_from_path_accepts_fork_f_prefix

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 717 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p rexymcp --bin rexymcp`
```

The old `strip_prefix('M')` body returned `None` for the F-prefixed directory, as expected. The function was then replaced per the spec, and tests #1 and #3 were added.

### Update — 2026-09-18 15:09 (end-to-end verification)

`cargo test -p rexymcp milestone_id_from_path -- --nocapture` (output captured mechanically to a file and pasted):

```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.11s
     Running unittests src/main.rs (target/debug/deps/rexymcp-cd922bc1926100ad)

running 3 tests
test runner::tests::milestone_id_from_path_rejects_non_milestone_dirs ... ok
test runner::tests::milestone_id_from_path_accepts_upstream_m_prefix ... ok
test runner::tests::milestone_id_from_path_accepts_fork_f_prefix ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 717 filtered out; finished in 0.00s

     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-13fb83281f3fe7ad)

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s
```

Finish condition met: `cargo test -p rexymcp milestone_id_from_path` → 3 passed; `cargo test -p rexymcp` unit-test line → 720 passed. Grep for the new doc-comment literal confirms it landed:

```text
$ grep -n "F07-completion-entry-date" mcp/src/runner.rs
180:/// `…/milestones/F07-completion-entry-date/phase-01.md` → `Some("F07-completion-entry-date")`.
621:        let path = Path::new("docs/dev/milestones/F07-completion-entry-date/phase-01-dated-completion-entry.md");
624:            Some("F07-completion-entry-date".to_string())
```

Full verification commands all passed: `cargo fmt --all --check` (FMT_OK, no output), `cargo build` (Finished, 0 warnings), `cargo clippy --all-targets --all-features -- -D warnings` (Finished), `cargo test` (720 passed / 2 integration passed, 0 failed).
