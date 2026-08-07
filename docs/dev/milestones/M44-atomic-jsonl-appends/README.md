# M44 — Atomic JSONL Appends

**Goal:** Make a telemetry append a single atomic write, so concurrent appenders
can no longer interleave and corrupt a line — and make the corruption that
already exists *visible* instead of silently skipped.

**Status:** done (closed 2026-08-05, one phase)

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
      runs. **Live in the running `serve` since 2026-08-05 10:03**, verified by
      `strace` rather than by timestamp — see § "Verified live, not assumed".
- [~] ~~Malformed lines are no longer invisible. Every reader that currently drops
      a parse failure silently reports a count, and at least one user-facing
      surface shows it.~~ **Withdrawn 2026-08-05** with the user's decision — see
      § "Phase 02 declined". This criterion was written before the corpus was
      cleaned and the cause fixed; with both done there is no corruption left to
      reveal and no known mechanism to create more.
- [x] No behavior change for well-formed stores. Met by phase 01, and by stronger
      evidence than the criterion asked for: an `strace` of one append shows the
      pre-fix form writing `payload(282) + "\n"(1)` and the fixed form writing
      `payload(283)`, producing a **byte-identical 283-byte file**. Output
      equivalence is proven at the syscall boundary rather than inferred from a
      `costs` comparison, and all 69 existing telemetry tests pass unmodified.

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
skipped in silence, so **roughly 418 ledger records were invisible** *(as of
milestone open — those lines are gone from the store now; see § "Phase 02
declined")* and nothing ever said so. The defect was found by accident, while
simulating an unrelated migration — which is the real problem: there is no path by
which this reports itself.

The fix to the writer is nearly free (build one buffer, issue one `write_all`).
The reason it survived is that nothing was looking.

## Phases

| # | Phase | Status |
| --- | --- | --- |
| 01 | one atomic write per append ([phase-01-one-atomic-write-per-append.md](phase-01-one-atomic-write-per-append.md)) | done |
| 02 | ~~surface malformed-line counts instead of skipping silently~~ | **declined** |

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

**02 was declined at milestone close** — the reasoning is in § "Phase 02
declined" below, and it supersedes the intent stated in this paragraph. Kept as
written because the design question it framed is still a real one, just not one
worth machinery today.

**02** was to be the observability half, carrying the open design question:
**what should a reader do with a line it cannot parse?**
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

---

## M44 retrospective — closed 2026-08-05, one phase

Opened and closed the same day. **One phase, `approved_first_try`, zero bugs,
zero escalations.** Phase 02 was drafted-in-outline and then **declined** — see
below; that decision is the most interesting thing in this milestone.

### What changed

All four telemetry append functions now delegate to a single private
`append_stamped` helper that builds payload + newline into one buffer and issues
**one** `write_all`. The four bodies had been byte-identical apart from the record
type, so the fix is single-sourced rather than copied four times — a fix applied
to three of four would have left the race live.

### Verified live, not assumed

The fix is in the running `serve` as of **2026-08-05 10:03**. That was confirmed
functionally, because the obvious check was actively misleading: `md5sum` of the
installed binary differs from a local `cargo build --release` of the same commit,
since `cargo install` embeds different build paths. Comparing hashes would have
suggested staleness that wasn't there.

The defect has a deterministic syscall signature, so `strace` of a single
`rexymcp review` append settled it — against a deliberately rebuilt pre-fix
control in the same session:

| Binary | `write` syscalls per append |
| --- | --- |
| pre-fix control | `write(9, payload, 282)` **then** `write(9, "\n", 1)` |
| installed binary | `write(9, payload, 283)` |

Both produce a **byte-identical 283-byte file**, which is exactly why this bug
was invisible for months: under a single writer the two forms are
indistinguishable in the output. The syscall boundary is the only place the
difference shows.

That same trace doubles as the phase's "no behavior change" evidence — stronger
than the `rexymcp costs` comparison the exit criterion originally asked for.

### Phase 02 declined

Phase 02 was to make malformed lines visible instead of silently skipped. **The
user declined it, and the reasoning is worth preserving rather than filing as
"deferred".**

By the time the fork was put to the user, both halves of the problem were already
closed:

- **The cause is fixed** (phase 01, verified live above), so no new spliced lines
  can appear from this mechanism.
- **The corpus is clean.** M43 phase-06's compaction dropped all 209 malformed
  lines from the live store that morning — that is what its `malformed: 209`
  report line was. A census of every file on disk at close confirms **zero
  malformed and zero blank lines** in the live store (891 lines) and in all three
  remaining June backups (184 lines each). The 108 MB compact backup that held
  them has since been removed, so the corrupt lines are gone from the system
  entirely.

So phase 02's remaining value was detecting *future, unknown* causes of readers
discarding input — real, but speculative. Weighed against that: the design fork
had no cheap option. Logging from inside a reader was effectively off the table
(no `tracing`/`log` crate, the executor library emits zero diagnostics today, and
the dashboard calls these readers at 2 Hz), and the criterion's literal reading —
all four readers returning counts — hits ~13 production call sites plus ~44 test
sites across 6 files.

**Reopening trigger, stated so this is not a silent drop:** a reader is found
discarding input silently again. Not a schedule, not a fourth occurrence of
something — that specific event.

### Carried forward — the wider defect this milestone did *not* fix

`filter_map(|l| serde_json::from_str::<Value>(l).ok())` at
`executor/src/store/telemetry.rs:223`, `:421`, `:585`, `:721` also silently
swallows **schema-mismatched** records, not only spliced ones. A future field
rename or type change would go equally quiet, and that mechanism is still live.

It has not bitten yet, and it is a different defect from the one M44 opened for,
so it is named here rather than used to justify a phase now. The generalizable
statement — *readers discard input without saying so* — is the thing to remember
if a numbers discrepancy ever shows up again with no obvious cause.

### Calibration — a trend at two, not folded

The executor misreported its own model in this milestone's Update Log
(`claude-opus-4-5-20251101`); the M43 phase-05 entry claimed "Claude (Sonnet
4.5)". Both false, both corrected at review, both harmless to telemetry because
the server-authored bookkeeping tail records the true model
(`Qwen/Qwen3.6-27B-FP8`).

Two occurrences is a trend, not a fix. If it recurs the fold is mechanical rather
than a judgment call: **stop asking the executor to write the model name at all**
and let the server own that field, the way it already owns the completion tail.

### What worked

The spec set the bar at a **mutation going red** rather than at a green suite, and
pre-injected the `thread::spawn` pattern because there is no threading precedent
anywhere in this repo's tests. Result: `approved_first_try` on a phase whose
entire substance was "write a test that can actually fail." That is the M43 lesson
applied forward rather than re-learned — the third milestone in a row where the
deciding question was *can this observation come out differently?*
