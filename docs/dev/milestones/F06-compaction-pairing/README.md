# F06 — Compaction pairing

**Goal:** context compaction never produces a request the backend rejects.

**Status:** in-progress — opened 2026-09-16 on human sign-off. Phase 01 is
drafted and dispatchable.

**Depends on:** none.

**Source:** the CRS modernisation deployment, 2026-09-15. A spec-check run
ended on turn 59 with HTTP 400 from DeepSeek ("Messages with role 'tool' must
be a response to a preceding message with 'tool_calls'"). The request that
failed was the first one after the run's 13th compaction, which evicted 15
messages; the 12 compactions before it had worked.

**Exit criteria:**

- [ ] Eviction removes an assistant tool-call message together with its
      replies.
- [ ] No compaction that evicts leaves a `tool` message first in the history.
- [ ] A regression test reproduces the failing shape and fails when the fix is
      reverted.
- [ ] All four gates pass.

## Architecture references

- `executor/src/context/compactor.rs` — `compact()`, Pass 2.
- `executor/src/ai/backends/openai.rs` — `convert_messages`.

## Phases

| #  | Phase                                                           | Status |
|----|-----------------------------------------------------------------|--------|
| 01 | evict-tool-pairs ([phase-01-evict-tool-pairs.md](phase-01-evict-tool-pairs.md)) | done        |

## Notes

- **Cause located in source, not in the failing request.** The session log
  records compaction counts, not the messages sent, so the pairing gap in Pass
  2 is the evident cause rather than a replayed one.
- **Position, not id.** Existing compactor tests pair `"tc1"` calls with
  `"c1"` replies, so the fix matches replies by position.
- **Opened ahead of F05 phase-01.** A failed compaction ends any long run,
  whatever it is doing. F05 resumes at its phase-01 once this closes.
