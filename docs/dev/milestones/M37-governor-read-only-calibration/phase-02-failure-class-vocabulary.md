# Phase 02: Add `oscillation_stall` and `missing_spec_test` to `FAILURE_CLASSES`

**Milestone:** M37 — Governor Read-Only Calibration
**Status:** in-progress
**Depends on:** none (independent of phase-01; both may land in either order)
**Estimated diff:** ~50 lines
**Tags:** language=rust, kind=feature, size=s

## Goal

Two failure classes have been recorded as **open-vocabulary** values — accepted
with a warning, and bucketed as noise by every aggregator that groups on the
canonical list. Both have recurred enough to earn a name:

- **`oscillation_stall`** — recorded 2× during M35 for runs the governor
  terminated on an oscillation / identical-repetition / stall detector.
- **`missing_spec_test`** — recorded 2× during M38 phase-01, for an otherwise
  correct implementation that omitted a test the spec's § Test plan named.

The code change is small. **The load-bearing part is the doc comment on each
new entry**, because the taxonomy's only defence against drift is that two
reviewers reading the same bounce pick the same class.

## Architecture references

Read before starting:

- `docs/architecture.md` § Status #37 — this milestone, including why
  `missing_spec_test` fits none of the nine existing classes.
- `docs/architecture.md` § Status #7 — the `PhaseReview` / failure-class design
  and the reason `spec_bug`/`infra_blip` exist: so a bounce that is *not the
  model's fault* is not charged against its competency. The two new classes sit
  on opposite sides of that line, which is the distinction the comments must
  carry.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**`executor/src/store/telemetry.rs:319-329`** — the canonical list. Note the
doc comment above it already says the vocabulary is *intentionally open* and
that "new classes fold in as they recur (WORKFLOW § Calibration)". This phase is
that fold; no policy change is needed.

```rust
pub const FAILURE_CLASSES: &[&str] = &[
    "none",              // clean approval
    "false_completion",  // self-reported complete on a red gate
    "prod_unwrap",       // unwrap/expect in a production path (STANDARDS §2.1)
    "multi_site_break",  // breaking multi-site type change ran out of verifier runway
    "parse_format",      // tool-call format / forgiving-parser repair churn
    "masked_diagnostic", // #[allow]/#[ignore] used to hide a warning/error
    "scope_deviation",   // touched out-of-scope files or widened scope
    "spec_bug",          // the bounce was the architect's spec fault, not the model's
    "infra_blip",        // transient backend/decode error, not a work defect
];

/// True if `class` is in the canonical `FAILURE_CLASSES` vocabulary.
pub fn is_known_failure_class(class: &str) -> bool {
    FAILURE_CLASSES.contains(&class)
}
```

**The warning path is automatic.** `mcp/src/review.rs:62-67` filters the
supplied classes through `is_known_failure_class`; `mcp/src/main.rs:862` prints
`warning: unknown failure class …` when any survive. Adding entries to the const
silences both — **no change is needed in `review.rs` or `main.rs`.**

**The existing vocabulary test** is
`telemetry.rs:1288` `is_known_failure_class_validates_vocabulary`, which asserts
four known classes and one made-up one.

## Spec

### 1. Add the two entries

Append to `FAILURE_CLASSES` in `executor/src/store/telemetry.rs`, keeping the
existing alignment of the trailing comments:

```rust
    "oscillation_stall", // governor terminated the run (oscillation / identical-repetition / stall)
    "missing_spec_test", // implementation correct, but a test the spec named was not written
```

Order matters only for readability — append at the end, after `infra_blip`, so
the diff is additive and the existing entries keep their positions.

### 2. Document the boundaries in the const's doc comment

This is the substantive part. Extend the doc comment above `FAILURE_CLASSES`
with a short paragraph distinguishing the two new classes from their nearest
neighbours, so the taxonomy stays usable:

- **`oscillation_stall` vs `parse_format`** — both show up as a run that made no
  progress, but `parse_format` is *tool-call syntax* churn the forgiving parser
  had to repair, while `oscillation_stall` is the **governor** ending a run that
  was repeating or cycling. If a `HardFail` event names an oscillation,
  identical-repetition, or stall detector, it is `oscillation_stall`.
- **`missing_spec_test` vs `false_completion`** — `false_completion` is a
  **red gate** the model reported as complete. `missing_spec_test` is the
  opposite situation: **all gates genuinely green**, production code correct, but
  a test the § Test plan named was never written, so the behavior is unpinned.
  It is charged to the model (the spec named the test), which is what separates
  it from `spec_bug`.
- **`missing_spec_test` vs `scope_deviation`** — `scope_deviation` is doing
  *more* than the spec asked; `missing_spec_test` is doing *less*.

Keep it tight — three or four sentences. This is a reference a reviewer reads
mid-verdict, not an essay.

### 3. Extend the vocabulary test

Per § Test plan below.

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes.
- [ ] `is_known_failure_class("oscillation_stall")` and
      `is_known_failure_class("missing_spec_test")` both return `true`.
- [ ] `rexymcp review … --failure-class oscillation_stall` records **without**
      the `warning: unknown failure class` line.
- [ ] The nine pre-existing entries are unchanged, in their original order.
- [ ] `mcp/src/review.rs` and `mcp/src/main.rs` are **unmodified** — the warning
      path needs no change.

## Test plan

In `executor/src/store/telemetry.rs`'s `#[cfg(test)] mod tests`:

- Extend the existing `is_known_failure_class_validates_vocabulary` with
  assertions for both new classes. **Add to it; do not replace its existing
  assertions and do not write a second near-duplicate test** — it is the
  vocabulary's single guard and should stay that way.
- `failure_classes_preserves_existing_vocabulary` — asserts every one of the
  nine pre-existing strings is still present. A negative-shaped guard: it fails
  if a future edit renames or drops one while adding new entries, which is the
  realistic way this list decays.

Do **not** assert the list's exact length or the new entries' index. Pinning
length or position makes every future fold a two-line change for no benefit,
and the const's own doc comment says the vocabulary is open.

## End-to-end verification

The warning path is the real artifact — exercise it against the actual binary,
writing to a throwaway telemetry file so the live store is untouched:

```bash
TMP=$(mktemp -d)
cargo run -p rexymcp -- review --config rexymcp.toml \
  --phase-id e2e-vocab --verdict bounced \
  --failure-class oscillation_stall --failure-class missing_spec_test \
  --telemetry-path "$TMP/phase_runs.jsonl" 2>&1

# Negative control — a genuinely unknown class must STILL warn:
cargo run -p rexymcp -- review --config rexymcp.toml \
  --phase-id e2e-vocab --verdict bounced \
  --failure-class definitely_not_a_class \
  --telemetry-path "$TMP/phase_runs.jsonl" 2>&1
```

Paste both outputs in the completion Update Log. Expected: the **first** command
prints only `recorded review for e2e-vocab -> …` with **no** warning line; the
**second** still prints `warning: unknown failure class "definitely_not_a_class"`.

The negative control matters more than the positive one — a change that silenced
the warning for *everything* would pass the first command and quietly destroy the
guard that keeps the vocabulary meaningful.

## Authorizations

None. No new dependencies.

**`docs/architecture.md` must not be edited.** Its § Status #7 mention of the
taxonomy (`false_completion`, `prod_unwrap`, `multi_site_break`, …) is
deliberately elliptical and stays accurate as the list grows.

## Out of scope

- Any change to `mcp/src/review.rs` or `mcp/src/main.rs`. Adding to the const is
  sufficient; touching the warning path risks the negative control above.
- Re-classifying historical `PhaseReview` records that used these strings as
  open-vocab. They already carry the right value — this phase makes them
  *recognised*, retroactively, at read time.
- Making `FAILURE_CLASSES` a closed enum. The const's doc comment states the
  open-vocabulary decision; changing it is an architecture decision, not a
  phase's.
- The other M37 phases (01 read-only exemption, 03 token formatters, 04
  calibrate-governor rendering, 05 completion-entry writer).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-07-24 13:33 (started)

**Executor:** Claude (Sonnet)

Added `oscillation_stall` and `missing_spec_test` to `FAILURE_CLASSES`, extended the doc comment with boundary distinctions, and added vocabulary tests.
