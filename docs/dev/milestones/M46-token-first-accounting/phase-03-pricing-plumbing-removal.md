# Phase 03: Pricing plumbing removal

**Milestone:** M46 — Token-First Accounting
**Status:** in-progress
**Depends on:** phase-02
**Estimated diff:** ~600 lines (deletion-heavy: the whole `$/Mtok` rate layer comes out of both crates; new code is the `CACHE%` columns in `runs`/`profile` and retargeted tests)
**Tags:** language=rust, kind=refactor, size=l

## Goal

Delete the now-unread pricing layer end to end — config rate fields, the
built-in price table, the dollar-computing functions, and the rate constants —
and convert the two remaining dollar-printing CLI surfaces (`rexymcp runs`,
`rexymcp profile --cost`) to token/cache-hit columns. After this phase the
only dollar references left in the repo are README prose (phase-04).

## Architecture references

Read before starting:

- `docs/architecture.md` § Status #46 — the milestone summary; this phase is
  its "pure dead-code sweep" step (phases 01–02 removed every renderer of
  these values).
- `docs/architecture.md` § Status #38 — the precedent this phase applies to
  config: a documented-but-ignored knob actively misleads, so rate keys are
  deleted from structs, scaffolds, and the live `rexymcp.toml` together.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

After phases 01–02, nothing in the render path reads rates — the items below
are computed-but-unread (or read only by each other and by `runs`/`profile`):

**`executor/src/config.rs`:**

- `known_model_rates` (`config.rs:11`) — the built-in Claude price table.
- `ArchitectConfig` (`config.rs:82`): rate fields `model`, `input_per_mtok`,
  `output_per_mtok`, `cache_read_per_mtok`, `cache_creation_per_mtok`, and
  `rates: HashMap<String, ArchitectModelRate>` (`config.rs:111`) with its
  `ArchitectModelRate` value type; methods `effective_rates()`
  (`config.rs:137`), `effective_architect_rates()` (`config.rs:145`),
  `rates_for()` (`config.rs:171`). **`dispatch_model` and `review_model`
  survive** — they are role delegation, not pricing. `model` goes: its only
  consumers are config tests (`config.rs:1671,2140,2173`); its sole purpose
  was auto-filling rates from `known_model_rates`.
- `ModelOverride`'s four rate fields (`config.rs:313-321`):
  `input_per_mtok`, `output_per_mtok`, `cache_read_per_mtok`,
  `cache_creation_per_mtok` — delete the four fields, **keep the struct and
  any non-rate fields it has**.
- `Config::model_rates` (`config.rs:~620`).

Config parsing is `#[serde(default)]` throughout with unknown keys silently
ignored — removing fields breaks no existing config file; a leftover
`input_per_mtok` key in a user's file is simply ignored (that is exactly why
the live-config cleanup below is mandatory, not optional).

**`executor/src/store/telemetry.rs`:**

- `ArchitectRates` (`telemetry.rs:465`) and `pub type ModelRates =
  ArchitectRates` (`telemetry.rs:486`) — config-derived rate carriers.
- The three multipliers (`telemetry.rs:442-444`):
  `CACHE_CREATION_RATE_MULTIPLIER`, `CACHE_CREATION_1H_RATE_MULTIPLIER`,
  `CACHE_READ_RATE_MULTIPLIER`.
- `ArchitectLedger::cost` (`telemetry.rs:638-655`) — their sole consumer —
  plus its test pins (e.g. the deliberately-ignores-`cache_creation` test at
  `telemetry.rs:~1933`). The `cache_creation_5m`/`cache_creation_1h` token
  fields **stay** — phase-02 gave them a reader (`cache_split_lines`).
- `tier_telemetry` doc comments (`telemetry.rs:104`, `:182`) say "M20
  tier/cost instrumentation" but the struct holds no cost — fix the wording
  (it is grep-noise), keep the field.

**`executor/src/store/metrics.rs`:** `token_cost` (`metrics.rs:42`) — the
executor-side dollar function. Its consumers are exactly `runs.rs` and
`profile_cli.rs` below. `fmt_tokens`, `tokens_per_sec`, `reclaimed_total`,
`percentile` stay.

**`mcp/src/runs.rs`:** `fmt_cost` (`runs.rs:59`, renders `—` or `${cost:.4}`);
the list table header `…TOKENS  COST  TOK/S` (`runs.rs:~204`) and cost cell
(`runs.rs:254-255,278`, via `config.model_rates` + `metrics::token_cost`);
the `runs show <id>` detail block's `cost: {cost_str}` line
(`runs.rs:101-102,147,160`). Test
`format_runs_shows_id_tokens_cost_speed_columns` (`runs.rs:843`) constructs a
`ModelOverride` with `input_per_mtok: Some(2.0)`.

**`mcp/src/profile_cli.rs`:** imports `fmt_cost` (`:10`);
`format_phase_costs` (`:136`) prints header
`PHASE  MILESTONE  ATTEMPTS  VERDICT  TOKENS  COST` (`:142`) with
`token_cost` at `:145-150`. The `--cost` flag name on the `profile`
subcommand **stays** (it selects the per-phase cost-to-ship report, whose
currency is now tokens); only the dollar column goes.

**Scaffolds:** `mcp/src/init.rs:73-104` (config template with `$/MTok`
comment blocks) and `mcp/src/calibrate.rs:55-56,305-306` (writes
`input_per_mtok = 0.0` lines).

**Live config, this repo (`rexymcp.toml`):** the `[architect]` section
carries `model = "claude-opus-4-8"` ("cost-rate model … auto-fills the
$/MTok rates below") and two commented `*_per_mtok` lines; a whole
`[dashboard]` block (`saved_input_per_mtok = 5.0` /
`saved_output_per_mtok = 25.0` + known-model price comments) survives from
pre-M38 and has been **ignored by the parser since M38** — it is exactly the
"documented-but-ignored knob" the precedent forbids.

**Guard test:** `mcp/tests/readme_config_reference.rs` reflects
`ModelOverride` (and `GovernorConfig`) field names against `README.md`'s
sample TOML. Deleting the four rate fields fails that test until the
README's `[models."<id>"]` sample block is edited **in the same commit**.

## Spec

### 1. Delete the config rate layer

In `executor/src/config.rs`: delete `known_model_rates`,
`ArchitectModelRate`, the six `ArchitectConfig` pricing members (`model`,
four `*_per_mtok` fields, `rates`), the three rate methods, the four
`ModelOverride` rate fields, and `Config::model_rates`. Keep
`dispatch_model` / `review_model` and every non-rate `ModelOverride` field.
Update or delete the config tests that construct/assert these (e.g.
`config.rs:1671,2140,2173`); add the negative test
`legacy_rate_keys_are_ignored` (Test plan) pinning that a config file
containing `[architect] model`/`input_per_mtok` and
`[models."<id>"] output_per_mtok` and a `[dashboard]` block still loads.

### 2. Delete the telemetry/metrics dollar functions

- `executor/src/store/telemetry.rs`: delete `ArchitectRates`, the
  `ModelRates` alias, the three `CACHE_*_RATE_MULTIPLIER` constants, and
  `ArchitectLedger::cost` with its dollar-specific tests. **Grep first**:
  `grep -rn 'ModelRates\|ArchitectRates' executor/src mcp/src` — after tasks
  3–4 the only consumers should be the deleted code; if a live consumer
  remains, stop and file a blocker rather than half-deleting.
  `cache_creation_5m`/`cache_creation_1h` and every other token field are
  untouched. Fix the two `tier_telemetry` doc comments (drop "cost").
- `executor/src/store/metrics.rs`: delete `token_cost` + its tests.

### 3. `rexymcp runs`: COST → CACHE%

In `mcp/src/runs.rs`:

- Delete `fmt_cost`. Add the per-run cache-hit cell using the phase-01
  formula (prompt-side classes, dash when no cache activity). Factor one
  shared helper in `mcp/src/costs.rs` so `runs`, `profile`, and
  `ledger_lines`' `cache_cell` agree — e.g.:

  ```rust
  /// Prompt-side cache-hit ratio in percent. `None` when the scope/run has
  /// no cache activity (absence of instrumentation is not a measurement).
  pub(crate) fn cache_hit_pct(input: u64, cache_read: u64, cache_write: u64) -> Option<f64> {
      let denom = input + cache_read + cache_write;
      if cache_read + cache_write == 0 || denom == 0 {
          None
      } else {
          Some(cache_read as f64 / denom as f64 * 100.0)
      }
  }
  ```

  and rewrite `ledger_lines`' internal `cache_cell` on top of it (behavior
  identical — the phase-01/02 cache tests must pass unchanged).
- List table: header `…TOKENS  COST  TOK/S` → `…TOKENS  CACHE%  TOK/S`; the
  cell renders `{:.1}%` or `—` from the run's own `TokenBreakdown` (u32 →
  u64 widening before the sum, as everywhere).
- `runs show <id>` detail: the `cost: $X` line becomes
  `cache: {:.1}%` / `cache: —`.
- Retarget `format_runs_shows_id_tokens_cost_speed_columns` (drop the
  `ModelOverride` rate construction) and any other dollar assertions.

### 4. `rexymcp profile --cost`: token-native table

In `mcp/src/profile_cli.rs`: drop the `fmt_cost` import and `token_cost`
call; header becomes `PHASE  MILESTONE  ATTEMPTS  VERDICT  TOKENS  CACHE%`
with the cache cell from the shared helper over `PhaseCost.tokens`. The
`--cost` flag and its help text stay, reworded to describe the token
cost-to-ship report (no dollar wording). Update `main.rs`'s subcommand doc
comment for `profile` if it mentions dollars.

### 5. Scaffolds

- `mcp/src/init.rs`: remove the `$/MTok` rate lines/comments from the
  generated template ( `[architect]` keeps `dispatch_model`/`review_model`
  placeholders; the `[models."<id>"]` example keeps only surviving fields).
- `mcp/src/calibrate.rs`: remove the `input_per_mtok = 0.0`-style lines from
  the scaffold it writes (`calibrate.rs:55-56,305-306`).
- Update the scaffold-content tests these files carry.

### 6. Live config + README sample blocks (same commit as task 1)

- `rexymcp.toml` (this repo): delete the `[dashboard]` block entirely, the
  two commented `*_per_mtok` lines, and the `model = "claude-opus-4-8"` line;
  reword the `[architect]` section comment to "per-role delegation" (no
  "cost accounting"). `dispatch_model`/`review_model` lines stay as-is.
- `README.md`: edit **only** the sample-TOML blocks the guard test reflects —
  the `[models."<id>"]` block (drop the four rate lines) and the
  `[architect]` sample block (drop `model`/rate lines, keep the role models).
  The full prose sweep (release notes, rate tables, dashboard text) is
  phase-04 — do not start it.
- `mcp/tests/readme_config_reference.rs` must be green in the same commit.

### 7. Tests

Per the Test plan. The phase-01/02 cache tests
(`cache_row_*`, `cache_split_*`, alignment) must pass **unchanged** — they
pin that the shared-helper refactor in task 3 preserved behavior.

### 8. E2E capture

Run the E2E block from `## End-to-end verification` and paste the captured
files into the `### Update — <date> (end-to-end verification)` entry.

## Acceptance criteria

- [ ] `grep -rn 'per_mtok\|known_model_rates\|token_cost\|RATE_MULTIPLIER\|ArchitectRates\|ModelRates\|fmt_cost' executor/src mcp/src` returns nothing.
- [ ] `grep -n 'per_mtok\|\[dashboard\]\|claude-opus-4-8' rexymcp.toml` returns nothing.
- [ ] `rexymcp runs` renders a `CACHE%` column (no `COST`, no `$`); `rexymcp
  runs show <id>` renders a `cache:` line (no `cost:`).
- [ ] `rexymcp profile --cost` renders
  `PHASE  MILESTONE  ATTEMPTS  VERDICT  TOKENS  CACHE%` with no `$`.
- [ ] `rexymcp init` writes a template containing no `per_mtok` and no price
  comment.
- [ ] Test `legacy_rate_keys_are_ignored` passes (a config carrying the old
  keys still loads; the values are ignored).
- [ ] Tests `cache_row_shows_hit_ratio_when_cache_present`,
  `cache_split_alignment_matches_decimal_column`, and
  `ledger_tokens_dash_aligns_with_decimal_column` pass **unchanged**.
- [ ] `cargo test --test readme_config_reference` (or the equivalent filter)
  passes in the same tree as the config-field removals.
- [ ] All four gates green (separate invocations).

## Test plan

- `legacy_rate_keys_are_ignored` in `executor/src/config.rs` — a TOML
  fixture with `[architect] model = "claude-opus-4-8"`,
  `input_per_mtok = 5.0`, `[models."m"] output_per_mtok = 1.0`, and a
  `[dashboard]` block loads successfully; `dispatch_model` etc. still parse.
- `cache_hit_pct_none_when_no_activity` and
  `cache_hit_pct_prompt_side_ratio` in `mcp/src/costs.rs` — the shared
  helper: `(600k, 300k, 100k) → 30.0`, zero cache classes → `None`.
- `format_runs_shows_cache_pct_column` in `mcp/src/runs.rs` — retargeted
  from the cost-column test: a run with cache tokens renders `NN.N%`, a run
  without renders `—`, header says `CACHE%`.
- `runs_show_detail_has_cache_line_not_cost` in `mcp/src/runs.rs` — the
  detail block contains `cache:` and no `cost:`/`$`.
- `format_phase_costs_shows_cache_pct` in `mcp/src/profile_cli.rs` —
  header + cell assertions, no `$`.
- `init_template_has_no_rate_keys` in `mcp/src/init.rs` — the generated
  template contains no `per_mtok` substring (retarget the existing template
  tests).
- Existing phase-01/02 cache/alignment tests — unchanged, still green.

## End-to-end verification

The real artifacts are the CLI surfaces and the scaffolder. Run exactly:

```bash
mkdir -p target/e2e3
cargo run -q -p rexymcp -- runs --config rexymcp.toml > target/e2e3/runs.txt 2>&1
RUN_ID=$(cargo run -q -p rexymcp -- runs --config rexymcp.toml 2>/dev/null | awk 'NR==2{print $1}')
cargo run -q -p rexymcp -- runs show "$RUN_ID" --config rexymcp.toml > target/e2e3/run-detail.txt 2>&1
cargo run -q -p rexymcp -- profile --cost --config rexymcp.toml > target/e2e3/profile.txt 2>&1
cargo run -q -p rexymcp -- init --config target/e2e3/fresh.toml > target/e2e3/init.txt 2>&1
grep -c '\$' target/e2e3/runs.txt target/e2e3/run-detail.txt target/e2e3/profile.txt > target/e2e3/dollar-counts.txt || true
grep -c 'per_mtok' target/e2e3/fresh.toml > target/e2e3/template-rate-count.txt || true
```

(If the `runs show` id extraction does not match the actual list format,
select any real run id by hand — the pinned behavior is the `cache:` line.)
Paste all captured files. Expected: `runs.txt` header has `CACHE%`;
`run-detail.txt` has `cache:` and no `cost:`; `profile.txt` header ends
`TOKENS  CACHE%`; every count in `dollar-counts.txt` is `0`;
`template-rate-count.txt` is `0`.

## Authorizations

- May edit `rexymcp.toml` (this repo's live config): **only** the deletions
  in Spec task 6.
- May edit `README.md`: **only** the sample-TOML config blocks named in Spec
  task 6 (the guard test forces them into this phase; all other README
  content is phase-04).

No dependencies, no `Cargo.toml`, no contract docs, no schema changes.

## Out of scope

- **README prose** — release-notes bullets, the known-model rate table, the
  dashboard/Budget descriptions, CLI-reference rows, the ASCII dashboard
  mock — all phase-04. Touch only the two sample-TOML blocks.
- **`plugin/skills/auto/SKILL.md`** and the dashboard screenshot — phase-04.
- **Serialized telemetry schemas** — `PhaseRun`, `ArchitectLedger` fields
  (including 5m/1h), `ArchitectTokens`: untouched. Only the never-serialized
  `ArchitectRates`/`ModelRates` carriers are deleted.
- **`docs/architecture.md`** — not authorized; historical mentions of rates
  in §35/§38/§39 stay as history.
- The Levenshtein `cost` variables in `executor/src/parser/` — not money, do
  not touch.
- Do not rename the `costs` subcommand or the `profile --cost` flag.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-16 18:15 (progress)

Started phase-03. Flipped phase doc Status to `in-progress` and the milestone
README's phase-03 row to match. Implementing Spec tasks: removing the config
rate layer (`known_model_rates`, `ArchitectModelRate`, `ArchitectConfig`
pricing members + methods, `ModelOverride` rate fields, `Config::model_rates`),
the telemetry/metrics dollar functions (`ArchitectRates`/`ModelRates`,
`ArchitectLedger::cost`, `token_cost`, the three multipliers), and the two
remaining dollar-printing CLI surfaces (`runs`, `profile --cost`) now render
token/cache-hit columns via a shared `cache_hit_pct` helper in
`mcp/src/costs.rs`. Also updating the `init`/`calibrate` scaffolds, the live
`rexymcp.toml`, and the README sample-TOML blocks (guard test).


### Update — 2026-08-16 18:15 (end-to-end verification)

Captured mechanically via the phase's E2E block (adjusted: `runs show <id>`
reads `--config <CONFIG> show <ID>` argument order, and `init` writes into a
target/scratch dir because `init` has no `--config` flag). All captures in
`target/e2e3/*`.

**`target/e2e3/runs.txt`** (positive control: real telemetry — rows show live
`CACHE%` values like `98.1%` and `—` when no cache activity):

```
ID        AGE     MODEL  TAGS           SETTINGS     GATES  TURNS  STATUS    VERDICT  SERVED_MODEL  TRUNC  CXT_WIN  PEAK_CXT  RECLAIMED  TOKENS  CACHE%  TOK/S
cb29dad2  45m19s  deepseek-v4-flash-0731 language=rust,kind=feature,size=m default      ✓✓✓✓  336    complete  approved_first_try deepseek-v4-flash-0731 0%      1024k   19%        131.8k  28.1M     98.1%  48
16b752b1  1h      deepseek-v4-flash-0731 language=rust,kind=refactor,size=l default      ✓✓✓✓  237    complete  approved_first_try deepseek-v4-flash-0731 1%      1024k   23%        161.2k  26.9M     96.7%  45
a8a0dc95  2d      Qwen/Qwen3.8-27B-FP8 language=rust,kind=refactor,size=s default      ✓✓✓✓  93     complete  —           Qwen/Qwen3.8-27B-FP8 0%      256k    36%        21.6k   2.7M      —      12
```

**`target/e2e3/run-detail.txt`** (run cb29dad2 — `cache:` line present, no `cost:`):

```
id: cb29dad2
model: deepseek-v4-flash-0731
...
tokens: input=523216 output=99755 cache_read=27432704 cache_write=0 total=28055675
cache: 98.1%
tok/s: 48
```

**`target/e2e3/profile.txt`** header (header ends `TOKENS  CACHE%`, cache cells render):

```
PHASE  MILESTONE  ATTEMPTS  VERDICT  TOKENS  CACHE%
phase-01-scaffold                        M1-foundations        2  approved_first_try          1.2M         —
phase-01-read-key-test-bound             M10-residual-hygiene        1  approved_first_try          1.3M     90.1%
```

**`target/e2e3/dollar-counts.txt`** (positive control: every file measured `0`):

```
target/e2e3/runs.txt:0
target/e2e3/run-detail.txt:0
target/e2e3/profile.txt:0
```

**`target/e2e3/template-rate-count.txt`** (`rexymcp init` output has `0` rate keys):

```
0
```

**Live-artifact greps:** `grep -rn 'per_mtok\|known_model_rates\|token_cost\|RATE_MULTIPLIER\|ArchitectRates\|ModelRates\|fmt_cost' executor/src mcp/src` returns nothing in production paths (only API-constant occurrences inside test fixtures and the new negative-guard tests remain); `grep -n 'per_mtok\|\[dashboard\]\|claude-opus-4-8' rexymcp.toml` returns nothing.
