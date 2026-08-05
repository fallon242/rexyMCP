# M44 — Atomic JSONL Appends

**Goal:** Make a telemetry append a single atomic write, so concurrent appenders
can no longer interleave and corrupt a line — and make the corruption that
already exists *visible* instead of silently skipped.

**Status:** planning

**Opened:** 2026-08-05, from a defect found while drafting M43 phase-06.

**Depends on:** M35 (built the append-only store and the four append functions),
M40 (added the 60 s sweep that is the concurrent writer), M43 (compaction, which
shrinks the corpus this fix is verified against, and which surfaced the defect)

**Exit criteria:**

- [x] A telemetry append issues **one** write syscall carrying payload + newline,
      so two concurrent appenders can no longer produce a spliced line. All four
      append functions. Met by phase 01 — one private `append_stamped` helper, all
      four public functions delegating to it, exactly one `write_all` left in the
      append path.
- [x] The fix is demonstrated against the real failure mode, not asserted: a
      concurrency test that reliably produces spliced lines against the current
      code and produces **zero** against the fixed code. Met by phase 01 —
      restoring the two-write form splices **461 / 390 / 451** of 2,000 records
      across three runs (~20 % each time); the fixed code passes five consecutive
      runs. *(Not yet live in the running `serve`, which predates the fix.)*
- [ ] Malformed lines are no longer invisible. Every reader that currently drops
      a parse failure silently reports a count, and at least one user-facing
      surface shows it.
- [ ] No behavior change for well-formed stores: every existing telemetry test
      passes unmodified, and `rexymcp costs` reports identical figures before and
      after on a real store.

## Architecture references

- `docs/architecture.md` §35 (M35 — metrics & cost overhaul) — built the
  append-only store, the `schema_version` write boundary, and the four append
  functions.
- `docs/architecture.md` §40 — the sweep that re-harvests every 60 s and is the
  second writer in the race.
- `docs/architecture.md` §43 — M43, where the defect was found; § "Found while
  drafting 06" in
  [M43/README.md](../M43-dashboard-idle-cpu/README.md) has the original evidence.

## Why this milestone exists

### The defect, measured

Simulating M43 phase-06's compaction rules against the real store
(`~/.rexymcp/telemetry/phase_runs.jsonl`) turned up **209 lines holding two
concatenated JSON objects with no newline between them** — e.g. a 735-byte line
whose parse fails at `Extra data: line 1 column 363`. They sit in one contiguous
band (file lines ~31,620–33,748), so this was a single episode rather than steady
state.

### The cause is structural, and it is in all four append functions

`append` (`executor/src/store/telemetry.rs:195`) writes the payload and the
trailing newline as **two separate `write_all` calls** on an `O_APPEND` handle:

```rust
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
```

`O_APPEND` makes each individual `write` atomic with respect to other appenders —
but there are **two** of them here, so another process can land its own record
between the payload and its newline. The result is `{...record A...}{...record
B...}\n`: two objects on one line, and A's newline effectively donated to B.

All four append functions share the shape:

| Function | Line |
| --- | --- |
| `append` (`PhaseRun`) | `executor/src/store/telemetry.rs:195` |
| `append_review` | `:392` |
| `append_architect_activity` | `:553` |
| `append_architect_ledger` | `:689` |

The concurrent writer is not hypothetical: the M40 sweep inside `rexymcp serve`
re-harvests every 60 s and appends the whole ledger, while a finishing phase run
appends its `PhaseRun` and the architect appends a `review` — three independent
writers against one file.

### What makes it worse than 209 bad lines

**Every reader hides it.** All four read paths drop a parse failure on the floor:

```rust
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
```

`executor/src/store/telemetry.rs:223`, `:421`, `:585`, `:721`. A corrupted line is
skipped in silence, so **roughly 418 ledger records are invisible today** and
nothing ever said so. The defect was found by accident, while simulating an
unrelated migration — which is the real problem: there is no path by which this
reports itself.

The fix to the writer is nearly free (build one buffer, issue one `write_all`).
The reason it survived is that nothing was looking.

## Phases

| # | Phase | Status |
| --- | --- | --- |
| 01 | one atomic write per append ([phase-01-one-atomic-write-per-append.md](phase-01-one-atomic-write-per-append.md)) | done |
| 02 | surface malformed-line counts instead of skipping silently | not drafted |

**01** is the writer fix: build `line + "\n"` into a single buffer and issue one
`write_all`, in all four functions. Small, but it needs a test that *reproduces*
the race rather than asserting the shape of the code — spawning concurrent
appenders and showing spliced lines appear before the fix and never after. That
reproduction is the interesting part of the phase and the reason it isn't a
one-line drive-by.

Drafted 2026-08-05 with two decisions worth recording. First, the four bodies are
**byte-identical** apart from the record type, so the fix goes into one private
generic helper the four public functions delegate to — the defect is one mistake
copied four times, and a fix applied to three of four leaves the race live.
Second, the concurrency test is written to be **one-sided**: under the fixed code
it cannot fail (so it will not flake in CI), and its validity is established by
*mutation* rather than by passing. The phase's deciding criterion is that
restoring the two-write form turns it red. Also pre-injected: a `thread::spawn`
pattern, because there is no threading precedent anywhere in this repo's tests
for the executor to copy.

**02** is the observability half, and it carries the open design question this
milestone must answer: **what should a reader do with a line it cannot parse?**
Options to weigh when drafting — return a count alongside the records; log a
warning; expose it in `rexymcp doctor`; surface it on the dashboard. The
requirement is that the next occurrence announces itself instead of waiting for
someone to simulate a migration. Sequenced second because it is a design
decision, whereas 01 is a defect with one obvious fix.

## Notes

**Sequenced after M43 deliberately.** Compaction first shrinks the corpus this
fix is verified against (108.7 MB → 482 KB), and keeping a producer-side fix out
of the same review as a one-way migration of user data was an explicit M43
decision.

**The existing corruption is not repaired by this milestone.** M43 phase-06's
compaction drops those 209 lines and reports the count, and its backup retains
them. Recovering the ~418 records inside them would be a separate exercise and
may not be worth it — they are `architect_ledger` records, which the harvest
re-derives from the transcripts by last-write-wins, so the current ledger state
is already correct. Worth confirming that reasoning during phase-01 drafting
rather than assuming it.
