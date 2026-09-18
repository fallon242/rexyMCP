# Phase 1: fork milestone ids

**Milestone:** F08 — Architect tokens by milestone
**Status:** todo
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
