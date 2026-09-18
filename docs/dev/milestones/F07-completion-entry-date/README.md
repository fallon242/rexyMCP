# F07 — Completion-entry date

**Goal:** the server-authored completion entry heads itself with the
`WORKFLOW.md` date format, not a raw epoch.

**Status:** open — opened 2026-09-18 on human go-ahead.

**Depends on:** none.

**Source:** `NEXT.md` open item, 4 occurrences (at fold threshold). Every
server-authored entry reads `### Update — ts=1783568064382 (complete,
server-authored)`, while every other Update Log entry reads
`### Update — YYYY-MM-DD HH:MM (…)`. A reviewer has to convert the epoch by
hand to order entries.

**Exit criteria:**

- [ ] `baseline_entry` heads the entry `### Update — YYYY-MM-DD HH:MM (complete, server-authored)` in UTC.
- [ ] No new dependency; the existing civil-from-days helper is reused.
- [ ] Historical Update Logs are left as written.
- [ ] All four gates pass.

## Architecture references

- `mcp/src/finalize.rs` — `baseline_entry()`.
- `executor/src/agent/prompt.rs` — `format_utc_date()` / `format_utc_time()`.

## Phases

| #  | Phase                                                                    | Status |
|----|--------------------------------------------------------------------------|--------|
| 01 | dated-completion-entry ([phase-01-dated-completion-entry.md](phase-01-dated-completion-entry.md)) | done        |

## Notes

- **Routing: local.** In-place edit in existing code.
