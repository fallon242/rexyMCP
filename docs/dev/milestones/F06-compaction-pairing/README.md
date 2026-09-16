# F06 — Compaction pairing

**Goal:** context compaction never produces a request the backend rejects.

**Status:** done — opened and closed 2026-09-16. One phase,
`approved_first_try`.

**Depends on:** none.

**Source:** the CRS modernisation deployment, 2026-09-15. A spec-check run
ended on turn 59 with HTTP 400 from DeepSeek ("Messages with role 'tool' must
be a response to a preceding message with 'tool_calls'"). The request that
failed was the first one after the run's 13th compaction, which evicted 15
messages; the 12 compactions before it had worked.

**Exit criteria:**

- [x] Eviction removes an assistant tool-call message together with its
      replies.
- [x] No compaction that evicts leaves a `tool` message first in the history.
- [x] A regression test reproduces the failing shape and fails when the fix is
      reverted.
- [x] All four gates pass.

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

## F06 retrospective

**Closed 2026-09-16 at one phase**, `approved_first_try`: zero bugs, zero
bounces, zero assists, 48 executor turns on `deepseek-flash` (code `7273fc7`,
approval `29e1c3f`). All four exit criteria were met. A mutation run at review
removed both fix steps and three of the new tests failed (22 passed; 3 failed).
Gates on review re-run: 706 + 2 + 1142 passed.

**Main finding: spec §1 does nothing.** Removing only the call-with-replies step
in Pass 2 left all 25 compactor tests green. Eviction removes from the front, so
an evicted call's replies become the oldest messages. The next loop pass evicts
them, or Pass 2.5 clears them once the loop stops. Pass 2.5 on its own is the
whole fix. The redundancy is about 15 lines in `compact()` and came from the
architect's spec, not the executor. Deleting it is a candidate follow-up, not
required.

**Calibration: no folds.** Held as data:

- Spec asked for a step another step makes redundant: 1×.
- Test assertion that always passes (`tool_call_id == "c1"` against a helper
  that only produces `"c1"`): 1×.
- Executor naming itself `claude-opus-4-6` in its own Update Log entries: 1×.
- Server-authored completion entry headed `ts=<epoch-ms>` instead of a date:
  **4×** (3× recorded at M46). Still waiting on the human go-ahead as a runtime
  fix.

**Found during the run, outside F06 scope.** The egress pre-scan failed with
`NER engine call failed ... http://Ip_1:8080/v1/chat/completions`. The
configured `engine_base_url` is a LAN IP address, and `Ip_1` is the tokenizer's
`<Kind>_<n>` token (`executor/src/privacy/tokenizer.rs:44`). Redaction rewrote
the NER engine's own URL, so the run had only structured-PII redaction and the
write-guard was off. Where the rewrite happens has not been traced. This belongs to
F05 (privacy hardening) and is not filed yet.
