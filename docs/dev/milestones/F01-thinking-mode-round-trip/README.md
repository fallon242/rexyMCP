# F01 — Thinking-mode round-trip

**Goal:** Let the executor work with reasoning models that require their
`reasoning_content` echoed back, instead of only being able to switch thinking
off — and make a config key that does not exist say so.

**Status:** in-progress — unparked 2026-09-18 on human go-ahead; phase-01 redrafted.

## Why this milestone, now

A dispatch against DeepSeek `hard_fail`ed on turn one:

```
API error 400 Bad Request:
  "The `reasoning_content` in the thinking mode must be passed back to the API."
```

Three defects sit behind that, and they chain:

| # | Defect | Where (on `master`) |
|---|---|---|
| 1 | `[models."…"]` overrides silently accept unknown keys | `executor/src/config.rs:292-294` |
| 2 | a reasoning block is *opened* with `</think>` | `executor/src/ai/backends/openai.rs:285` |
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

## The `thinking` key is now real

The `feat/executor-thinking-and-autocomplete` branch this section used to
describe was merged (`a2fdbe2`): `ModelOverride` has a `thinking:
Option<String>` field, so `thinking = "disabled"` is a **valid** key. Phase-01's
unknown-key test uses a genuine typo (`thinkng`) instead. Switching thinking off
remains the workaround; phase-02's round-trip is the fix.

**Leftover rate keys stay accepted.** The pricing removal (`8901564`) promised
that old `input_per_mtok` / `output_per_mtok` / `cache_read_per_mtok` /
`cache_creation_per_mtok` keys are ignored. Phase-01 declares those four as
ignored fields so `deny_unknown_fields` does not break existing configs.

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
| 01 | reject unknown model-override keys; fix the `<think>` open tag ([phase-01](phase-01-reject-unknown-keys-and-fix-think-tag.md)) | done (escalated) |
| 02 | round-trip `reasoning_content` on assistant turns               | todo   |

Split because phase-01 is two verified one-line changes needing only tests,
while phase-02 changes the `Message` type and the streaming capture — different
size, different risk. Phase-01 also makes phase-02 diagnosable: until unknown
keys are rejected, a config typo can masquerade as a round-trip bug.

## Notes

- **Baseline is 729 / 2 / 1218** on `master` at `ea67993` (2026-09-18; was 1761 at `a2fdbe2`).
- **Phase-01 was prototyped and verified; phase-02 was not.** Phase-02 needs a
  live thinking-mode endpoint to confirm end to end, which the architect did not
  have. Its spec is derived from the API error and the code, and says so.
- **Neither phase-01 fix is covered by any existing test.** The architect applied
  both and the suite stayed at 1761 passing. Treat "green" as evidence of
  nothing here; the tests are the deliverable.
- **This numbering had a false start.** An earlier draft of this milestone was
  written as "M35" against a clone that was 238 commits behind and had a stale
  remote-tracking ref. M35–M42 already existed upstream. The defects were
  re-verified against `659d321` before this renumber; all three still present.
