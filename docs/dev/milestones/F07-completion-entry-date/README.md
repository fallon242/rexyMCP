# F07 — Completion-entry date

**Goal:** the server-authored completion entry heads itself with the
`WORKFLOW.md` date format, not a raw epoch.

**Status:** done — opened and closed 2026-09-18. One phase,
`approved_first_try`.

**Depends on:** none.

**Source:** `NEXT.md` open item, 4 occurrences (at fold threshold). Every
server-authored entry reads `### Update — ts=1783568064382 (complete,
server-authored)`, while every other Update Log entry reads
`### Update — YYYY-MM-DD HH:MM (…)`. A reviewer has to convert the epoch by
hand to order entries.

**Exit criteria:**

- [x] `baseline_entry` heads the entry `### Update — YYYY-MM-DD HH:MM (complete, server-authored)` in UTC.
- [x] No new dependency; the existing civil-from-days helper is reused.
- [x] Historical Update Logs are left as written.
- [x] All four gates pass.

## Architecture references

- `mcp/src/finalize.rs` — `baseline_entry()`.
- `executor/src/agent/prompt.rs` — `format_utc_date()` / `format_utc_time()`.

## Phases

| #  | Phase                                                                    | Status |
|----|--------------------------------------------------------------------------|--------|
| 01 | dated-completion-entry ([phase-01-dated-completion-entry.md](phase-01-dated-completion-entry.md)) | done        |

## Notes

- **Routing: local.** In-place edit in existing code.

## F07 retrospective

**Closed 2026-09-18 at one phase**, `approved_first_try`: zero bugs, zero
bounces, 54 executor turns on the local `RedHatAI/Qwen3.8-27B-INT4` (code
`77d192b`, approval `d6e5db1`). All four exit criteria met. Red test captured
before the fix. Gates on review re-run: 717 + 2 + 1206 passed.

**Not live until `serve` is rebuilt.** The phase's own completion entry still
reads `ts=1789743238937`: the running `serve` binary predates the fix. The
first dated server entry proves it landed.

**Calibration: no folds.** The `ts=<epoch-ms>` counter (4×) is retired by
this fix. Held as data:

- Executor commit swept in the architect's uncommitted milestone docs: 1×.
  Harmless here; commit dispatch prep before dispatching to avoid it.
