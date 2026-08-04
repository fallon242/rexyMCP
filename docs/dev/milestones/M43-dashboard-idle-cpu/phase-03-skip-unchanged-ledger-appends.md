# Phase 03: skip unchanged ledger appends

**Milestone:** M43 — Dashboard Idle CPU
**Status:** todo
**Depends on:** phase-02 (done — `telemetry::read_all`, which this phase uses to
read the current ledger state in one pass)
**Estimated diff:** ~180 lines
**Tags:** language=rust, kind=bugfix, size=m

## Goal

Stop `rexymcp serve` appending ~143 identical ledger records to
`phase_runs.jsonl` every 60 seconds. Harvest re-derives every bucket from the
whole transcript corpus and appends all of them regardless of whether anything
changed; `fold_ledger` then throws away all but the newest per key at read time.
Append only the buckets that actually differ from what the store already holds.

## Architecture references

Read before starting:

- `docs/dev/milestones/M43-dashboard-idle-cpu/README.md` § "The three multiplied
  factors", factor 3 — the write amplification this phase fixes.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture reference above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

`harvest()` (`mcp/src/harvest.rs:214`) builds one `ArchitectLedger` per
`(session_id, model, skill)` accumulator and appends **every** one
(`mcp/src/harvest.rs:307–332`):

```rust
    // Build ledger records from accumulators, sorted for deterministic output
    let mut total_messages = 0usize;
    let mut total_records = 0usize;
    for (key, acc) in accum {
        let ledger = ArchitectLedger {
            record: ARCHITECT_LEDGER_RECORD_TAG.to_string(),
            project_id: _project_id.clone(),
            session_id: key.0,
            model: key.1,
            skill: key.2,
            tokens: ArchitectTokens { /* … */ },
            cache_creation_5m: acc.cache_creation_5m,
            cache_creation_1h: acc.cache_creation_1h,
            messages: acc.messages,
            last_ts: acc.last_ts,
        };
        if let Err(e) = append_architect_ledger(&telemetry_dir, &ledger) {
            eprintln!("warning: failed to append ledger record: {}", e);
        }
        total_messages += acc.messages as usize;
        total_records += 1;
    }
```

Nothing consults the existing store first. The sweep inside `serve`
(`mcp/src/sweep.rs:142`) calls this every 60 s whenever any transcript's mtime
moved — which is continuously while the architect works — so one edited session
re-appends all ~143 buckets. That is how the store reached 103 MB / 278,836 lines
of which 278,226 are ledger records folding down to ~143.

`fold_ledger` (`executor/src/store/telemetry.rs:666`) defines the identity that
matters — **last write wins on a four-part key**:

```rust
pub fn fold_ledger(ledgers: Vec<ArchitectLedger>) -> Vec<ArchitectLedger> {
    use std::collections::HashMap;
    let mut latest: HashMap<(Option<String>, String, String, String), usize> = HashMap::new();
    let mut out: Vec<ArchitectLedger> = Vec::new();
    for l in ledgers {
        let key = (
            l.project_id.clone(),
            l.session_id.clone(),
            l.model.clone(),
            l.skill.clone(),
        );
        // …replace in place if the key was seen, else push…
    }
    out
}
```

`ArchitectLedger` derives `PartialEq` (`executor/src/store/telemetry.rs:603`), so
"has this bucket changed?" is a plain `==`. `schema_version` is **not** a struct
field — it is injected at the write boundary (`:698`) — so it does not
participate in the comparison, which is what you want.

`HarvestOutcome` (`mcp/src/harvest.rs:27`):

```rust
pub struct HarvestOutcome {
    pub path: PathBuf,
    pub messages: usize,
    pub duplicates: usize,
    pub sessions: usize,
    pub records: usize,
}
```

## Spec

### 1. Read the current ledger state once, before the append loop

In `harvest()` (`mcp/src/harvest.rs`), immediately before the
`for (key, acc) in accum` loop, read what the store already holds and index it by
`fold_ledger`'s key:

```rust
    // What the store already holds, folded to one record per key. Appending a
    // record identical to the folded state is pure write amplification: the
    // reader would discard it immediately.
    let existing: std::collections::HashMap<
        (Option<String>, String, String, String),
        ArchitectLedger,
    > = fold_ledger(
        telemetry::read_all(&store_path)
            .map(|s| s.ledgers)
            .unwrap_or_default(),
    )
    .into_iter()
    .map(|l| {
        (
            (
                l.project_id.clone(),
                l.session_id.clone(),
                l.model.clone(),
                l.skill.clone(),
            ),
            l,
        )
    })
    .collect();
```

Use `telemetry::read_all` (added in phase 02) rather than
`read_architect_ledger` — one pass, no `serde_json::Value` round-trip. Read from
**`store_path`**, the exact path the appends target, so a `--telemetry-path`
override is honored on both sides.

A read error must **not** abort the harvest: `.unwrap_or_default()` yields an
empty map, and an empty map means "nothing matches, append everything" — the
current behavior. Degrading to today's behavior on an unreadable store is the
correct failure direction.

### 2. Skip appends whose record is unchanged

Inside the loop, after building `ledger`, compare against the folded state and
append only on a difference:

```rust
        let key = (
            ledger.project_id.clone(),
            ledger.session_id.clone(),
            ledger.model.clone(),
            ledger.skill.clone(),
        );
        if existing.get(&key) == Some(&ledger) {
            total_messages += acc.messages as usize;
            total_unchanged += 1;
            continue;
        }
        if let Err(e) = append_architect_ledger(&telemetry_dir, &ledger) {
            eprintln!("warning: failed to append ledger record: {}", e);
        }
        total_messages += acc.messages as usize;
        total_records += 1;
```

Three things this must get right:

- **Compare the whole record, not just key presence.** `existing.contains_key(&key)`
  would skip a bucket whose token totals grew — silently losing every update after
  the first. The comparison is `== Some(&ledger)`.
- **`total_messages` stays unconditional.** It counts messages *processed*, not
  records written. `harvest_is_idempotent` (`mcp/src/harvest.rs:578`) asserts
  `outcome2.messages == 1` on a second run over unchanged fixtures; that test must
  pass **unmodified**. If you find yourself editing it, stop and file a blocker.
- **`record` must be set on the candidate before comparing.** The store's records
  deserialize with `record == "architect_ledger"`; the candidate sets the same at
  `harvest.rs:311`. If the candidate had `record: String::new()`, every comparison
  would fail and nothing would ever be skipped — a silent no-op fix. Pin this with
  the test below.

### 3. Report what was skipped

Add a field to `HarvestOutcome` (additive — `harvest()` is its only construction
site):

```rust
    /// Buckets whose record was byte-identical to the store's folded state and
    /// therefore not appended.
    pub unchanged: usize,
```

Then surface it in the two places that report a harvest:

- `mcp/src/sweep.rs:144` — the liveness marker written to `sweep_state.json`:
  ```rust
  let outcome = format!(
      "{} new / {} unchanged / {} msgs",
      o.records, o.unchanged, o.messages
  );
  ```
  No test asserts this string (only the `"no change"` and
  `"skipped: no transcript dir"` outcomes are pinned, at `sweep.rs:338` and
  `:351`), so extending it is safe.
- `mcp/src/main.rs:1084` — the `rexymcp harvest` CLI printout. Add the unchanged
  count to the existing line; keep the existing fields.

## Acceptance criteria

- [ ] `cargo build` succeeds.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff. (Fix with `rustfmt <file>` on
      touched files only — never `cargo fmt --all`.)
- [ ] `cargo test` passes, with `harvest_is_idempotent` **unmodified**.
- [ ] A second consecutive `rexymcp harvest` over an unchanged transcript corpus
      appends **zero** bytes — verified end-to-end below.

## Test plan

In `mcp/src/harvest.rs`'s existing `#[cfg(test)] mod tests` (reuse
`make_config` / `write_fixture` / the `TempDir` style already there):

- `harvest_skips_appending_unchanged_records` — harvest twice over one unchanged
  fixture; assert the second outcome has `records == 0` and `unchanged == 1`, and
  that the store's **line count is identical** before and after the second run.
  The line-count assertion is the one that would catch a comparison that never
  matches.
- `harvest_appends_when_a_bucket_changes` — harvest, then append a *second*
  message to the same session fixture, then harvest again; assert the second
  outcome has `records == 1` and `unchanged == 0`, and that folding the store
  yields the **summed** totals. This is the negative case for §2's
  "compare the whole record" rule — a key-presence-only check passes the first
  test and fails this one.
- `harvest_appends_everything_into_an_empty_store` — first harvest into a fresh
  `TempDir` appends all buckets and reports `unchanged == 0`.
- `harvest_candidate_carries_the_record_tag` — after one harvest, read the store
  back and assert every ledger record has `record == ARCHITECT_LEDGER_RECORD_TAG`.
  Guards the silent-no-op failure mode in §2.

Determinism: no `sleep`, no wall clock, no new crate. The fixtures already carry
fixed timestamps — keep using them.

## End-to-end verification

The artifact is the running binary's write behavior against a real transcript
corpus. Verify with a **scratch copy** of the store so the real one is never
mutated.

```bash
cargo build --release
SP=$(mktemp -d)
head -60000 ~/.rexymcp/telemetry/phase_runs.jsonl > "$SP/store.jsonl"
TX=~/.claude/projects/-home-matt-src-rexyMCP

before=$(wc -l < "$SP/store.jsonl")
target/release/rexymcp harvest --config rexymcp.toml --transcript-dir "$TX" \
  --telemetry-path "$SP/store.jsonl"
mid=$(wc -l < "$SP/store.jsonl")
target/release/rexymcp harvest --config rexymcp.toml --transcript-dir "$TX" \
  --telemetry-path "$SP/store.jsonl"
after=$(wc -l < "$SP/store.jsonl")

echo "before=$before after-first=$mid after-second=$after"
echo "second harvest appended $((after - mid)) lines"
```

The **second** harvest must append **0 lines** (`after == mid`), and its printout
must report `0` new with a non-zero unchanged count. The first harvest may append
records — the scratch store is a 60k-line prefix, so some buckets legitimately
differ from the full corpus. Quote the literal `before=… after-first=… after-second=…`
line and the second harvest's printout in the completion Update Log.

> **Measurement discipline (M43 phases 01–02 lesson, twice burned).** State the
> result as a **difference you observe in one session** — here, `after - mid`
> lines — not as an absolute number carried in from elsewhere. And assert the
> thing you measured actually ran: if the harvest command errors, `after == mid`
> is also true, and a broken command would read as a perfect pass. Check the
> command's exit status and that its printout appeared before believing the zero.

## Authorizations

None. No new dependency, no `Cargo.toml` edit. Touches `mcp/src/harvest.rs`,
`mcp/src/sweep.rs`, and the `Commands::Harvest` printout in `mcp/src/main.rs`.

## Out of scope

- **Compacting the existing 103 MB store.** This phase stops the *growth*; it does
  not reclaim what is already written. Compaction rewrites the user's telemetry
  file and so carries a data-migration surface that deserves its own review —
  it is **phase 06**. Do not add a compaction pass, and do not rewrite or truncate
  `phase_runs.jsonl` here.
- **Auto-compaction inside the sweep.** Same reason, more so: a background process
  rewriting the store unattended is exactly the shape that loses data.
- **Changing `fold_ledger`'s key or last-write-wins semantics.** The dedup identity
  is the contract this phase reads; leave it alone.
- **The `schema_version` divergence** (phase 05) and **the render path** (phase 04).
- **Making the added store read conditional or incremental.** This phase adds one
  full read of the store per harvest (≈150 ms against today's 103 MB file, once
  per 60 s sweep tick) in exchange for stopping ~53 KB/minute of appends. That
  trade is deliberate and it improves further once phase 06 shrinks the file.
  If the read shows up as a problem, that is a finding for a later phase, not a
  reason to complicate this one.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
