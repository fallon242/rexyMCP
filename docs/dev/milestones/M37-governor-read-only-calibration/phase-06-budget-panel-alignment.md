# Phase 06: Budget panel — align debit decimals + `Tokens` header

**Milestone:** M37 — Governor Read-Only Calibration
**Status:** todo
**Depends on:** none (M38's `ledger_lines`; independent of 01–05)
**Estimated diff:** ~70 lines
**Tags:** language=rust, kind=bugfix, size=s

## Goal

Two small budget-panel (`ledger_lines`) fixes found using the dashboard, plus
the repair of a fake test that let the first one regress:

1. **Debit decimals sit one column left of credit decimals.** A debit renders
   `($1940.72)` and a credit `$1414.70`; right-aligned in the same column, the
   debit's trailing `)` pushes its `.` one column left of the credit's `.`. This
   is the M35 07d–07h alignment problem, reintroduced when M38 phase-02 rewrote
   this renderer. Align them via a trailing **sign-gutter** on non-debit values.

2. **Tokens-mode header reads `Spend (tok)`; make it `Tokens`.** When the `b`
   toggle is on tokens, the header should say `Tokens`, not `Spend (tok)`.

3. **The guard test is fake and must be fixed.**
   `savings_lines_debit_digits_align_with_non_debit`
   (`mcp/src/dashboard/panels.rs:1920`) has a docstring promising "the decimal
   point of a debit row and a non-debit row must be at the same column index"
   but a body that only asserts **all rows are equal width**. Equal width ≠
   aligned decimals — which is exactly why defect 1 shipped. The test must assert
   the decimal-column equality it claims.

## Architecture references

Read before starting:

- `docs/dev/milestones/M35-metrics-cost-accounting/phase-07g-dash-decimal-align.md`
  and `phase-07h-dash-tight-parens.md` — the sign-gutter / decimal-column
  convention this restores (`—` and `.` land 3 chars from a cell's right edge).
- `docs/dev/milestones/M38-discount-accounting/phase-02-ledger-layout-shared-renderer.md`
  — where `ledger_lines` was written and where the alignment regressed.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**`mcp/src/costs.rs` `ledger_lines` / `paren` / `make_row`** own the rendering.
The dashboard's `savings_lines` delegates to `ledger_lines` entirely
(`panels.rs:519`), so this fixes both the CLI `rexymcp costs` table and the
dashboard Budget panel at once.

**The exact misalignment** (reproduced; `make_row` uses `{:>10}` per value cell
with a milestone, `{:>9}` without):

```
  Architect:       (—)        (—)  ($1940.72)     <- '.' at column 38
  Executor:     $13.76     $27.97    $1414.70     <- '.' at column 39
  Net:               —          —   ($526.03)     <- '.' at column 38
```

Debit forms (`($1940.72)`, `(—)  `) place their marker 3 chars from the cell's
right edge; credit forms (`$1414.70`) place theirs 2 chars in, and the
non-debit `DASH = "—  "` also 2 in — one column right of the debits.

**The value producers in `ledger_lines`:**
- `paren(v)` — debit wrapper: `($X.XX)` for a value, `"(—)  "` for no-value.
  **Debit forms are already correct; do not touch `paren`.**
- `executor_val` / `net_val` — return a **credit** `$X.XX` (via `fmt_dollars`),
  a **debit** `(X.XX)` / `(${...})` when negative, or the non-debit
  `DASH = "—  "` when `None`.

## Spec

### 1. Add a 1-char sign-gutter to non-debit values

The rule: **debit forms (parenthesised, ending in `)`) are unchanged; every
non-debit form gains one trailing space** so its marker lands 3 chars from the
right edge, matching the debits.

Two concrete edits — the worked target (reproduced, exact):

- **Credit dollars** — where `executor_val`/`net_val` return `fmt_dollars(val)`
  (`$X.XX`), append one trailing space: `format!("{} ", fmt_dollars(val))` →
  `"$1414.70 "`. Do **not** add the space to the negative/debit branch
  (`(X.XX)` / `(${...})`), which is already a debit form.
- **Non-debit dash** — change `const DASH: &str = "—  ";` to
  `const DASH: &str = "—   ";` (three trailing spaces).

After the fix, all three rows' markers land at the same column:

```
  Architect:       (—)        (—)  ($1940.72)     <- '.' col 38
  Executor:     $13.76     $27.97    $1414.70     <- '.' col 38  (number shifted 1 left)
  Net:               —          —   ($526.03)     <- '.' col 38
```

The debit rows (Architect, and Net/Executor when negative) are visually
unchanged; the credit numbers and dashes shift one column left into alignment.

**Tokens mode is unaffected** — token cells (`fmt_tokens`) are never
parenthesised, so they are already internally consistent. Do not add a gutter in
tokens mode.

### 2. Tokens-mode header `Spend (tok)` → `Tokens`

In `ledger_lines`, the `LedgerUnits::Tokens` header branches (both the
`has_milestone` and no-milestone arms) render `"Spend (tok)"`. Change the label
to `"Tokens"`. The `LedgerUnits::Dollars` header stays `"Spend"`.

### 3. Fix the fake alignment test

In `mcp/src/dashboard/panels.rs`, `savings_lines_debit_digits_align_with_non_debit`
(`:1920`) currently asserts only equal row width. Replace the body's assertion
with the decimal-column-equality check its docstring already promises: find the
`Architect:` and `Executor:` rendered lines and assert
`architect.find('.') == executor.find('.')` (for a fixture where both rows carry
a dollar value in the same scope). Keep the equal-width check too if convenient —
but the decimal-equality assertion is the one that must be present. This test
**must fail** against the pre-fix code.

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes.
- [ ] In dollars mode, the `.` (or `—`) of the `Architect:`, `Executor:`, and
      `Net:` rows are at the **same column index** in each scope — pinned by a
      real decimal-equality test, not a width check.
- [ ] Tokens mode renders the header `Tokens` (not `Spend (tok)`); dollars mode
      still renders `Spend`.
- [ ] `savings_lines_debit_digits_align_with_non_debit` asserts decimal-column
      equality and would fail against the old rendering.

## Test plan

`ledger_lines` is pure; `savings_lines` is a thin adapter over it. Test through
whichever the existing tests use — the module already has `savings_lines`-based
render tests (`panels.rs`) and `ledger_lines` tests (`costs.rs`).

- **Strengthen** `savings_lines_debit_digits_align_with_non_debit` per § Spec 3 —
  decimal-column equality between Architect and Executor. This is the load-bearing
  test; it must fail on the pre-fix code (verify by reverting the gutter locally).
- `ledger_dash_and_decimal_share_column` — a fixture where one scope's Architect
  is `(—)` (debit dash), Executor is a `$X.XX` credit, and Net is `—` (non-debit
  dash); assert the `—`/`.`/`—` markers are at the same column index across the
  three rows.
- `ledger_tokens_header_is_tokens` — tokens mode header equals `Tokens` (exact),
  and does **not** contain `Spend`. (Negative case: guards against renaming to
  something that still says Spend.)
- `ledger_dollars_header_still_spend` — dollars mode header still begins `Spend`.
  Pins that fix 2 is scoped to tokens mode only.

Pin **marker column equality** and **exact header text**, not full-line byte
layout, per WORKFLOW § "Specs pin behavior, not rendering".

## End-to-end verification

Against the real binary — the CLI `costs` table shares the renderer, so it shows
the alignment directly:

```bash
cargo run -p rexymcp -- costs --config rexymcp.toml --repo .
cargo run -p rexymcp -- costs --config rexymcp.toml --repo . --tokens | head -3
```

Paste both. Expected: in the dollars output, the `.`/`—` of the Architect,
Executor, and Net rows line up vertically in each column; in the `--tokens`
output, the header reads `Tokens`. (The dashboard Budget panel renders the same
`ledger_lines`, so it is fixed in lock-step — note that rather than claiming a
separate TUI screenshot.)

## Authorizations

None. No new dependencies. No edits to `docs/architecture.md`.

## Out of scope

- Widening the value columns to align at the *credit* column instead (the
  "keep Executor put, move Architect right" alternative). The chosen fix aligns
  at the debit column via the sign-gutter — the minimal, standard-accounting
  change (user decision).
- Any tokens-mode alignment change — token cells are never parenthesised.
- The `b`-key handler, panel borders, `budget_lines`, or any non-ledger panel.
- Governor / calibrate-governor / completion-entry code (the rest of M37).

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
