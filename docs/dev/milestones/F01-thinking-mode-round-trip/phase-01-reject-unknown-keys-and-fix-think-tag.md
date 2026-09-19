# Phase 1: Reject unknown model-override keys; fix the `<think>` open tag

**Milestone:** F01 — Thinking-mode round-trip
**Status:** todo
**Depends on:** none
**Estimated diff:** ~90 lines, mostly tests and a small extraction
**Tags:** language=rust, kind=fix, size=s

## Goal

Two independent fixes, each with the test that proves it:

1. A key `[models."…"]` does not know (`thinkng = …`) stops the config from
   loading and names itself, instead of being silently ignored.
2. A streamed reasoning block opens with `<think>`, not `</think>`.

**Redrafted 2026-09-18.** The original draft is stale: `ModelOverride` has had
a `thinking` field since `a2fdbe2`, and the baseline has moved. Every block
below was applied by the architect to `master` at `ea67993` in a scratch
worktree: format and clippy clean, suite **729 / 2 / 1220**, and both new
tests were shown to fail before the fixes.

## Pre-flight

1. `cargo test -p rexymcp-executor push_delta_text` → **0 passed** (the test
   does not exist yet).
2. Full `cargo test` → **729 / 2 / 1218 passed**.

## Current state

- `executor/src/config.rs:209-211` — `ModelOverride` derives
  `#[serde(default)]` with no `deny_unknown_fields`, so an unknown key is
  dropped silently.
- `executor/src/ai/backends/openai.rs:285` — the reasoning opener is
  `out.push_str("</think>")`. The logic sits inline in the streaming loop, so no
  test can reach it; this phase moves it into a small function,
  `push_delta_text`, and tests that.
- **Gotcha — removed rate keys.** The pricing removal (`8901564`) left four
  rate keys that old configs still carry in `[models."…"]`:
  `input_per_mtok`, `output_per_mtok`, `cache_read_per_mtok`,
  `cache_creation_per_mtok`. The README promises leftovers are ignored and
  `legacy_rate_keys_are_ignored` pins it. `deny_unknown_fields` alone would
  break both, so those four are declared as ignored fields (block 6).
- **Gotcha — full-field literals.** Two tests in `mcp/src/runner.rs` build a
  `ModelOverride` naming every field; the four new fields break them. Block 8
  shrinks both to `..Default::default()`.

## Spec

Apply these replace blocks exactly. Each "Replace" text occurs once in its
file, except block 8, which occurs exactly twice.

### `executor/src/ai/backends/openai.rs`

**1. Call site in the streaming loop — replace the inline reasoning/content handling with one call.** Replace:

```rust
                                        }

                                        let reasoning_chunk = delta
                                            .get("reasoning")
                                            .or_else(|| delta.get("reasoning_content"))
                                            .and_then(|r| r.as_str())
                                            .filter(|r| !r.is_empty());
                                        if let Some(chunk) = reasoning_chunk {
                                            if !in_reasoning {
                                                out.push_str("</think>");
                                                in_reasoning = true;
                                            }
                                            out.push_str(chunk);
                                        }
                                        if let Some(content) =
                                            delta.get("content").and_then(|c| c.as_str())
                                            && !content.is_empty()
                                        {
                                            if in_reasoning {
                                                out.push_str("</think>\n");
                                                in_reasoning = false;
                                            }
                                            out.push_str(content);
                                        }
                                        if let Some(tool_calls) =
                                            delta.get("tool_calls").and_then(|t| t.as_array())
```

with:

```rust
                                        }

                                        push_delta_text(&mut out, &mut in_reasoning, delta);
                                        if let Some(tool_calls) =
                                            delta.get("tool_calls").and_then(|t| t.as_array())
```

**2. New helper — add after `stream_retry_backoff`.** Replace:

```rust
}

/// Whether a delta carries a real token (non-empty content, reasoning, or tool calls).
fn delta_carries_token(delta: &serde_json::Map<String, Value>) -> bool {
```

with:

```rust
}

/// Append one streamed delta's reasoning and content text to `out`. A run of
/// reasoning chunks is wrapped in `<think>` … `</think>\n`; the block closes
/// when content arrives (tool calls and end-of-stream close it at the call site).
fn push_delta_text(
    out: &mut String,
    in_reasoning: &mut bool,
    delta: &serde_json::Map<String, Value>,
) {
    let reasoning_chunk = delta
        .get("reasoning")
        .or_else(|| delta.get("reasoning_content"))
        .and_then(|r| r.as_str())
        .filter(|r| !r.is_empty());
    if let Some(chunk) = reasoning_chunk {
        if !*in_reasoning {
            out.push_str("<think>");
            *in_reasoning = true;
        }
        out.push_str(chunk);
    }
    if let Some(content) = delta.get("content").and_then(|c| c.as_str())
        && !content.is_empty()
    {
        if *in_reasoning {
            out.push_str("</think>\n");
            *in_reasoning = false;
        }
        out.push_str(content);
    }
}

/// Whether a delta carries a real token (non-empty content, reasoning, or tool calls).
fn delta_carries_token(delta: &serde_json::Map<String, Value>) -> bool {
```

**3. Test-module imports — add `push_delta_text`. The list is explicit; a missing name fails with `E0425`.** Replace:

```rust
    use super::{
        build_chat_body, convert_messages, delta_carries_token, drain_stream_with_retry,
        emit_tool_call_generic, is_retriable_transport, parse_openai_usage, render_openai_tools,
        select_timeout, should_retry_stall, stream_retry_backoff,
    };
    use crate::ai::SamplingParams;
```

with:

```rust
    use super::{
        build_chat_body, convert_messages, delta_carries_token, drain_stream_with_retry,
        emit_tool_call_generic, is_retriable_transport, parse_openai_usage, push_delta_text,
        render_openai_tools, select_timeout, should_retry_stall, stream_retry_backoff,
    };
    use crate::ai::SamplingParams;
```

**4. New test — before `delta_carries_token_with_non_empty_content`.** Replace:

```rust
    }

    #[test]
    fn delta_carries_token_with_non_empty_content() {
```

with:

```rust
    }

    #[test]
    fn push_delta_text_opens_reasoning_with_think_tag() {
        let mut out = String::new();
        let mut in_reasoning = false;
        let reasoning = json!({ "reasoning_content": "plan the edit" });
        let content = json!({ "content": "done" });
        push_delta_text(&mut out, &mut in_reasoning, reasoning.as_object().unwrap());
        push_delta_text(&mut out, &mut in_reasoning, content.as_object().unwrap());
        let open = out
            .find("<think>")
            .expect("reasoning must open with <think>");
        let close = out
            .find("</think>")
            .expect("reasoning must close with </think>");
        assert!(open < close, "<think> must precede </think>: {out:?}");
        assert!(!in_reasoning, "content must close the reasoning block");
    }

    #[test]
    fn delta_carries_token_with_non_empty_content() {
```

### `executor/src/config.rs`

**5. `ModelOverride` derive.** Replace:

```rust
/// in the `[models]` table (e.g. `[models."Qwen/Qwen3.6-27B-FP8"]`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModelOverride {
    pub task_tracking: Option<bool>,
```

with:

```rust
/// in the `[models]` table (e.g. `[models."Qwen/Qwen3.6-27B-FP8"]`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ModelOverride {
    pub task_tracking: Option<bool>,
```

**6. `ModelOverride` — accept and ignore the four removed rate keys.** Replace:

```rust
    pub novelty_distinct_floor: Option<usize>,
    pub novelty_action: Option<NoveltyAction>,
}

```

with:

```rust
    pub novelty_distinct_floor: Option<usize>,
    pub novelty_action: Option<NoveltyAction>,
    /// Rate keys from the removed pricing layer. Still accepted so existing
    /// configs load, and ignored.
    #[serde(skip_serializing)]
    pub input_per_mtok: Option<serde::de::IgnoredAny>,
    #[serde(skip_serializing)]
    pub output_per_mtok: Option<serde::de::IgnoredAny>,
    #[serde(skip_serializing)]
    pub cache_read_per_mtok: Option<serde::de::IgnoredAny>,
    #[serde(skip_serializing)]
    pub cache_creation_per_mtok: Option<serde::de::IgnoredAny>,
}

```

**7. New test — before `legacy_rate_keys_are_ignored`.** Replace:

```rust
    }

    #[test]
    fn legacy_rate_keys_are_ignored() {
```

with:

```rust
    }

    #[test]
    fn model_override_rejects_unknown_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[executor]
provider = "openai"
model = "m"
base_url = "http://localhost:1234/v1"

[models."m"]
thinkng = "disabled"
"#,
        )
        .unwrap();

        let err = Config::load(&path).unwrap_err().to_string();
        assert!(
            err.contains("thinkng"),
            "error must name the unknown key: {err}"
        );
    }

    #[test]
    fn legacy_rate_keys_are_ignored() {
```

### `mcp/src/runner.rs`

**8. Both `ModelOverride` test literals (there are exactly 2, identical) — shrink to `..Default::default()`.** Replace **both occurrences** of:

```rust
            rexymcp_executor::config::ModelOverride {
                temperature: Some(0.2),
                seed: None,
                task_tracking: None,
                max_tokens: None,
                enable_thinking: None,
                identical_call_threshold: None,
                verifier_persistence_threshold: None,
                runaway_output_bytes: None,
                empty_completion_threshold: None,
                gate_feedback_repeat_threshold: None,
                thinking: None,
                oscillation_window: None,
                oscillation_distinct_max: None,
                output_window: None,
                output_window_bytes: None,
                read_only_stall_threshold: None,
                novelty_window: None,
                novelty_distinct_floor: None,
                novelty_action: None,
            },
        );
```

with:

```rust
            rexymcp_executor::config::ModelOverride {
                temperature: Some(0.2),
                ..Default::default()
            },
        );
```

**Must NOT:**

- Add `deny_unknown_fields` to any struct other than `ModelOverride`.
- Implement the `reasoning_content` round-trip (phase-02).
- Change `thinking` or `enable_thinking` behaviour.
- Edit any `rexymcp.toml`.
- Run `cargo fmt --all` (the writing form). If formatting is needed, run
  `rustfmt --edition 2024` on the three files this phase touches.

## Acceptance criteria

- [ ] A `[models."m"]` table containing `thinkng = "disabled"` fails to load with
      an error containing `thinkng`.
- [ ] `thinking = "disabled"` and `output_per_mtok = 1.0` in `[models."m"]` still load.
- [ ] `grep -c 'push_str("<think>")' executor/src/ai/backends/openai.rs` → **1**.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

The two tests are in blocks 4 and 7. Before applying blocks 1, 2, 5 and 6's
fixes, the tests alone do not compile or fail; the architect's red run had both
failing with the extraction in place and the old opener. Write the tests first
and quote their failure in the Update Log.

**Finish condition:**

- `cargo test -p rexymcp-executor push_delta_text` → **1 passed**
- `cargo test -p rexymcp-executor model_override_rejects_unknown_key` → **1 passed**
- `cargo test -p rexymcp-executor legacy_rate_keys_are_ignored` → **1 passed**
- Full `cargo test` → **729 / 2 / 1220 passed**

Paste the `test result:` lines.

## End-to-end verification

In a `### Update — YYYY-MM-DD HH:MM (end-to-end verification)` entry, paste the
output of:

```bash
cargo build -q -p rexymcp
T=$(mktemp -d)
base='[project]\nid = "x"\n\n[executor]\nprovider = "openai"\nmodel = "m"\nbase_url = "http://127.0.0.1:9/v1"\n\n'
for kv in 'thinkng = "disabled"' 'thinking = "disabled"' 'output_per_mtok = 1.0'; do
  printf "$base[models.\"m\"]\n$kv\n" > "$T/c.toml"
  echo "== $kv"; ./target/debug/rexymcp health --config "$T/c.toml" 2>&1 | grep -E "unknown field|unreachable" | cut -c1-120
done
```

Expected: the first prints `unknown field \`thinkng\`, expected one of …`; the
other two print `unreachable: http://127.0.0.1:9/v1` (the config loaded; the
dummy endpoint is closed).

## Authorizations

- `executor/src/config.rs`, `executor/src/ai/backends/openai.rs`,
  `mcp/src/runner.rs` as specified.

## Out of scope

- The `reasoning_content` round-trip (phase-02).
- `deny_unknown_fields` on any other struct.

## Update Log

<!-- entries appended below this line -->
