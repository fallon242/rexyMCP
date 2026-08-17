# Phase 04: Docs sweep

**Milestone:** M46 — Token-First Accounting
**Status:** todo
**Depends on:** phase-03
**Estimated diff:** ~120 lines (docs only — README + two plugin skill docs; zero code)
**Tags:** language=markdown, kind=docs, size=s

## Goal

Finish the milestone by making the prose match the product: the README and
the plugin skill docs stop describing dollar accounting, rate config, the
`--tokens` flag, and the removed `[architect] model` field, and start
describing the token-first surfaces phases 01–03 shipped. Zero code changes.

## Architecture references

Read before starting:

- `docs/architecture.md` § Status #46 — what shipped in phases 01–03; the
  prose below must describe exactly that, no more.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

Line numbers below are from the current `README.md` (post-phase-03; verify
with grep before editing — they may drift a line or two):

- `README.md:27-38` — the release-notes bullet "**Improved cost
  accounting.**" describing per-model `$/Mtok` rates, the savings framing,
  `--tokens`, and dollar/token column alignment.
- `README.md:120` — "real token/cost accounting" in the auto-loop blurb.
- `README.md:287` — a sample `[architect]` block line
  `model = "claude-opus-4-8"   # drafting + escalation judgment (and cost rates)` —
  the `model` field no longer exists in `ArchitectConfig` (phase-03).
- `README.md:307-315` — harvest prose: "joins **real per-class token/cost**
  … architect cost is *harvested, never estimated*".
- `README.md:353-358` — dashboard Budget/Spend panel description: "a
  **Spend** block pricing … Press `b` to toggle between dollar …".
- `README.md:617,621,624` — CLI-reference table rows for `dashboard`
  ("drives cost rates"), `costs` ("Architect / Executor / Net … `--tokens`
  for raw token counts instead of dollar values"), `harvest`
  ("per-class token/cost").
- `README.md:713-716` — the ASCII dashboard mock showing a `Spend` block
  with `($0.00)` / `$0.50` / `Net:` rows.
- `README.md:731-735` — the Budget panel bullet, again "Spend block" +
  dollar/token `b` toggle.
- `README.md:798` — "harvests its real token/cost".
- `plugin/skills/auto/SKILL.md:86` — "Do **not** substitute `[architect]
  model` — that field is the cost-rate model, a separate concern." The
  field was deleted in phase-03; the sentence now points at nothing.
- `plugin/skills/auto/SKILL.md:189-192` — journaling rationale "feeds
  per-activity token/cost accounting … so cost uses that role model's
  rates".
- `plugin/skills/auto/SKILL.md:223-232` — harvest step: "so the report's
  cost totals are real, not estimated … report token/cost as **absent**".
- `plugin/skills/auto/SKILL.md:247` — the loop-report template line
  `- **Token / cost:** <harvested totals, …>`.
- `plugin/skills/escalate/SKILL.md:149` — one cost mention (read it; fix in
  the same spirit).
- `docs/rexymcp_dashboard.png` — the screenshot still shows the dollar-era
  Spend block. **Regenerating it is a human action** (needs a live terminal
  capture) — it is explicitly NOT your task; see Out of scope.

What the product actually does now (describe this, nothing else): `rexymcp
costs` renders a token ledger — `Architect:` / `Executor:` / `Cache:` (the
Cache row is the executor's prompt-side cache-hit ratio) — across Session /
Milestone / Project, plus a token-share by-skill table; `--json` emits
token-only keys; the dashboard Budget panel shows the same ledger and `b`
cycles **Totals ⇄ Cache split** (the split view shows executor Read/Write
per scope and the architect ledger's 5m/1h cache-creation split,
project-scoped); `runs` and `profile --cost` show `CACHE%` columns; config
has no rate keys (leftover `*_per_mtok` / `[dashboard]` keys are ignored);
the architect ledger harvests token counts only.

## Spec

### 1. README release-notes bullet

Replace the entire "**Improved cost accounting.**" bullet
(`README.md:27-38`) with exactly:

```markdown
- **Token-first accounting (M46).** Dollar cost accounting is gone; token
  counts are the accounting currency. The dashboard's Budget panel and the
  **`rexymcp costs`** CLI render a token ledger — **Architect / Executor /
  Cache** across **Session / Milestone / Project** — where the Cache row is
  the executor's prompt-side cache-hit ratio, plus a token-share by-skill
  table. Press `b` in the dashboard to cycle **Totals ⇄ Cache split** (the
  split view surfaces executor cache read/write per scope and the architect
  ledger's 5m/1h cache-creation split). `rexymcp runs` and `rexymcp profile
  --cost` show a `CACHE%` column. These changes **break** three things
  loudly: `costs --json` now emits token-only keys (`saved`/`executor`/
  `architect`/`net` are gone), the `costs --tokens` flag is removed (tokens
  are the only mode), and all `$/Mtok` rate config keys (`[architect]`
  rates, `[models."<id>"]` rates, `[dashboard]`) are deleted — leftover keys
  in existing config files are silently ignored.
```

### 2. README prose sweep

- `:120` — "real token/cost accounting" → "real token accounting".
- `:287` — replace the `model = …` sample line with nothing (delete it) —
  the surrounding sample keeps `dispatch_model` / `review_model`; adjust
  the comment on those lines only if they mention rates.
- `:307-315` — harvest prose → "joins **real per-class token counts** onto
  each activity … architect tokens are *harvested, never estimated*", and
  the loop-report sentence → "token totals, why it stopped, and what needs
  you".
- `:353-358` and `:731-735` — Budget panel: rename "Spend block" wording to
  "token ledger" and describe `b` as cycling Totals ⇄ Cache split (no
  dollar mention). Keep the panel name **Budget**.
- `:617` — drop "(drives cost rates)" from the `dashboard` row's `--config`
  note (say "`--config <path>`" plain).
- `:621` — `costs` row: "Report the token ledger — **Architect / Executor /
  Cache** across **Session / Milestone / Project** — for a repo's session
  log + project telemetry. The scriptable, one-shot equivalent of the
  dashboard's Budget ledger." Options list drops `--tokens`.
- `:624` — `harvest` row: "join **real** per-class token counts onto
  journal activities … (fills the architect token rows; **harvested, never
  estimated**)".
- `:798` — "harvests its real token/cost" → "harvests its real token
  usage".

### 3. README ASCII mock

Replace the `Spend` block lines in the mock (`README.md:713-716`) with the
real current shape (three data rows, same box widths — pad with spaces to
keep the box borders aligned):

```
│ Model: Qwen/Qwen3.6-27B │ Tokens    Session  Milestone … │ Filter: 18 calls … │
│ State: running          │   Architect:    —       —    …  │ Evict: 6 reads …   │
│ Duration: 4m12s         │   Executor:  90.2k   733.9k …  │ Dedupe: 3 reads …  │
│ Turn 42, stage verify   │   Cache:        —     96.7% …  │                    │
```

(The pinned behavior is the row labels `Tokens`/`Architect:`/`Executor:`/
`Cache:` and the absence of `$` — exact cell values in the mock are
illustrative; keep the box drawing intact.)

### 4. `plugin/skills/auto/SKILL.md`

- `:86` — replace the sentence "Do **not** substitute `[architect] model` —
  that field is the cost-rate model, a separate concern." with: "There is
  no fallback field to substitute — the role keys are the only architect
  model configuration."
- `:189-192` — "feeds per-activity token/cost accounting" → "feeds
  per-activity token accounting"; drop the trailing rationale "so cost uses
  that role model's rates" and keep the instruction to pass `--model` = the
  model that actually performed the step (attribution, not pricing).
- `:223-232` — "so the report's cost totals are real" → "so the report's
  token totals are real"; "report token/cost as **absent**" → "report
  tokens as **absent**".
- `:247` — the template line becomes:
  `- **Tokens:** <harvested totals, or "absent — <client> provides no transcript usage">`

### 5. `plugin/skills/escalate/SKILL.md`

Read `:149` and rewrite its cost mention in the same token-first spirit
(mechanical wording fix only — do not restructure the skill).

### 6. Sweep verification

Run the acceptance greps below; fix any residual hit that describes current
behavior in dollars. Mentions of dollars **as the thing that was removed**
(the release-notes bullet you wrote in task 1, milestone history) are
correct and stay.

## Acceptance criteria

- [ ] `grep -nE '\$[0-9]|/Mtok|per_mtok|saved_(input|output)' README.md plugin/skills/auto/SKILL.md plugin/skills/escalate/SKILL.md` returns nothing.
- [ ] `grep -n 'tokens for raw\|--tokens\|Spend block\|cost rates\|token/cost' README.md` returns nothing.
- [ ] `grep -n 'Token / cost\|cost-rate model\|\[architect\] model' plugin/skills/auto/SKILL.md` returns nothing.
- [ ] `grep -c 'Cache' README.md` is ≥ 3 (the ledger row, the `b` cycler
  description, and the CLI-reference row all describe the Cache surfaces).
- [ ] The ASCII mock block contains `Tokens`, `Architect:`, `Executor:`,
  `Cache:` and no `$`.
- [ ] `cargo test --test readme_config_reference` passes (the sample-TOML
  blocks are untouched by this phase, but the guard must stay green).
- [ ] All four gates green (docs-only diff — they must pass trivially; a
  failure means you touched code, which this phase forbids).

## Test plan

No new tests — this phase ships no code. The pinned checks are the
acceptance greps above plus the existing `readme_config_reference` guard.

## End-to-end verification

The real artifacts are the checked-in docs themselves. Run exactly:

```bash
mkdir -p target/e2e4
grep -nE '\$[0-9]|/Mtok|per_mtok|saved_(input|output)' README.md plugin/skills/auto/SKILL.md plugin/skills/escalate/SKILL.md > target/e2e4/dollar-grep.txt; echo "exit=$?" >> target/e2e4/dollar-grep.txt
grep -n 'tokens for raw\|--tokens\|Spend block\|cost rates\|token/cost' README.md > target/e2e4/readme-stale.txt; echo "exit=$?" >> target/e2e4/readme-stale.txt
grep -n 'Token / cost\|cost-rate model\|\[architect\] model' plugin/skills/auto/SKILL.md > target/e2e4/skill-stale.txt; echo "exit=$?" >> target/e2e4/skill-stale.txt
sed -n '/Tokens    Session/,+3p' README.md > target/e2e4/mock.txt
cargo test --test readme_config_reference 2>&1 | tail -3 > target/e2e4/guard.txt
```

Paste all captured files. Expected: the three grep files contain only
`exit=1` (no matches); `mock.txt` shows the four mock rows with no `$`;
`guard.txt` shows 2 passed.

## Authorizations

- May edit `README.md` (full prose sweep — this phase owns it).
- May edit `plugin/skills/auto/SKILL.md` and
  `plugin/skills/escalate/SKILL.md`: **only** the wording changes in Spec
  tasks 4–5. No structural or procedural changes to any skill.

No code, no `Cargo.toml`, no config, no contract docs
(STANDARDS/WORKFLOW/architecture.md remain untouchable).

## Out of scope

- **`docs/rexymcp_dashboard.png`** — regenerating the screenshot needs a
  live terminal capture; it is a **human follow-up action**, recorded in
  the milestone README at close. Do not attempt to edit, delete, or
  regenerate the image, and do not remove its reference from the README.
- **Any `.rs` file.** A docs phase with a code diff is an automatic bounce.
- **`docs/architecture.md`** §35/§38/§39 history — dollar mentions there
  are historical record, not current-behavior claims; leave them.
- **Skill procedure changes** — the auto/escalate skills' logic, steps, and
  stop conditions are untouched; only the pinned wording changes.
- The phase docs and bug docs of earlier milestones — historical, leave.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
