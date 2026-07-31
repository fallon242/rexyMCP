# Phase 1: Reject unknown model-override keys; fix the `<think>` open tag

**Milestone:** M43 — Thinking-mode round-trip
**Status:** todo
**Depends on:** none
**Estimated diff:** 2 production lines + tests
**Tags:** language=rust, kind=fix, size=s

## Goal

Turn a mistyped or not-yet-supported `[models."…"]` key from a silent no-op into
a loud config error, and make a streamed reasoning block open with `<think>`
rather than `</think>`.

Two unrelated one-line fixes, batched because both are trivial, both belong to
the same failure story, and together they make phase-02 diagnosable.

## Architecture references

- `docs/dev/milestones/M43-thinking-mode-round-trip/README.md` — the three
  defects, how they chain, and the `feat/executor-thinking-and-autocomplete`
  branch that interacts with this one.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the milestone README, including the branch note.
3. Run Spec §0 and confirm the anchors match. If they do not, **stop and file a
   blocker.**

## ⚠️ Both fixes were prototyped and verified before this doc was written

The architect applied both against `master` at `a2fdbe2` and got **1761 tests
passing** (685 + 2 + 1074), then reverted. Do **not** redesign them.

## ⚠️ Neither fix is covered by an existing test — that is the actual work

The prototype changed both production lines and the suite **stayed at 1761
passing**. Nothing asserted either behaviour. So the two-line diff is the easy
part; the tests are the deliverable.

Do not treat a green suite as evidence the fix works. It was green before.

## ⚠️ `deny_unknown_fields` is a deliberate breaking change

After this phase, a config carrying a key `ModelOverride` does not know **fails
to load** instead of being ignored. That is the point — but note the specific
case that motivated it: `thinking = "disabled"` is a **valid key on the
`feat/executor-thinking-and-autocomplete` branch** and an **unknown key on
`master`**. So a user running a `master` build with a config written for that
branch will, after this phase, get a hard error where they previously got
silence.

That is the correct outcome — silence is what wasted a debugging session — but
if that branch is expected to merge soon, sequence the two deliberately rather
than landing this first and breaking configs that are about to become valid.

## Current state

**Defect A — `executor/src/config.rs:292-294`:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelOverride {
```

There is no `thinking` field on `master` — the nearest is `enable_thinking:
Option<bool>`, which is sent as `chat_template_kwargs.enable_thinking`, a
vLLM/llama-server convention rather than a DeepSeek one.

Note `config.rs` already carries a comment acknowledging the same gap on the
top-level `Config` struct ("because `Config` derives `#[serde(default)]` without
`deny_unknown_fields`"). This phase fixes `ModelOverride` **only**.

**Defect B — `executor/src/ai/backends/openai.rs:285`:**

```rust
if let Some(chunk) = reasoning_chunk {
    if !in_reasoning {
        out.push_str("</think>");   // ← opens the block with a CLOSING tag
        in_reasoning = true;
    }
```

`grep -n 'push_str("</think>")' executor/src/ai/backends/openai.rs` finds the
opener at 285 alongside the genuine closers; `grep -c 'push_str("<think>")'`
returns `0`.

## Spec

### 0. Verify the anchors

```bash
grep -n "pub struct ModelOverride" executor/src/config.rs
grep -c 'push_str("<think>")' executor/src/ai/backends/openai.rs
cargo test 2>&1 | grep -E "^test result"
```

Expected: the struct is at ~294 and the two lines above it are the derive and a
bare `#[serde(default)]`; the second returns `0`; the third totals **1761**
passing. If any differs, **stop and file a blocker.**

### 1. Apply this diff

```diff
diff --git a/executor/src/ai/backends/openai.rs b/executor/src/ai/backends/openai.rs
index 9d4a680..0360a58 100644
--- a/executor/src/ai/backends/openai.rs
+++ b/executor/src/ai/backends/openai.rs
@@ -267,7 +267,7 @@ impl AiClient for OpenAiClient {
                                             .filter(|r| !r.is_empty());
                                         if let Some(chunk) = reasoning_chunk {
                                             if !in_reasoning {
-                                                out.push_str("</think>");
+                                                out.push_str("<think>");
                                                 in_reasoning = true;
                                             }
                                             out.push_str(chunk);
diff --git a/executor/src/config.rs b/executor/src/config.rs
index c8bb99a..a8cf4fb 100644
--- a/executor/src/config.rs
+++ b/executor/src/config.rs
@@ -290,7 +290,7 @@ impl Default for GovernorConfig {
 /// model; each `None` field inherits the global value. Keyed by exact model id
 /// in the `[models]` table (e.g. `[models."Qwen/Qwen3.6-27B-FP8"]`).
 #[derive(Debug, Clone, Serialize, Deserialize, Default)]
-#[serde(default)]
+#[serde(default, deny_unknown_fields)]
 pub struct ModelOverride {
     pub task_tracking: Option<bool>,
     pub temperature: Option<f64>,
```

### 2. Add the two missing tests

Both fixes need a test that fails when the production line is reverted.

**Defect A** — in `executor/src/config.rs`'s test module: a TOML string with a
bogus key inside a `[models."…"]` table must fail to load, and the error text
must mention the offending key. The module already has config-loading tests
(`grep -n "fn config_" executor/src/config.rs`) — follow their shape, including
how they build a temp config file.

**Defect B** — in `executor/src/ai/backends/openai.rs`'s test module: a streamed
reasoning delta must produce output where `<think>` appears **before**
`</think>`. `grep -n "fn delta_carries_token\|fn.*stream" executor/src/ai/backends/openai.rs`
finds the existing streaming-test harness.

Pin the **behaviour**: assert the opening tag is present and precedes the
closing tag. Do not assert an exact rendered blob — the surrounding text is not
what this fix is about.

### 3. Run the gates

```bash
cargo fmt --all
cargo build
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## Acceptance criteria

- [ ] `cargo fmt --all --check` reports no diff.
- [ ] `cargo build` succeeds with zero new warnings.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo test` passes with **at least 2 more tests than the 1761 baseline**.
- [ ] `grep -c 'push_str("<think>")' executor/src/ai/backends/openai.rs` is `1`.
- [ ] `deny_unknown_fields` is added to `ModelOverride` **only** — not to
      `Config`, `ExecutorConfig`, or any other struct.
- [ ] Reverting either production line makes a test fail. Demonstrate both.

## Test plan

Two new tests, one per defect, placed beside the existing tests in the file they
cover. No new test files.

## End-to-end verification

Paste the output of:

```
cargo test 2>&1 | grep -E "^test result"
```

Then prove each fix is load-bearing, one at a time — revert, run, restore:

1. `push_str("<think>")` → `push_str("</think>")`, `cargo test`, expect a
   failure, restore.
2. Remove `deny_unknown_fields`, `cargo test`, expect a failure, restore.

Paste both results, and confirm `git diff` is clean of the temporary reverts.

Finally, the real-world behaviour that motivated defect A:

```
printf '[project]\nid = "x"\n\n[models."m"]\nthinking = "disabled"\n' > /tmp/bad.toml
cargo run -- doctor --config /tmp/bad.toml 2>&1 | head -6
```

Expect a parse error naming `thinking` and listing the valid keys.

## Authorizations

Files this phase may modify:

- `executor/src/config.rs` — the `ModelOverride` derive, plus one test.
- `executor/src/ai/backends/openai.rs` — the one `push_str`, plus one test.

No new dependencies. No new files.

## Out of scope

- Do **not** implement the `reasoning_content` round-trip — phase-02.
- Do **not** add a `thinking` config key. That belongs to
  `feat/executor-thinking-and-autocomplete`; duplicating it here would conflict
  on merge.
- Do **not** add `deny_unknown_fields` to any other struct. Tightening the whole
  schema is a larger decision with migration risk for existing configs.
- Do **not** change what `enable_thinking` does, only how an unknown key is
  reported.
- Do **not** edit any `rexymcp.toml`.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
