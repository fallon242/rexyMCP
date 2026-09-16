# Phase 1: evict tool-call pairs together

**Milestone:** F06 — Compaction pairing
**Status:** todo
**Depends on:** none
**Estimated diff:** ~150 lines, most of it tests
**Tags:** language=rust, kind=bugfix, size=s

## Goal

Stop context compaction from sending a request that the backend rejects.
Eviction removes the oldest messages one at a time. When it stops just after
removing an assistant message that made tool calls, the replies to those calls
are left at the front of the history with nothing before them. An
OpenAI-compatible backend then fails the whole run:

```
API error 400 Bad Request: Messages with role 'tool' must be a response to a
preceding message with 'tool_calls'
```

This happened in a real run: the 13th compaction of the session, after 12 that
worked. After this phase, eviction removes an assistant message together with
its replies, and never leaves a `tool` message at the front of the history.

## Architecture references

- `executor/src/context/compactor.rs` — the whole file is under 800 lines.
  Read `compact()` (lines 51–125) and the test module helpers.
- `executor/src/ai/backends/openai.rs` — `convert_messages` (around line 37).
  Read only; it shows why the pairing matters.

## Pre-flight

1. `cargo test -p rexymcp-executor compactor` passes before you start.
   Record the count.
2. Read `compact()` in full before editing.

## Current state

`compact()` works in three passes. Pass 1 and Pass 1.5 shrink tool output in
place and never remove a message. Pass 2 removes messages:

```rust
// executor/src/context/compactor.rs:106-115
// ── Pass 2: evict oldest non-system messages until under target ──
while running_total > target {
    let evict_idx = messages.iter().position(|m| m.role != "system");
    let Some(idx) = evict_idx else {
        break;
    };
    let removed = messages.remove(idx);
    running_total = running_total.saturating_sub(message_tokens(&removed));
    messages_evicted += 1;
}
```

Nothing ties an assistant message to its replies. `compact()` is the only code
in `executor/src/` that removes messages from the history (`grep -rn
'messages.remove' executor/src` finds only line 112), so fixing it here fixes
every path.

How the pair looks in memory (`executor/src/ai/types.rs`):

- **The call:** `role == "assistant"`, `tool_calls: Some(vec![ToolCall { id, .. }])`.
- **The reply:** `role == "tool"`, `tool_results: Some(vec![ToolResult { tool_call_id, .. }])`.
  One reply message follows the call, or several if the call held several.

`convert_messages` turns every `tool_results` entry into a `{"role": "tool",
"tool_call_id": …}` object. The backend requires each one to come after the
assistant message that issued that id.

## Spec

### 1. Evict a call together with its replies

In Pass 2, when the removed message has `role == "assistant"` and a non-empty
`tool_calls`, also remove every message directly after it (at the same index,
now that the call is gone) whose `role == "tool"`. Stop at the first message
that is not `role == "tool"`. Subtract each removed message's
`message_tokens` from `running_total`, and count each one in
`messages_evicted`.

Match on **position, not id.** Existing tests build tool messages with the id
`"c1"` while their assistant messages use `"tc1"`, so an id-matching rule would
treat valid test pairs as orphans and break tests that do not evict at all.

### 2. Never leave a reply at the front

After the Pass 2 loop, **only if Pass 2 removed at least one message**, remove
any `role == "tool"` messages that are now the first non-system messages.
Repeat until the first non-system message is not a `tool` message, or none is
left. Count and subtract them the same way.

This catches a history whose oldest message was already a reply when eviction
started. Do not run this step when nothing was evicted. Several existing tests
start with a `tool` message straight after `system` and assert that it
survives (`compact_reclaims_command_output_before_file_read`,
`compact_protects_recent_tool_result`). Those must keep passing unchanged.

### 3. Keep the rest as is

Pass 1 and Pass 1.5 are unchanged. `system` messages are never removed. The
`CompactionReport` fields are unchanged. `messages_evicted` now includes the
replies removed with their call.

## Acceptance criteria

- [ ] After any `compact()` call that evicted at least one message, the first
      non-system message is not `role == "tool"`.
- [ ] Every assistant message removed by Pass 2 takes its directly following
      `tool` messages with it.
- [ ] A call/reply pair that eviction did not reach is left intact.
- [ ] All existing compactor tests pass without modification.
- [ ] The new regression test fails against the unfixed code. Show this in the
      Update Log: write the test first, run it, and quote the failure before
      making the fix.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

Add these to the existing `mod tests` in `compactor.rs`. Reuse the helpers
already there: `make_system`, `make_user_with_turn`,
`make_assistant_with_turn`, `make_tool_msg`. Add one helper for an assistant
message with calls, shaped like the literal already inside
`compact_signaturization_preserves_pairing_and_count`:

```rust
fn make_call_msg(content: &str, ids: &[&str], turn: usize) -> Message {
    Message {
        role: "assistant".to_string(),
        content: content.to_string(),
        tool_calls: Some(
            ids.iter()
                .map(|id| crate::ai::types::ToolCall {
                    id: id.to_string(),
                    name: "bash".to_string(),
                    arguments: r#"{"command":"ls"}"#.to_string(),
                    thought_signature: None,
                })
                .collect(),
        ),
        tool_results: None,
        turn: Some(turn),
    }
}
```

1. **`compact_eviction_takes_replies_with_call`** — the regression test.
   History: `system`, a call whose **`content` is large** (`"x".repeat(2000)`,
   about 500 tokens) at turn 1, its small reply at turn 1, a second small
   call at turn 2, its small reply at turn 2, and `make_user_with_turn("recent",
   3)`. Every turn is within 3 of the newest, so Pass 1.5 shrinks nothing and
   only Pass 2 can free tokens. Pick a `Budget::new(n)` where removing the
   large call alone brings the total under `n * 3 / 4`; about 400 works. Check
   the numbers against `Budget::estimate`. Assert: the first non-system
   message is not `role == "tool"`, and the second call and its reply are both
   still present, in order. On the unfixed code, the first reply is left at
   index 1, so the test fails.
2. **`compact_eviction_takes_every_reply_of_a_multi_call`** — a large call
   carrying two ids, followed by two `tool` messages, then a small call/reply
   pair and a recent user message. Assert that both replies are gone and that
   `messages_evicted` counts the call and both replies.
3. **`compact_eviction_drops_leading_orphan_reply`** — after `system`: a
   **large plain user message** (`make_user_with_turn`, 2000 characters, turn
   1), then a small `tool` message at turn 1 that no call precedes, then a
   small call/reply pair at turn 2 and a recent user message at turn 3.
   Evicting the large user message alone reaches target, so on the unfixed
   code the orphan reply is left first. Assert that the first non-system
   message is not `role == "tool"`, and that the turn-2 pair survives.
4. **`compact_without_eviction_keeps_leading_reply`** — a leading `tool`
   message and a budget that needs no eviction. Assert that it is still
   present. This pins the "only if Pass 2 removed something" condition.

**Must NOT happen:** a `tool` message that sits after a call which was not
evicted must never be removed (test 1's second pair covers this).

## End-to-end verification

Not applicable — the change is internal to `compact()` and has no new
user-visible surface. The regression test reproduces the exact history shape
that failed in production. A live check would need a run long enough to hit
the 13th compaction, which cannot be triggered on demand.

## Authorizations

- [x] May edit `executor/src/context/compactor.rs`: `compact()` and its test
      module.
- [x] May add a private helper function in `compactor.rs` if it makes Pass 2
      clearer.
- [ ] May edit `executor/src/ai/backends/openai.rs` — **no.** The fix belongs
      where messages are removed.
- [ ] May edit `executor/src/agent/mod.rs` — **no.**
- [ ] May change any existing test — **no.**
- [ ] May add a dependency — **no.**
- [ ] May change `CompactionReport` — **no.**

## Out of scope

- **A request-time validator in the backend.** `compact()` is the only place
  messages are removed. Add one if a second removal path ever appears.
- **An assistant `tool_calls` message with no reply after it.** Eviction works
  from the front, so it cannot create that shape.
- **Logging the role sequence after compaction.** It would help diagnosis, but
  it changes `SessionEvent`.
- **Eviction of the opening user prompt.** Pass 2 can remove it; that is a
  separate question.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
