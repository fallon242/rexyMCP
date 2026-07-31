# M43 — Thinking-mode round-trip

**Goal:** Let the executor work with reasoning models that require their
`reasoning_content` echoed back, instead of only being able to switch thinking
off — and make a config key that does not exist say so.

**Status:** in-progress

## Why this milestone, now

A dispatch against DeepSeek `hard_fail`ed on turn one:

```
API error 400 Bad Request:
  "The `reasoning_content` in the thinking mode must be passed back to the API."
```

Three defects sit behind that, and they chain:

| # | Defect | Where (on `master`) |
|---|---|---|
| 1 | `[models."…"]` overrides silently accept unknown keys | `executor/src/config.rs:293` |
| 2 | a reasoning block is *opened* with `</think>` | `executor/src/ai/backends/openai.rs:270` |
| 3 | `reasoning_content` is dropped when an assistant turn is replayed | `executor/src/ai/backends/openai.rs` `convert_messages` |

**Defect 3 is the failure.** `convert_messages` rebuilds an assistant turn as
`{role, content, tool_calls}`. `Message` (`executor/src/ai/types.rs`) has no
field for reasoning at all, so a thinking model's `reasoning_content` is read
off the stream, folded into the content string as `<think>` text, and then lost
on the next request. DeepSeek rejects the follow-up.

**Defect 1 is why it was confusing.** `ModelOverride` derives `#[serde(default)]`
without `deny_unknown_fields`, so a key it does not know is accepted and
ignored. A config carrying `thinking = "disabled"` therefore did nothing on a
`master` build — see the branch note below for why that key looked valid.

**Defect 2 is adjacent and independent.** Every `push_str` in that state machine
emits `</think>` — there is no `<think>` anywhere in the file — so a streamed
reasoning block renders as `</think>…</think>` and downstream consumers that
strip `<think>…</think>` pairs cannot match it.

## The `feat/executor-thinking-and-autocomplete` branch already knows

Before drafting, read that branch. It is 6 commits ahead of `master` on the
`fallon242` fork and touches the same files. It does **not** fix any of the
three defects above, but it is directly relevant:

- It **adds `thinking: Option<String>`** to `[executor]` and to `[models."…"]`,
  emitted verbatim as `"thinking": {"type": <value>}`. So `thinking = "disabled"`
  is a *real key on that branch* and an *unknown key on `master`* — which is
  exactly how a config could look correct and do nothing.
- Its own doc comment states the case plainly: DeepSeek-style APIs require
  thinking-mode tool-call turns to carry something *"rexyMCP does not
  implement,"* so it disables thinking to take the non-thinking path instead.

**That branch is the workaround; this milestone is the fix.** They are
complementary, not competing: being able to switch thinking off is worth having
regardless. Whoever lands M43 should decide whether that branch merges first —
if it does, phase-01's `deny_unknown_fields` will start rejecting configs that
were silently tolerated, which is the point but is also a breaking change worth
sequencing deliberately.

## Exit criteria

- [ ] An unknown key in a `[models."…"]` table is a **loud config error** naming
      the valid keys, not a silent no-op.
- [ ] A streamed reasoning block opens with `<think>` and closes with `</think>`.
- [ ] An assistant turn carrying reasoning is replayed with its
      `reasoning_content` intact, so a thinking-mode dispatch survives past turn
      one.
- [ ] Each fix has a test that fails when the fix is reverted.
- [ ] All four gates pass.

## Phases

| #  | Phase                                                          | Status |
|----|----------------------------------------------------------------|--------|
| 01 | reject unknown model-override keys; fix the `<think>` open tag  | todo   | ← active
| 02 | round-trip `reasoning_content` on assistant turns               | todo   |

Split because phase-01 is two verified one-line changes needing only tests,
while phase-02 changes the `Message` type and the streaming capture — different
size, different risk. Phase-01 also makes phase-02 diagnosable: until unknown
keys are rejected, a config typo can masquerade as a round-trip bug.

## Notes

- **Baseline is 1741 tests** (685 + 2 + 1054) on `master` at `659d321`.
- **Phase-01 was prototyped and verified; phase-02 was not.** Phase-02 needs a
  live thinking-mode endpoint to confirm end to end, which the architect did not
  have. Its spec is derived from the API error and the code, and says so.
- **Neither phase-01 fix is covered by any existing test.** The architect applied
  both and the suite stayed at 1741 passing. Treat "green" as evidence of
  nothing here; the tests are the deliverable.
- **This numbering had a false start.** An earlier draft of this milestone was
  written as "M35" against a clone that was 238 commits behind and had a stale
  remote-tracking ref. M35–M42 already existed upstream. The defects were
  re-verified against `659d321` before this renumber; all three still present.
