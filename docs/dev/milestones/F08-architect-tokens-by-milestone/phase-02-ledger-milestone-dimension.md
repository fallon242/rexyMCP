# Phase 2: ledger milestone dimension

**Milestone:** F08 — Architect tokens by milestone
**Status:** todo
**Depends on:** phase-01 (done)
**Estimated diff:** ~350 lines, about two-thirds tests
**Tags:** language=rust, kind=feature, size=m

## Goal

`rexymcp harvest` records which milestone each architect (Claude) message was
working on, and milestone-scoped costs include those tokens. Today the
milestone column of the Architect row is always zero, because the ledger has
no milestone field.

Attribution comes from the transcript itself. A session's **current
milestone** is the last `milestones/<slug>` path it named in a tool call.
Messages before the first mention stay unattributed (`None`). The architect
measured this rule on this repo's 13 transcripts (968 messages): 2.5% of
tokens stay unattributed, 4 tool inputs name two milestones and are skipped.

## Pre-flight

Measured 2026-09-18:

1. `cargo test -p rexymcp harvest::` → **19 passed**.
2. `cargo test -p rexymcp costs::` → **33 passed**.
3. `cargo test -p rexymcp-executor ledger` → **4 passed**.
4. Full `cargo test` → **720 / 2 / 1206 passed**.

## Current state

**Transcript lines.** Claude Code writes one assistant message as several
lines sharing one `message.id`: typically a `thinking` line, then a `tool_use`
line, each repeating the same `usage`. The harvester counts usage on the first
line and treats later lines as duplicates (`duplicates += 1; continue;`). A
tool call looks like this (one line, shortened):

```json
{"type":"assistant","timestamp":"2026-09-18T15:00:00.000Z","message":{"id":"msg_b","role":"assistant","model":"claude-opus-5","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/repo/docs/dev/milestones/F07-completion-entry-date/README.md"}}],"usage":{"input_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":1}}}
```

**The harvest loop** (`mcp/src/harvest.rs:252-300`): per transcript file, per
line: skip non-`assistant` lines, `extract_usage` (skip on `None`), dedup by
`message.id` (skip duplicates), then add into
`accum: HashMap<(String, String, String), Accum>` keyed
`(session_id, model, skill)`.

**`ArchitectLedger`** (`executor/src/store/telemetry.rs:567-599`) has no
milestone field. `fold_ledger` (`telemetry.rs:605-624`) keeps the last record
per `(project_id, session_id, model, skill)`.

**Milestone scope** (`mcp/src/costs.rs:142-157`) returns zero architect tokens
whenever `milestone_id.is_some()`. The dashboard's milestone column calls the
same `scope_costs`, so fixing it here fixes both views.

## Spec

### 1. Ledger field and fold key — `executor/src/store/telemetry.rs`

Add after `skill` in `ArchitectLedger`:

```rust
    /// Milestone directory slug these tokens were attributed to: the last
    /// `milestones/<slug>` path the session named in a tool call. `None`
    /// before the session named one, and on records written before this
    /// field existed.
    #[serde(default)]
    pub milestone_id: Option<String>,
```

In `fold_ledger`, add `l.milestone_id.clone()` as a fifth key element. Declare
the key type as an alias inside the function so the `HashMap` type stays
short (clippy `type_complexity` is denied):

```rust
    type Key = (Option<String>, String, String, String, Option<String>);
    let mut latest: HashMap<Key, usize> = HashMap::new();
```

Update the doc comments on `ArchitectLedger` and `fold_ledger` that name the
key to include the milestone.

### 2. Slug extraction — `mcp/src/harvest.rs`

Add these two private functions exactly as written. The architect has already
compiled them with clippy clean and run them against the cases in the test
plan:

```rust
/// Milestone directory slugs named in `text` after `milestones/`. A slug is
/// the run of `[A-Za-z0-9_-]` that follows, and counts only if it is an
/// uppercase letter, one or more digits, `-`, then at least one more character.
fn milestone_slugs(text: &str) -> BTreeSet<String> {
    text.match_indices("milestones/")
        .filter_map(|(i, m)| {
            let rest = &text[i + m.len()..];
            let end = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                .unwrap_or(rest.len());
            let slug = &rest[..end];
            is_milestone_slug(slug).then(|| slug.to_string())
        })
        .collect()
}

fn is_milestone_slug(slug: &str) -> bool {
    let Some(rest) = slug.strip_prefix(|c: char| c.is_ascii_uppercase()) else {
        return false;
    };
    let after_digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    after_digits.len() < rest.len()
        && after_digits.strip_prefix('-').is_some_and(|tail| !tail.is_empty())
}

/// Move `current` to the milestone named by this line's tool calls. Each
/// `tool_use` input that names exactly one milestone moves it; an input that
/// names none, or several (a grep across milestones), leaves it alone.
fn update_current_milestone(v: &serde_json::Value, current: &mut Option<String>) {
    let Some(content) = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    else {
        return;
    };
    for block in content {
        if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
            continue;
        }
        let Some(input) = block.get("input") else {
            continue;
        };
        let mut slugs = milestone_slugs(&input.to_string()).into_iter();
        if let (Some(slug), None) = (slugs.next(), slugs.next()) {
            *current = Some(slug);
        }
    }
}
```

Import `std::collections::BTreeSet`. **Do not** reuse `milestone_id_from_path`
from `runner.rs`: it takes a file path, and it accepts `M37` with no `-`, which
here would turn a glob like `milestones/M37*` into a truncated id.

### 3. Attribute in the harvest loop — `mcp/src/harvest.rs`

- Declare `let mut current_milestone: Option<String> = None;` **inside** the
  per-file loop, so state resets for every transcript file (a subagent
  transcript starts fresh).
- Call `update_current_milestone(&v, &mut current_milestone);` right after the
  "skip non-assistant lines" check, **before** `extract_usage` and the dedup
  check. The `tool_use` line is normally a duplicate id, so calling it after
  the dedup `continue` would never see it.
- Add the milestone as a fourth accumulator key element, using aliases to
  keep clippy's `type_complexity` quiet:

```rust
/// `(session_id, model, skill, milestone_id)`.
type BucketKey = (String, String, String, Option<String>);
/// `(project_id, session_id, model, skill, milestone_id)` — the `fold_ledger` key.
type LedgerKey = (Option<String>, String, String, String, Option<String>);
```

  `accum` becomes `HashMap<BucketKey, Accum>` and its key is
  `(session_id.clone(), model, skill, current_milestone.clone())`. The
  `existing` map becomes `HashMap<LedgerKey, ArchitectLedger>`; add
  `l.milestone_id.clone()` / `ledger.milestone_id.clone()` to both key tuples,
  and set `milestone_id: key.3` in the `ArchitectLedger` literal.
- Add `#[derive(Default)]` to `Accum`.

### 4. Legacy double-count guard — `mcp/src/harvest.rs`

Stores written before this phase hold one record per `(session, model,
skill)` with the session's **full** sum and no milestone, so it folds under
the `None` key. After this phase, a re-harvest of that session writes
attributed buckets. If it wrote no `None` bucket, the legacy record would
survive the fold and every token would count twice at project scope.

After the per-file loop, before building records, insert:

```rust
    // A pre-attribution record has `milestone_id: None` and the session's full
    // sum. Emit a `None` bucket for every (session, model, skill) seen — zero
    // when every message was attributed — so it replaces that record at fold
    // time instead of adding to the attributed buckets.
    let seen: Vec<(String, String, String)> = accum
        .keys()
        .map(|(s, m, k, _)| (s.clone(), m.clone(), k.clone()))
        .collect();
    for (s, m, k) in seen {
        accum.entry((s, m, k, None)).or_default();
    }
```

### 5. Milestone scope — `mcp/src/costs.rs`

Replace the architect block at `costs.rs:142-157` with:

```rust
    // Architect: ledger records carry the milestone their tokens were
    // attributed to. Project scope (`None`) sums every record.
    let mut architect_tokens = ArchitectTokens::default();
    for l in ledgers.iter().filter(|l| {
        l.project_id.as_deref() == Some(project_id)
            && (milestone_id.is_none() || l.milestone_id.as_deref() == milestone_id)
    }) {
        architect_tokens.input = architect_tokens.input.saturating_add(l.tokens.input);
        architect_tokens.cache_creation = architect_tokens
            .cache_creation
            .saturating_add(l.tokens.cache_creation);
        architect_tokens.cache_read = architect_tokens
            .cache_read
            .saturating_add(l.tokens.cache_read);
        architect_tokens.output = architect_tokens.output.saturating_add(l.tokens.output);
    }
```

### 6. Every `ArchitectLedger` literal gains the field

Exactly **7** existing construction sites. Add `milestone_id: None,` to each:

- `executor/src/store/telemetry.rs` tests — 5 literals (near lines 1670, 1719,
  1753, 1770, 1787).
- `mcp/src/costs.rs` — the `ledger()` test helper (near line 828).
- `mcp/src/harvest.rs` — the one in `harvest()` (gets `key.3`, per §3).

Run `grep -rn "ArchitectLedger {" executor/src mcp/src` after editing to
confirm nothing was missed.

**Must NOT:**

- Add a dependency or a regex. No `Cargo.toml` edits.
- Change `extract_usage`, dedup, or `duplicates` counting.
- Count usage twice for a message that spans lines.
- Change `HarvestOutcome`'s fields. `records` may now include zero-valued
  `None` buckets; that is expected.
- Touch `cache_split_lines` or the architect 5m/1h split (it stays project-scope).
- Touch `milestone_id_from_path` in `runner.rs`.

## Acceptance criteria

- [ ] A harvested message is stored under the milestone last named by a
      `tool_use` input in the same transcript file, or `None` before any.
- [ ] A `tool_use` input naming two milestones leaves the current one unchanged.
- [ ] Re-harvesting over a legacy (no-milestone) record does not double-count.
- [ ] `scope_costs(…, Some(id))` sums only ledger records with that milestone;
      `scope_costs(…, None)` sums them all.
- [ ] A ledger JSON line without `milestone_id` deserializes with `None`.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

**Write `harvest_attributes_messages_to_last_named_milestone` first, run it,
and quote the compile error or failure in the Update Log before implementing.**

`executor/src/store/telemetry.rs` tests:

1. `fold_ledger_keeps_milestones_apart` — two records identical except
   `milestone_id` (`Some("F07-a")` vs `None`) → `fold_ledger` returns 2.
2. `ledger_without_milestone_field_reads_as_none` —
   `serde_json::from_str::<ArchitectLedger>` on a literal JSON object with no
   `milestone_id` key → `milestone_id == None`.

`mcp/src/harvest.rs` tests (reuse `make_config`, `write_fixture`,
`HarvestArgs`, `read_architect_ledger`, `fold_ledger`; build tool-call lines
in the shape shown under Current state):

3. `milestone_slugs_extracts_milestone_ids` — each gives exactly the one slug:
   - `{"file_path":"/home/x/docs/dev/milestones/F07-completion-entry-date/README.md"}` → `F07-completion-entry-date`
   - `D=docs/dev/milestones/F05-privacy-security-hardening && ls $D` → `F05-privacy-security-hardening`
   - `docs/dev/milestones/M46-token-first-accounting/phase-01.md` → `M46-token-first-accounting`
4. `milestone_slugs_rejects_non_ids` — each gives an empty set:
   `ls docs/dev/milestones/`, `ls docs/dev/milestones/F05*`, `milestones/M37`,
   `milestones/README.md`, `milestones/f07-lower/x`, `milestones/-F07-x`,
   `milestones/F-x`, `milestones/F07-`, `milestones/F07_x`.
5. `harvest_attributes_messages_to_last_named_milestone` — one file, three
   single-line messages with distinct ids: A (no `content`), B (a `tool_use`
   naming `F07-completion-entry-date`), C (no `content`). After folding:
   `None` bucket has `messages == 1` (A); the `F07-completion-entry-date`
   bucket has `messages == 2` (B and C).
6. `harvest_ignores_tool_input_naming_two_milestones` — B names `F07-a`
   (e.g. `milestones/F07-a/x.md`), then C's single `tool_use` input names both
   `milestones/F05-b/x` and `milestones/F06-c/x`, then D (no content). C and D
   land in `F07-a`.
7. `harvest_resets_milestone_per_transcript_file` — `s1.jsonl` names `F07-a`;
   `s2.jsonl` has one message with no `content`. The `s2` record is `None`.
8. `harvest_emits_zero_none_bucket_when_all_messages_attributed` — one
   message, which names `F07-a`. Folded records for that session: one with
   `milestone_id == None`, `messages == 0`, all four token classes 0; one with
   `F07-a` and `messages == 1`.
9. `reharvest_replaces_legacy_record_without_double_count` — first
   `append_architect_ledger` a legacy record for `project_id:
   Some("test-project")` (the id `make_config` writes), the same session /
   model / skill as the fixture, `milestone_id: None`, `messages: 1`,
   `tokens.input: 1000`. Then harvest a fixture whose single message
   (`input_tokens: 1000`) names `F07-a`. Sum `tokens.input` across the folded
   records for that session → **1000**, not 2000.

`mcp/src/costs.rs` tests:

10. Replace `scope_costs_milestone_architect_is_zero` with
    `scope_costs_milestone_counts_only_matching_ledgers` — three `ledger(…)`
    records with `milestone_id` `Some("F07-a")`, `Some("F08-b")` and `None`
    (each 1 000 000 input). `Some("F07-a")` → architect input **1 000 000**;
    `None` → **3 000 000**.

**Finish condition:**

- `cargo test -p rexymcp harvest::` → **26 passed**
- `cargo test -p rexymcp costs::` → **33 passed**
- `cargo test -p rexymcp-executor ledger` → **6 passed**
- Full `cargo test` → **727 / 2 / 1208 passed**

## End-to-end verification

Run `cargo test -p rexymcp harvest:: -- --nocapture` and
`cargo test -p rexymcp-executor ledger -- --nocapture`. Paste both outputs in
an `(end-to-end verification)` entry. The architect runs the real-transcript
harvest at review.

## Authorizations

- New field `milestone_id` on `ArchitectLedger` (store-compatible via `#[serde(default)]`).
- `#[derive(Default)]` on `Accum`.
- Replacing the test `scope_costs_milestone_architect_is_zero`.

## Out of scope

- The stale "no session or milestone dimension" wording on `cache_split_lines`.
- Backfilling `milestone_id` on stored phase runs.
- Any change to the executor contract.

## Update Log

<!-- entries appended below this line -->
