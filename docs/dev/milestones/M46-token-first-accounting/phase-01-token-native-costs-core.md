# Phase 01: Token-native costs core

**Milestone:** M46 — Token-First Accounting
**Status:** todo
**Depends on:** none
**Estimated diff:** ~800 lines (deletion-heavy: the dollars branch of the ledger and its tests come out; new code is the Cache row, the token-share by-skill table, and retargeted tests)
**Tags:** language=rust, kind=refactor, size=l

## Goal

Make the entire `rexymcp costs` render path — CLI table, `--json`, and the
dashboard Budget panel that shares its renderer — token-native, and delete the
dollars mode it replaces. After this phase no output surface of `costs` or the
dashboard contains a dollar value, and the previously-invisible cache-token
split gets its first display: a `Cache:` hit-ratio row in the ledger.

## Architecture references

Read before starting:

- `docs/architecture.md` § Status #46 — this milestone's summary (why dollars
  are being removed; what token reporting gains).
- `docs/architecture.md` § Status #40 — the tokens-mode dash/decimal alignment
  invariant this phase must preserve.
- `docs/architecture.md` § Status #38 — the two-line Architect/Executor ledger
  and the shared-renderer rule (dashboard and CLI produce identical strings);
  the ledger shape survives, its dollar semantics do not.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

All dollar values are derived at read time; nothing on disk is
dollar-denominated, so no serialized schema changes in this phase.

**`mcp/src/costs.rs`** (the file this phase mostly rewrites; ~545 lines code +
~1350 lines tests):

- `ScopeReport` (`costs.rs:20`) carries four dollar fields (`saved`,
  `executor`, `architect`, `net` — `f64`/`Option<f64>`) plus two token totals
  (`executor_tokens`, `architect_tokens`). It is `serde::Serialize` — the
  dollar fields leak into `costs --json`.
- `scope_report` (`costs.rs:47`) multiplies token counts by `$/Mtok` rates. The
  only parts worth keeping are the two token-total folds at `costs.rs:76-86`.
- `SkillCost` (`costs.rs:100`) is `{ skill, tokens: u64, cost: f64 }`;
  `skill_costs` (`costs.rs:122`) prices each ledger via
  `architect.rates_for(&l.model)` + `l.cost(i, o)` and sorts by
  `b.cost.total_cmp(&a.cost)` (`costs.rs:155`).
- `scope_costs` (`costs.rs:165`) sums the executor's four token classes per
  scope (keep verbatim — this is the u64-widening discipline) and also prices
  the architect ledger into `ScopeCosts.architect_cost` (goes).
- `load_cost_report` (`costs.rs:222`) resolves rates from config at
  `costs.rs:242-248` (`cfg.architect.effective_rates()`,
  `cfg.model_rates(...)`) purely to feed `scope_report`.
- `format_costs_with` (`costs.rs:324`) renders the table; the by-skill block at
  `costs.rs:341-364` prints a `COST` column with `${:.2}` and computes `%` as
  a *dollar* share — even in `--tokens` mode.
- `ledger_lines` (`costs.rs:405`) has a `units: LedgerUnits` parameter and two
  branches. The tokens branch (`costs.rs:438-472`) is the keeper; its `Net:`
  row renders a bare `TOK_DASH` in all three scopes — dead space. The dollars
  branch (`costs.rs:474-538`), `LedgerUnits` (`costs.rs:371`), and `paren`
  (`costs.rs:378`) all go.

**`mcp/src/dashboard/`** (mechanical cascade — the dashboard shares the
renderer, so it cannot stay compiled without these edits):

- `panels.rs:18-25` `ScopeCosts` has one dollar field: `architect_cost:
  Option<f64>`.
- `panels.rs:29-33` `BudgetDisplay` (Dollars/Tokens), `panels.rs:37-42`
  `BudgetRates` — both exist only to feed the dollars branch.
- `panels.rs:490-521` `savings_lines(summary, rates, milestone_costs,
  project_costs, _project_escalation_count, display)` maps `BudgetDisplay →
  LedgerUnits` and delegates to `costs::ledger_lines`.
- `event_loop.rs:24` `budget_display` state; `event_loop.rs:120-125` the `b`
  keypress toggling it.
- `render.rs:23` `DashboardState.budget_display`; `render.rs:263-270` Top
  skill, gated on `ts.cost > 0.0` and printed as `${:.2}`:

  ```rust
  if let Some(ref ts) = data.top_skill
      && ts.cost > 0.0
  {
      budget.push(Line::from(format!(
          "  Top skill: {} ${:.2}",
          ts.skill, ts.cost
      )));
  }
  ```

- `mod.rs:22` re-exports `BudgetRates`; `mod.rs:132` takes
  `skill_costs(...).into_iter().next()` as `top_skill` — after this phase that
  means top-by-tokens, which is the intended new semantics.

**`mcp/src/main.rs`**: the `Costs` subcommand has a `--tokens` flag
(`main.rs:300-302`), mapped to `LedgerUnits` at `main.rs:979-983`; the
`dashboard` subcommand builds a `BudgetRates` from config at `main.rs:948`.
CLI parse test `cli_parse_costs_with_defaults` at `main.rs:1593`.

## Spec

### 1. Strip `ScopeReport` and `scope_report` to tokens

In `mcp/src/costs.rs`, replace the struct and function:

```rust
/// One scope's token totals. All-`u64`; summed per-run with `saturating_add`
/// (never routed through the u32 `TokenBreakdown`).
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct ScopeReport {
    /// Non-cached executor input tokens (the disjoint `input` class).
    pub executor_input: u64,
    pub executor_output: u64,
    pub executor_cache_read: u64,
    pub executor_cache_write: u64,
    /// All four executor classes summed. `0` when the scope has no runs.
    pub executor_tokens: u64,
    /// Architect tokens for this scope, all four classes summed.
    pub architect_tokens: u64,
}

/// Fold one scope's `ScopeCosts` into its token report.
pub fn scope_report(costs: &ScopeCosts) -> ScopeReport {
    // ... executor_tokens / architect_tokens exactly as today's
    // costs.rs:76-86 saturating_add folds; the four class fields copied
    // straight from `costs` ...
}
```

No rates parameters. Delete the `per_m` closure and all dollar math. Rewrite
the file-header doc comment (`costs.rs:1-5`) — it currently describes
"Saved / Executor / Architect / Net" dollar reporting.

### 2. Tokenize `SkillCost` / `skill_costs`

`SkillCost` becomes `{ skill: String, tokens: u64 }`. `skill_costs` drops the
`architect: &ArchitectConfig` parameter and all pricing (the `rates_for` /
`l.cost(...)` call at `costs.rs:139-141` goes); the token fold and
`display_skill` grouping stay byte-identical. New sort:

```rust
out.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.skill.cmp(&b.skill)));
```

Update the two call sites: `costs.rs:301` and `dashboard/mod.rs:132`.

### 3. Strip `scope_costs` and `ScopeCosts` of architect pricing

- `panels.rs`: delete the `architect_cost: Option<f64>` field from
  `ScopeCosts`. Do not leave it unread — `mcp` is a binary crate and an unread
  field fails `clippy -D warnings` as dead code.
- `costs.rs` `scope_costs`: drop the `architect` parameter; in the
  project-scope arm keep the `ArchitectTokens` fold (`costs.rs:200-203`) and
  delete the `cost` accumulator (`costs.rs:195`, `:204-206`). The
  milestone-scope arm keeps returning `ArchitectTokens::default()` (the ledger
  has no milestone dimension — keep the comment at `costs.rs:190`).

### 4. `ledger_lines` loses its units — and gains a `Cache:` row

Signature: `pub fn ledger_lines(session: &ScopeReport, milestone:
Option<&ScopeReport>, project: &ScopeReport) -> Vec<String>`.

Delete: `LedgerUnits`, `paren`, the entire Dollars branch (`costs.rs:474-538`
including the `DASH` constant and the M35-07g sign-gutter comment), and the
dollars header arm. Keep the tokens branch verbatim — header word `"Tokens"`,
the `TOK_DASH` constant, the `tok` closure, and the M40 render-level-padding
comment (`costs.rs:439-449`) — with one change: the `Net:` row
(`costs.rs:466-472`) becomes a `Cache:` row showing the **executor cache-hit
ratio** per scope.

Definition (pin this exactly): the hit ratio is prompt-side only —

```
hit_pct = cache_read / (input + cache_read + cache_write) * 100
```

over the executor's three disjoint input classes; output tokens are excluded.
Cell rendering:

```rust
let cache_cell = |r: &ScopeReport| -> String {
    let denom = r.executor_input + r.executor_cache_read + r.executor_cache_write;
    if r.executor_cache_read + r.executor_cache_write == 0 || denom == 0 {
        TOK_DASH.to_string()
    } else {
        format!("{:.1}%", r.executor_cache_read as f64 / denom as f64 * 100.0)
    }
};
```

The no-cache-activity case renders `TOK_DASH`, not `0.0%` — the Session scope
has no cache-class data at all today (the session summary only carries
input/output totals), and printing `0.0%` there would report an absence of
instrumentation as a measurement.

Alignment holds by construction: `NN.N%` places its decimal 2 characters from
the right edge (`.` + digit + `%`), exactly like `X.Xk`/`X.XM` (`.` + digit +
suffix), and the dash case reuses `TOK_DASH`. The M40 marker-column-equality
test must be **extended** to the Cache row, not weakened (task 9).

### 5. `format_costs_with` → `format_costs`, token-share by-skill table

Rename to `pub fn format_costs(report: &CostReport) -> String` (single
caller: `main.rs:994`). Delete the dollars-mode legend block
(`costs.rs:334-338`). The by-skill table becomes three columns:

```
By skill (architect)
SKILL                   TOKENS       %
rexymcp:auto             45.1M    62.0%
architect chat           27.7M    38.0%
```

with `%` = `s.tokens as f64 / total_tokens as f64 * 100.0` (0.0 when the
total is 0), keeping `metrics::fmt_tokens` for the TOKENS column. Column
widths: keep `{:<20}` for SKILL and pick right-aligned widths for the two
numeric columns; the exact widths are yours, the column *set* and the
token-share semantics are pinned.

### 6. `load_cost_report` stops resolving rates

Delete `costs.rs:242-248` (the `effective_rates()` / `BudgetRates` /
`model_rates` block) and pass nothing rate-shaped anywhere. The
config-loading, telemetry-reading, session/milestone/project scoping, and
assists logic all stay as-is. The `use crate::dashboard::{BudgetRates,
ScopeCosts}` import shrinks to `ScopeCosts`.

### 7. Dashboard cascade: delete the dollars mode

- `panels.rs`: delete `BudgetDisplay` and `BudgetRates`. `savings_lines`
  drops its `rates` and `display` parameters (keep
  `_project_escalation_count` exactly as-is — it is deliberately retained,
  see the comment at `panels.rs:495`) and calls
  `costs::ledger_lines(&sess, mile.as_ref(), &proj)`.
- `event_loop.rs`: delete the `budget_display` state (`:24`), the `b` key arm
  (`:120-125`), the `BudgetDisplay`/`BudgetRates` imports, and the `rates`
  parameter threading (`:12`).
- `render.rs`: delete `DashboardState.budget_display` (`:23`) and the `rates`
  parameter (`:213`); Top skill becomes token-gated and token-printed:

  ```rust
  if let Some(ref ts) = data.top_skill
      && ts.tokens > 0
  {
      budget.push(Line::from(format!(
          "  Top skill: {} {}",
          ts.skill,
          metrics::fmt_tokens(ts.tokens)
      )));
  }
  ```

  (This un-hides skills whose model had no configured rate — intended.)
- `mod.rs`: drop the `BudgetRates` re-export (`:22`) and the `rates`
  parameter on the run entrypoint (`:184`); `skill_costs` call site loses the
  `architect` argument (`:132`).
- `main.rs:948`: delete the `BudgetRates` construction for the `dashboard`
  subcommand and stop passing it.

### 8. CLI: remove `--tokens`

In `main.rs`: delete the `tokens` field from `Commands::Costs`
(`main.rs:300-302`), the `units` mapping (`main.rs:979-983`), and update the
subcommand doc comment (`main.rs:278-283`) — it currently says "instead of
dollar values". Call `costs::format_costs(&report)`. `rexymcp costs --tokens`
must now **fail to parse** (clap unknown-argument error) — pin with a test.

### 9. Tests

In `costs.rs` — delete the dollars-mode tests outright (they test deleted
code): `scope_report_priced_executor_and_saved`,
`scope_report_unpriced_executor_is_zero_not_stub`,
`scope_report_no_saved_rate_is_none`, `scope_report_includes_executor_cache`,
`scope_costs_prices_architect_per_model_from_ledger`,
`skill_costs_groups_and_prices_per_model` (reshape to grouping-only),
`skill_costs_sorted_by_cost_desc`, `format_costs_legend_present_when_saved_priced`,
`format_costs_legend_absent_in_tokens_mode`,
`discount_rate_comes_from_architect_config`,
`ledger_executor_row_is_saved_minus_executor_cost`,
`ledger_net_equals_sum_of_rendered_rows`, `ledger_negative_net_is_parenthesised`,
`ledger_positive_net_is_not_parenthesised`,
`ledger_executor_row_renders_when_cost_is_zero`, `ledger_none_net_renders_dash`,
`ledger_none_saved_renders_dash`, `ledger_zero_net_renders_dollar_zero`,
`ledger_dash_aligns_with_decimal_column`, `ledger_dash_and_decimal_share_column`,
`ledger_dollars_header_still_spend`, and the dollar-share assertions inside the
`format_costs_*by_skill*` tests. The `zero_rates`/`priced_exec_rates`/
`priced_saved_rates` fixtures go with them.

Retarget (same behavior, new shape): `ledger_row_order_is_architect_executor_net`
→ order is Architect/Executor/Cache; `ledger_tokens_mode_shows_counts_and_dash_net`
→ counts + Cache row; `scope_costs_milestone_architect_is_none` → milestone
architect tokens are zero. Keep as-is (behavior unchanged):
`scope_costs_none_sums_all_milestones`, `scope_costs_sums_cache_buckets`,
`skill_costs_empty_is_empty`, the three `display_skill`/`architect chat` tests,
`format_costs_omits_milestone_when_none`, `format_costs_shows_milestone_when_some`,
`format_costs_omits_by_skill_when_empty`, `load_cost_report_telemetry_disabled_errors`,
`ledger_tokens_mode_has_no_parens`, `ledger_tokens_header_is_tokens`, and —
load-bearing — `ledger_tokens_dash_aligns_with_decimal_column`, extended so the
marker-column equality also covers the Cache row's `—` and `%` cells.

New tests are listed in the Test plan. `panels.rs` tests that construct
`BudgetRates::default()` / pass `BudgetDisplay::Dollars`
(`panels.rs:1580-1962`, `savings_lines_delegates_to_ledger_lines` at `:2389`)
update to the new signatures; dollars-specific assertions among them are
deleted.

### 10. E2E capture

Run the E2E block from `## End-to-end verification` and paste the captured
files into the `### Update — <date> (end-to-end verification)` entry.

## Acceptance criteria

- [ ] `rexymcp costs` renders a `Tokens` ledger with rows
  `Architect:` / `Executor:` / `Cache:` and a by-skill table with columns
  `SKILL`/`TOKENS`/`%` — and the full output contains no `$` character.
- [ ] `rexymcp costs --json` output contains no key named `saved`, `net`, or
  `cost`, and `ScopeReport` serializes exactly `executor_input`,
  `executor_output`, `executor_cache_read`, `executor_cache_write`,
  `executor_tokens`, `architect_tokens`.
- [ ] `rexymcp costs --tokens` exits non-zero with a clap unknown-argument
  error.
- [ ] `grep -rn 'LedgerUnits\|BudgetRates\|BudgetDisplay\|architect_cost' mcp/src`
  returns nothing.
- [ ] Test `ledger_tokens_dash_aligns_with_decimal_column` passes and asserts
  the Cache row's markers on the same column as the token decimals.
- [ ] Test `cache_row_dashes_when_no_cache_activity` passes (no-cache scope
  renders `—`, not `0.0%`).
- [ ] All four gates green (`cargo fmt --all --check`, `cargo build`,
  `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` —
  separate invocations).

## Test plan

All in the existing `#[cfg(test)] mod tests` blocks of the named files.

- `cache_row_shows_hit_ratio_when_cache_present` in `mcp/src/costs.rs` —
  a scope with `input=600_000, cache_read=300_000, cache_write=100_000`
  renders `30.0%` in the Cache row (300k / 1.0M prompt tokens).
- `cache_row_dashes_when_no_cache_activity` in `mcp/src/costs.rs` — a scope
  with input+output but zero cache classes renders the padded `—`, never
  `0.0%`.
- `skill_costs_sorted_by_tokens_desc` in `mcp/src/costs.rs` — two skills,
  larger token count first; equal counts tie-break by skill name ascending.
- `by_skill_percent_is_token_share` in `mcp/src/costs.rs` — two skills at
  75k/25k tokens render `75.0%` / `25.0%`.
- `costs_output_contains_no_dollar_sign` in `mcp/src/costs.rs` — a fully
  populated report (milestone present, by-skill non-empty) formats to a
  string with no `'$'` anywhere.
- `scope_report_json_is_token_only` in `mcp/src/costs.rs` — serde_json of a
  `ScopeReport` has exactly the six token keys.
- `ledger_tokens_dash_aligns_with_decimal_column` (extended) in
  `mcp/src/costs.rs` — for every row including Cache, marker column ==
  decimal column.
- `cli_rejects_removed_tokens_flag` in `mcp/src/main.rs` —
  `Cli::try_parse_from(["rexymcp", "costs", "--tokens"])` errors.
- `top_skill_line_uses_tokens` (or extend the existing render/panels test
  that covers the Budget block) in `mcp/src/dashboard/` — a top skill with
  `tokens > 0` renders `Top skill: <name> <fmt_tokens>`, no `$`.

## End-to-end verification

The real artifact is the `rexymcp costs` CLI against this repo's own config
and telemetry. Run exactly:

```bash
mkdir -p target/e2e
cargo run -q -p rexymcp -- costs --config rexymcp.toml > target/e2e/costs.txt 2>&1
grep -c '\$' target/e2e/costs.txt > target/e2e/dollar-count.txt || true
cargo run -q -p rexymcp -- costs --config rexymcp.toml --json > target/e2e/costs.json 2>&1
cargo run -q -p rexymcp -- costs --config rexymcp.toml --tokens > target/e2e/tokens-flag.txt 2>&1; echo "exit=$?" >> target/e2e/tokens-flag.txt
```

Paste the contents of all four files into the E2E Update Log entry.
Expected: `costs.txt` shows the Tokens ledger with a `Cache:` row;
`dollar-count.txt` is `0`; `costs.json` has token-only keys; `tokens-flag.txt`
shows a clap error and a non-zero exit.

## Authorizations

None. (No dependencies, no `Cargo.toml`, no config-schema changes — the
`*_per_mtok` config fields still parse; they become unread by this path and
are deleted in phase-03.)

## Out of scope

- **Executor-crate pricing plumbing** — `known_model_rates`,
  `ArchitectConfig`'s rate fields/methods, `Config::model_rates`,
  `metrics::token_cost`, `ArchitectLedger::cost`, and the three
  `CACHE_*_RATE_MULTIPLIER` constants stay untouched (phase-03). They are
  `pub` in the `executor` library crate, so leaving them unread does not trip
  dead-code lints.
- **`runs.rs` / `profile_cli.rs`** — their `COST` columns and `fmt_cost`
  stay for phase-03. Do not touch.
- **`metrics::fmt_tokens`** — shared by scorecard/runs/calibrate-governor;
  its bare `—` is correct there. Padding stays at the `ledger_lines` render
  level (the M40 rule).
- **Reintroducing a `b` keybinding** — phase-02 adds it back as a token-view
  cycler (totals ⇄ cache split) and wires session-scope cache classes. This
  phase only removes; the dashboard ships with a single token view.
- **README / docs prose** — phase-04. (No `mcp/tests/readme_config_reference.rs`
  interaction in this phase: that guard covers config-struct fields, which
  this phase does not remove.)
- The three Levenshtein `cost` variables in
  `executor/src/parser/{repair/name.rs,score.rs,feedback.rs}` are edit
  distance, not money — do not touch.
- Do not "fix" the Session scope's missing cache classes (the summary only
  carries input/output totals) — phase-02. The Session Cache cell correctly
  renders `—` this phase.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
