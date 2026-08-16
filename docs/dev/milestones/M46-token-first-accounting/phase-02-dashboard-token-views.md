# Phase 02: Dashboard token views

**Milestone:** M46 — Token-First Accounting
**Status:** todo
**Depends on:** phase-01
**Estimated diff:** ~350 lines
**Tags:** language=rust, kind=feature, size=m

## Goal

Give the dashboard's Budget panel a second token view and make the Session
scope's cache data real. The `b` key returns as a **view cycler** (Totals ⇄
Cache split); the cache-split view is the first visible surface for the
architect ledger's 5m/1h cache-creation split; and the executor's per-turn
session `Metrics` event gains cache classes so the Session column of the
`Cache:` row (and the new split view) shows measurements instead of `—`.

## Architecture references

Read before starting:

- `docs/architecture.md` § Status #46 — the milestone summary (the `b`
  cycler and session cache wiring are its phase-02 commitments).
- `docs/architecture.md` § Status #39 — the executor cache-token capture
  whose classes this phase finally surfaces per-session.
- `docs/architecture.md` § Status #38 — the shared-renderer rule: ledger
  strings are built in `mcp/src/costs.rs`, the dashboard only wraps them.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

**Session log schema (executor crate).** The per-turn resource snapshot
`SessionEvent::Metrics` (`executor/src/store/sessions/event.rs:81-88`) carries
only `input_tokens` / `output_tokens` (+ context fields) — no cache classes.
A real emitted line looks like:

```json
{"ts":1786920874821,"turn":1,"event":{"event_type":"metrics","input_tokens":15015,"output_tokens":231,"context_pct":0.0203,"context_used":17110,"context_window":838860}}
```

The emit site is `executor/src/agent/mod.rs:620`:

```rust
SessionEvent::Metrics {
    input_tokens: metrics.tokens.input_tokens,
    output_tokens: metrics.tokens.output_tokens,
    context_pct: deps.budget.fraction_used(&system, &messages),
    ...
},
```

and `metrics.tokens` is a `TokenBreakdown` whose `cache_read_tokens` /
`cache_write_tokens` are **already accumulated** per turn
(`executor/src/agent/metrics.rs:40-50`, `add_tokens`) — the data exists at the
emit site; it just isn't written.

**Status summarize (mcp crate).** `mcp/src/status.rs:191-198` copies the
Metrics event into `StatusSummary.last_input_tokens` / `last_output_tokens`
(both `Option<u32>`, declared at `status.rs:59-61`). No cache fields.

**Costs session scope.** `mcp/src/costs.rs` `load_cost_report` builds the
session `ScopeCosts` from those two summary fields only (`executor_cache_read`
/ `executor_cache_write` default to 0), so the `Cache:` row's Session cell is
always `—`. Same wiring in `mcp/src/dashboard/panels.rs` `savings_lines`
(`panels.rs` ~`:470-490` after phase-01: builds `session_costs` from
`summary.last_input_tokens` / `last_output_tokens`).

**The ledger renderer.** `costs::ledger_lines(session, milestone, project)`
(phase-01 shape) renders header + Architect/Executor/Cache rows using
`make_row` (`{:<10}` label + three `{:>10}` columns, or two `{:>9}` without a
milestone), the `TOK_DASH = "—  "` padding, and the `tok` closure over
`metrics::fmt_tokens`. The M40 alignment test
`ledger_tokens_dash_aligns_with_decimal_column` pins marker columns.

**The `b` key.** Removed in phase-01 (`event_loop.rs` no longer has a
`KeyCode::Char('b')` arm; `render.rs`'s `DashboardState` has no display
field). `savings_lines(summary, milestone_costs, project_costs,
_project_escalation_count)` renders the single Totals view.

**The 5m/1h split.** `ArchitectLedger.cache_creation_5m` /
`cache_creation_1h` (`executor/src/store/telemetry.rs:622,626`, both
`#[serde(default)]` `u64`) are harvested token fields with **zero readers**
since phase-01 deleted `ArchitectLedger::cost`. `dashboard/mod.rs` already
holds the folded `ledgers` vec where project scoping happens.

## Spec

### 1. Extend `SessionEvent::Metrics` with cache classes (backward-compatible)

In `executor/src/store/sessions/event.rs`, add to the `Metrics` variant:

```rust
Metrics {
    input_tokens: u32,
    output_tokens: u32,
    /// Cumulative executor cache-read / cache-write tokens (M46 phase-02).
    /// `#[serde(default)]` so pre-M46 session logs (no such keys) still parse.
    #[serde(default)]
    cache_read_tokens: u32,
    #[serde(default)]
    cache_write_tokens: u32,
    context_pct: f64,
    ...
},
```

Update the emit site (`executor/src/agent/mod.rs:620`) to write
`metrics.tokens.cache_read_tokens` / `metrics.tokens.cache_write_tokens`.
Update the non-wildcard `Metrics` constructors in tests (e.g.
`executor/src/store/telemetry.rs:871`); the `{ context_pct, .. }` pattern at
`telemetry.rs:72` needs no change.

### 2. Surface the classes in `StatusSummary`

In `mcp/src/status.rs`: add `pub last_cache_read_tokens: Option<u32>` and
`pub last_cache_write_tokens: Option<u32>` next to the existing token fields
(`status.rs:59-61`), and fill them in the `SessionEvent::Metrics` arm
(`status.rs:191-198`) exactly like `last_input_tokens`.

### 3. Wire the session `ScopeCosts` (both call paths)

In `mcp/src/costs.rs` `load_cost_report` and in `mcp/src/dashboard/panels.rs`
`savings_lines`, the session `ScopeCosts` gains:

```rust
executor_cache_read: summary.last_cache_read_tokens.unwrap_or(0) as u64,
executor_cache_write: summary.last_cache_write_tokens.unwrap_or(0) as u64,
```

After this, `rexymcp costs` and the dashboard Totals view show a real Session
`Cache:` percentage for sessions logged by the new binary, and `—` for
pre-M46 logs (zero classes) — the phase-01 no-cache-activity rule already
handles that; do not special-case it.

### 4. `cache_split_lines` — the new view's renderer, in `costs.rs`

Per the shared-renderer rule, build the strings in `mcp/src/costs.rs`:

```rust
/// The Cache-split view: executor read/write per scope, plus the architect
/// ledger's 5m/1h cache-creation split (project scope only — the ledger has
/// no session or milestone dimension).
pub fn cache_split_lines(
    session: &ScopeReport,
    milestone: Option<&ScopeReport>,
    project: &ScopeReport,
    arch_cache_5m: u64,
    arch_cache_1h: u64,
) -> Vec<String>
```

Layout — same `make_row` widths, same `TOK_DASH` / `tok` conventions as
`ledger_lines` (factor the `tok` closure + `TOK_DASH` into a small shared
helper rather than duplicating them):

```
Cache          Session Milestone   Project
  Read:         90.2k     25.9M     53.5M     <- executor_cache_read per scope
  Write:            —         —    810.4k     <- executor_cache_write per scope
  Arch 5m:          —         —      1.5M     <- arch_cache_5m, Project only
  Arch 1h:          —         —    500.3k     <- arch_cache_1h, Project only
```

Header word is `Cache` (label column), scope columns identical to
`ledger_lines`. The `Arch 5m:` / `Arch 1h:` Session and Milestone cells are
**always** `TOK_DASH` (no per-session/per-milestone architect data exists).
Zero values render the padded dash via `tok`, exactly like `ledger_lines`.
Marker-column alignment must hold across all four rows — extend or mirror the
M40-style alignment test for this view.

### 5. The `b` cycler

- `mcp/src/dashboard/panels.rs`: new enum

  ```rust
  /// Which token view the Budget panel renders. `b` cycles.
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub enum TokenView {
      #[default]
      Totals,
      CacheSplit,
  }
  ```

- `savings_lines` gains a `view: TokenView` parameter and two data
  parameters for the split (`arch_cache_5m: u64, arch_cache_1h: u64`);
  `Totals` renders `ledger_lines` as today, `CacheSplit` renders
  `cache_split_lines`. The empty-session guard (return empty when
  `summary.last_input_tokens` is `None`) applies to both views.
- `mcp/src/dashboard/event_loop.rs`: reintroduce state + the key arm:

  ```rust
  KeyCode::Char('b') => {
      token_view = match token_view {
          TokenView::Totals => TokenView::CacheSplit,
          TokenView::CacheSplit => TokenView::Totals,
      };
  }
  ```

  threading it through `DashboardState` (`render.rs`) like the removed
  `budget_display` was.
- `mcp/src/dashboard/mod.rs`: `DashboardData` gains
  `arch_cache_5m: u64` / `arch_cache_1h: u64`, summed with `saturating_add`
  over the folded project-scoped ledgers
  (`l.cache_creation_5m` / `l.cache_creation_1h` for
  `l.project_id == Some(pid)`), `0` in the no-project and error arms.
- `render.rs`: pass the view + split values into `savings_lines`. The Top
  skill line renders in both views (unchanged).
- Update the dashboard help/footer line that lists keybindings, if it
  enumerates keys (check `render.rs`/`panels.rs` for a hints line), to
  include `b`.

### 6. Tests

Per the Test plan below. Old-log compatibility is load-bearing: a `metrics`
JSON line **without** the two new keys must deserialize with zeros.

### 7. E2E capture

Run the E2E block from `## End-to-end verification` and paste the captured
files into the `### Update — <date> (end-to-end verification)` entry.

## Acceptance criteria

- [ ] A pre-M46 `metrics` session-log line (the exact JSON in Current state)
  deserializes: `cargo test` includes a test parsing that literal line with
  `cache_read_tokens == 0 && cache_write_tokens == 0`.
- [ ] A fixture session log whose `metrics` event carries
  `cache_read_tokens`/`cache_write_tokens` produces a real Session `Cache:`
  percentage in `rexymcp costs` output (E2E block below).
- [ ] `cache_split_lines` renders the four-row layout above with
  marker-column alignment pinned by a test.
- [ ] The dashboard `b` key cycles Totals ⇄ CacheSplit (state-level test on
  the view enum + `savings_lines` branch; the TUI key loop itself is not
  unit-testable — cover the render branch, not the terminal I/O).
- [ ] `arch_cache_5m`/`arch_cache_1h` are summed from project-scoped ledgers
  and rendered in the split view's Project column (test with two ledger
  fixtures).
- [ ] No `$` anywhere in any new output; no executor-crate behavior change
  other than the two new emitted fields.
- [ ] All four gates green (separate invocations).

## Test plan

- `metrics_event_without_cache_fields_parses` in
  `executor/src/store/sessions/event.rs` (or the module's existing test
  block) — deserializes the pre-M46 JSON line verbatim; asserts zeros.
- `metrics_event_roundtrips_cache_fields` in the same block — serialize →
  deserialize preserves the two new counts.
- `summarize_reads_cache_classes_from_metrics` in `mcp/src/status.rs` — a
  records fixture with cache-bearing metrics fills both new summary fields.
- `session_scope_cache_cells_from_summary` in `mcp/src/costs.rs` — a
  summary with `cache_read=300k, input=600k, cache_write=100k` yields a
  session `Cache:` cell of `30.0%` via `load_cost_report`'s wiring (test at
  the `scope_report`/`ledger_lines` level with a hand-built `ScopeCosts`).
- `cache_split_lines_renders_four_rows` in `mcp/src/costs.rs` — labels
  `Read:`/`Write:`/`Arch 5m:`/`Arch 1h:` in order; executor cells per scope;
  architect cells dash outside Project.
- `cache_split_alignment_matches_decimal_column` in `mcp/src/costs.rs` —
  the M40-style marker-column equality for the split view.
- `savings_lines_cache_split_delegates_to_cache_split_lines` in
  `mcp/src/dashboard/panels.rs` — the dashboard's split-view strings equal
  `cache_split_lines` for the same inputs (mirror of the existing
  delegation test).
- `dashboard_data_sums_arch_cache_split` in `mcp/src/dashboard/mod.rs` —
  two project ledgers with 5m/1h values sum; a foreign-project ledger is
  excluded.

## End-to-end verification

The real artifacts are the session-log schema and the `costs` CLI. Run
exactly:

```bash
mkdir -p target/e2e/fixture-repo/.rexymcp/sessions
cat > target/e2e/fixture-repo/.rexymcp/sessions/session-e2e-cache.jsonl <<'EOJ'
{"ts":1786920874821,"turn":1,"event":{"event_type":"session_start","phase":"e2e","model":"fixture"}}
{"ts":1786920874822,"turn":1,"event":{"event_type":"metrics","input_tokens":600000,"output_tokens":1000,"cache_read_tokens":300000,"cache_write_tokens":100000,"context_pct":0.1,"context_used":1,"context_window":10}}
EOJ
cargo run -q -p rexymcp -- costs --config rexymcp.toml --repo target/e2e/fixture-repo > target/e2e/costs-cache-session.txt 2>&1
grep 'Cache:' target/e2e/costs-cache-session.txt > target/e2e/cache-row.txt
cat > target/e2e/fixture-repo/.rexymcp/sessions/session-e2e-old.jsonl <<'EOJ'
{"ts":1786920874923,"turn":1,"event":{"event_type":"session_start","phase":"e2e-old","model":"fixture"}}
{"ts":1786920874924,"turn":1,"event":{"event_type":"metrics","input_tokens":15015,"output_tokens":231,"context_pct":0.0203,"context_used":17110,"context_window":838860}}
EOJ
cargo run -q -p rexymcp -- costs --config rexymcp.toml --repo target/e2e/fixture-repo --session session-e2e-old > target/e2e/costs-old-session.txt 2>&1
grep 'Cache:' target/e2e/costs-old-session.txt >> target/e2e/cache-row.txt
```

Adjust the `session_start` fixture line to the real `SessionEvent` variant
shape if it differs (read `event.rs` first); the load path may also accept a
log whose first record is a `metrics` event — whatever minimal fixture
`status::load_records` accepts is fine, the pinned behavior is the two
`Cache:` cells. Paste all captured files. Expected: `cache-row.txt`'s first
line shows a Session cell of `30.0%`; the second line's Session cell is `—`
(old-format line parsed, zero cache classes). If `--session` does not select
by bare id, select the old-session fixture however `status` resolves sessions
(e.g. separate fixture repos) — the pinned behavior is unchanged.

The `b` cycler cannot be exercised non-interactively — it is covered by the
delegation + enum tests above; say so in the E2E entry rather than faking a
TUI capture.

## Authorizations

None. (The `SessionEvent::Metrics` extension is additive with
`#[serde(default)]` — no schema-version bump, no `Cargo.toml`, no contract
docs.)

## Out of scope

- **Pricing plumbing** (`known_model_rates`, config rate fields,
  `metrics::token_cost`, the `CACHE_*_RATE_MULTIPLIER` constants,
  `runs`/`profile` COST columns) — phase-03. The multipliers'
  orphaned state is expected this phase.
- **A CLI flag for the cache-split view** — not in this milestone's scope;
  the split view is dashboard-only. Do not add `costs --cache`.
- **`PhaseRun` / telemetry schema** — untouched; only the session-log event
  gains fields.
- **Removing `dashboard/mod.rs`'s `_architect` parameter** — phase-03.
- **Back-filling cache data for old sessions** — impossible and not wanted;
  old logs render `—`.
- The Levenshtein `cost` variables in `executor/src/parser/` — not money, do
  not touch.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
