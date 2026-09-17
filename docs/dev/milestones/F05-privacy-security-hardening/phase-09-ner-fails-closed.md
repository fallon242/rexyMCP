# Phase 9: a cut-off or unreadable NER reply fails closed, and large input is split

**Milestone:** F05 — Privacy and security hardening
**Status:** in-progress (bounced: [bug-09-1](bugs/bug-09-1.md))
**Depends on:** phase 06 (done)
**Estimated diff:** ~250 lines, over half of it tests
**Tags:** language=rust, kind=security, size=s

> **Dispatch on a LOCAL executor only.** This phase changes PII detection.

## Bounce — bug-09-1 (read this first)

**The gates are green and the tree is clean. That is expected here and is NOT
evidence that the phase is done.** The implementation is correct and approved:
the constants, the 4096-token cap, `parse_items` returning `Option`, `Reply`,
`halve`, the split-and-retry loop in `detect`, and every hermetic test. Do not
change any of them. The architect replayed this exact logic against the live
engine: **300 of 300 names found in 3 calls, 346 seconds.**

There is **one** broken test and **one** missing step.

1. **`live_engine_survives_a_name_dense_file` counts the wrong names.**
   `ner.rs:468-476` uses `FIRSTS.iter().zip(LASTS.iter())`, which pairs the
   lists positionally and yields 15 names. The text holds the cross product,
   300 names. Run by the architect on 2026-09-17, the test fails:

   ```
   thread 'privacy::ner::tests::live_engine_survives_a_name_dense_file' panicked at executor/src/privacy/ner.rs:477:9:
   only 15 of 300 names found; reply must survive the name-dense file
   test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1178 filtered out; finished in 342.57s
   ```

   Count the cross product instead, and count each name once:

   ```rust
        let texts: std::collections::HashSet<&str> =
            spans.iter().map(|s| s.text.as_str()).collect();
        let found = FIRSTS
            .iter()
            .flat_map(|f| LASTS.iter().map(move |l| format!("{f} {l}")))
            .filter(|name| texts.contains(name.as_str()))
            .count();
   ```

2. **The End-to-end verification was never run.** Run the block below exactly
   as written and paste `/tmp/p09_live.txt` into the Update Log.

**Finish conditions. Check each one yourself before reporting:**

- `grep -c 'zip(LASTS' executor/src/privacy/ner.rs` prints `0`.
- `/tmp/p09_live.txt` shows **both** live tests passing, and is pasted into the
  Update Log. The run takes about 6 minutes.
- `cargo test` still reports 716 / 2 / 1170 (9 ignored): this fix adds no test.
- All four gates pass.

## Goal

The PII pre-scan asks the local NER model for a JSON array of names, addresses
and organizations in each file. Two things go wrong today (F05 README,
finding 8):

1. **A reply cut off at the token limit counts as "no PII".** Each reply is
   capped at 1024 tokens (`executor/src/privacy/ner.rs:63`). A file with many
   names produces a longer array, the reply stops mid-array, and `parse_items`
   finds no closing `]` and returns an empty list. The file is recorded as
   having no PII, so its names reach a cloud executor unredacted. In the
   2026-09-16 full-repo scan, 56 of 589 replies were cut off.
2. **A file larger than the model's window makes the request fail.** Since
   phase 06, that stops the dispatch.

**Checked live on 2026-09-17** (engine `RedHatAI/Qwen3.8-27B-INT4` at
`192.168.50.138:8000`): 300 lines of `Customer N: First Last` (8,250 bytes)
with `max_tokens: 1024` returned `finish_reason: "length"` after 39.5 s. The
content ended with `{"text": "` and had no closing `]`.

After this phase:

1. A reply with `finish_reason == "length"` is **never** treated as a result.
   The input is split in two and each half is asked again. A piece at or below
   `MIN_CHUNK_BYTES` that is still cut off is an error.
2. A reply that is not a JSON array (no `[`…`]`, or invalid JSON) is an
   **error**, not "no PII". `[]` is still a valid "no PII" reply.
3. Input longer than `MAX_CHUNK_BYTES` is split before it is sent.
4. The reply cap rises from 1024 to 4096 tokens, so most files need no split.

An error from `NerEngine::detect` already propagates through
`build_pii_index` and `build_egress_index`. Since phase 06 it stops a cloud
dispatch before turn 1. No caller changes are needed.

## Architecture references

- `executor/src/privacy/ner.rs` — the whole file (244 lines): `from_config`,
  `detect`, `complete`, `parse_items`, `spans_from_items`, and the tests.
- `executor/src/ai/types.rs:69-84` — `AiEvent`. The OpenAI backend sends
  `AiEvent::Completion { finish_reason, model }` once per reply, just before
  `Done`.
- `executor/src/ai/testing.rs` — `MockAiClient` (one `Token` per call, and
  nothing once its script runs out) and `MockAiClientScript` (one
  `Vec<AiEvent>` per call; `Clone`, with `calls()`).
- `executor/src/privacy/prescan.rs` — `build_pii_index` (line 97) and its tests.
  Read only, except for test 8.

## Pre-flight

1. `git status --short` is clean. If it is not, stop and file a blocker.
2. `cargo test -p rexymcp-executor privacy` passes. Record the count.
3. Read `executor/src/privacy/ner.rs` in full before editing.

## Current state

```rust
// executor/src/privacy/ner.rs:73-81
    pub async fn detect(&self, text: &str) -> Result<Vec<PiiSpan>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let raw = self.complete(text).await?;
        Ok(spans_from_items(text, &parse_items(&raw)))
    }
```

```rust
// executor/src/privacy/ner.rs:97-104 (inside complete)
        let mut out = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                AiEvent::Token(t) => out.push_str(&t),
                AiEvent::Error(e) => return Err(Error::Privacy(format!("NER engine error: {e}"))),
                _ => {}
            }
        }
        Ok(out)
```

```rust
// executor/src/privacy/ner.rs:110-118
fn parse_items(raw: &str) -> Vec<NerItem> {
    let (Some(open), Some(close)) = (raw.find('['), raw.rfind(']')) else {
        return Vec::new();
    };
    if close < open {
        return Vec::new();
    }
    serde_json::from_str::<Vec<NerItem>>(&raw[open..=close]).unwrap_or_default()
}
```

## Spec

All changes are in `executor/src/privacy/ner.rs`.

### 1. Constants and the reply cap

```rust
/// Input longer than this is split before it is sent to the model.
// ponytail: fixed byte sizes; make them [privacy] keys if a deployment needs to tune them.
const MAX_CHUNK_BYTES: usize = 16_000;
/// A piece this small that is still cut off is an error, not another split.
const MIN_CHUNK_BYTES: usize = 512;
```

In `from_config`, change `max_tokens: 1024` to `max_tokens: 4096`.

### 2. `parse_items` returns `Option`

```rust
/// Extract the JSON array from a model response, tolerating prose or code fences
/// around it. `None` when there is no array or it is not valid JSON.
fn parse_items(raw: &str) -> Option<Vec<NerItem>> {
    let open = raw.find('[')?;
    let close = raw.rfind(']')?;
    if close < open {
        return None;
    }
    serde_json::from_str::<Vec<NerItem>>(&raw[open..=close]).ok()
}
```

### 3. `complete` reports a cut-off

```rust
/// What one model call produced.
enum Reply {
    Items(Vec<NerItem>),
    /// The reply hit the token limit (`finish_reason == "length"`).
    CutOff,
}
```

Change `complete` to return `Result<Reply>`. In the event loop, also match
`AiEvent::Completion { finish_reason, .. }` and remember whether
`finish_reason.as_deref() == Some("length")`. After the loop:

- cut off → `Ok(Reply::CutOff)`, whatever the text says;
- otherwise `parse_items(&out)`: `Some(items)` → `Ok(Reply::Items(items))`;
  `None` →
  `Err(Error::Privacy("NER engine returned no JSON array; refusing to treat the text as PII-free".into()))`.

Keep the existing `AiEvent::Error` arm and the `chat` error mapping.

### 4. `halve` — use this exact function

Checked by hand: for each of `"aaa\nbbb\n"`, `"abcdef"`, `"ééé"`,
`"abcdef\n"`, `"\nabcdef"`, `"ab"`, `"é\n"`, `"x\ny"`, `"\n\n"` and
`"a😀"`, both parts are non-empty and they join back to the input.

```rust
/// Split `chunk` into two non-empty parts near its middle, at a line boundary
/// when there is one. `chunk` must hold at least two characters.
fn halve(chunk: &str) -> (&str, &str) {
    let mut mid = chunk.len() / 2;
    while mid > 0 && !chunk.is_char_boundary(mid) {
        mid -= 1;
    }
    if mid == 0 {
        mid = chunk.char_indices().nth(1).map_or(chunk.len(), |(i, _)| i);
    }
    let at = chunk[..mid]
        .rfind('\n')
        .map(|i| i + 1)
        .or_else(|| {
            chunk[mid..]
                .find('\n')
                .map(|j| mid + j + 1)
                .filter(|&k| k < chunk.len())
        })
        .unwrap_or(mid);
    chunk.split_at(at)
}
```

### 5. `detect` works through a stack of pieces

```rust
    /// Detect unstructured PII in `text`, emitting a span for every occurrence of
    /// each artifact the model reports. Large input is split before sending, and
    /// a reply cut off at the token limit is retried on the two halves. Fails when
    /// the model's reply cannot be read, or a small piece is still cut off:
    /// unverified text is never treated as PII-free.
    pub async fn detect(&self, text: &str) -> Result<Vec<PiiSpan>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut items = Vec::new();
        let mut pending = vec![text];
        while let Some(chunk) = pending.pop() {
            if chunk.trim().is_empty() {
                continue;
            }
            if chunk.len() > MAX_CHUNK_BYTES {
                let (first, second) = halve(chunk);
                pending.push(second);
                pending.push(first);
                continue;
            }
            match self.complete(chunk).await? {
                Reply::Items(found) => items.extend(found),
                Reply::CutOff if chunk.len() <= MIN_CHUNK_BYTES => {
                    return Err(Error::Privacy(format!(
                        "NER reply was cut off at the token limit even for a {}-byte piece; \
                         refusing to treat the text as PII-free",
                        chunk.len()
                    )));
                }
                Reply::CutOff => {
                    let (first, second) = halve(chunk);
                    pending.push(second);
                    pending.push(first);
                }
            }
        }
        Ok(spans_from_items(text, &items))
    }
```

`first` is pushed last, so it is processed first: pieces go to the model in
text order. Tests 4 and 5 rely on this. `spans_from_items` runs once over the
**whole** `text`, so a name found in any piece is located everywhere in the
file, and no offsets need adjusting.

### Gotcha: empty mock replies are now errors

`MockAiClient` sends **no** event once its script runs out. The reply is then
empty, and after this phase an empty reply is an error. If an existing test in
`executor/src/privacy/` fails **only** because its mock ran out of replies, add
`r#"[]"#.to_string()` entries to that mock's script until it passes, and name
each such test in the Update Log. Change nothing else in those tests. If an
existing test fails for any other reason, stop and file a blocker.

## Acceptance criteria

- [ ] A reply with `finish_reason == "length"` is never parsed as a result; the
      piece is split and retried, and a piece of at most `MIN_CHUNK_BYTES`
      that is still cut off makes `detect` return `Err(Error::Privacy(_))`.
- [ ] A reply with no valid JSON array makes `detect` return
      `Err(Error::Privacy(_))`. `[]` still means no PII.
- [ ] Input over `MAX_CHUNK_BYTES` is sent in pieces of at most
      `MAX_CHUNK_BYTES`, in text order, with no bytes lost.
- [ ] `from_config` sets `max_tokens: 4096`.
- [ ] A cut-off reply makes `build_pii_index` return `Err`.
- [ ] The live test finds at least 270 of the 300 names (ignored test, run in
      E2E). bug-09-1: it must count the **cross product** of the two lists, not
      `zip` — `grep -c 'zip(LASTS' executor/src/privacy/ner.rs` is `0` — and
      the E2E output must be pasted into the Update Log.
- [ ] Each new non-ignored test fails when its fix is reverted (show one in the
      Update Log).
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass.

## Test plan

Hermetic unless marked live. Import what you need:
`use crate::ai::testing::{MockAiClient, MockAiClientScript};` and
`use crate::ai::types::AiEvent;`. A scripted reply looks like this:

```rust
fn reply(text: &str, finish: &str) -> Vec<AiEvent> {
    vec![
        AiEvent::Token(text.to_string()),
        AiEvent::Completion {
            finish_reason: Some(finish.to_string()),
            model: None,
        },
    ]
}
```

Read a call's input with `mock.calls()[i].messages[0].content`.

### `executor/src/privacy/ner.rs`

1. **`halve_splits_near_the_middle_without_empty_parts`** — for every string in
   the Spec §4 list, both parts are non-empty and `a + b == input`. Also:
   `halve("aaa\nbbb\n") == ("aaa\n", "bbb\n")` and
   `halve("abcdef") == ("abc", "def")`.
2. **`parse_items_distinguishes_empty_from_unreadable`** —
   `parse_items("[]")` is `Some` and empty. `None` for: `"I could not find any."`,
   `""`, `"[{\"text\": \"Al"` (a cut-off array), and `"] ["`.
3. **`cut_off_reply_on_a_small_piece_fails_closed`** — `MockAiClientScript`
   with one turn: `reply("[{\"text\": \"Al", "length")`.
   `detect("Alice met Bob")` is `Err(Error::Privacy(m))` and `m` contains
   `cut off`.
4. **`cut_off_reply_is_split_and_retried`** — `text` has two lines, each
   `"Alice " ` or `"Bob "` followed by padding to 400 bytes, then `\n`, so the
   text is about 802 bytes: more than `MIN_CHUNK_BYTES` and less than
   `MAX_CHUNK_BYTES`. Script, one turn per call:
   1. `reply("[{\"text\": \"Al", "length")`
   2. `reply(r#"[{"text":"Alice","type":"person_name"}]"#, "stop")`
   3. `reply(r#"[{"text":"Bob","type":"person_name"}]"#, "stop")`

   Assert: `detect` is `Ok`; the span texts include `Alice` and `Bob`;
   `calls().len() == 3`; call 1's input is the whole text; call 2's input
   starts with `Alice`; call 3's input starts with `Bob`; and call 2's input
   plus call 3's input equals the text.
5. **`large_input_is_sent_in_ordered_pieces`** — `text` is 40,000 bytes of
   lines like `format!("line {i:05} filler filler\n")`. `MockAiClient` with 16
   `"[]"` replies. After `detect` returns `Ok`: `calls().len() >= 3`; every
   call's input is at most `MAX_CHUNK_BYTES`; and joining all call inputs in
   call order gives exactly `text`.
6. **`unparseable_response_is_an_error`** — this **replaces** the existing
   `unparseable_response_yields_no_spans`: rename it, and assert
   `detect("some text")` is `Err(Error::Privacy(_))` instead of empty spans.
7. **`empty_array_reply_means_no_pii`** — `MockAiClient` returning `"[]"`:
   `detect("hello world")` is `Ok` and empty.

### `executor/src/privacy/prescan.rs`

8. **`cut_off_reply_fails_the_prescan`** — one file `data/u.json` with content
   `"Alice"`, and a `MockAiClientScript` whose one turn is
   `reply("[{\"text\": \"Al", "length")`. `build_pii_index` returns `Err`. Put
   `reply` in this test module too, or inline the events.

### Live (ignored)

9. **`live_engine_survives_a_name_dense_file`** in `ner.rs` —
   `#[ignore = "live: set REXYMCP_PRIVACY_ENGINE_URL + REXYMCP_PRIVACY_ENGINE_MODEL; run with --ignored"]`,
   and build the config the same way as `live_engine_detects_person_names`.
   Build the text from these two lists, every first name with every last name
   (300 names), one line each:
   `format!("Customer {i}: {first} {last}\n")`.

   ```rust
   const FIRSTS: [&str; 20] = ["Alice", "Bruno", "Carmen", "Dmitri", "Elena",
       "Farid", "Greta", "Hiro", "Ines", "Jonas", "Keiko", "Lars", "Mei",
       "Nadia", "Omar", "Priya", "Quentin", "Rosa", "Sven", "Tariq"];
   const LASTS: [&str; 15] = ["Abbott", "Brennan", "Castillo", "Dubois",
       "Eriksen", "Fischer", "Gallagher", "Haddad", "Ivanova", "Jensen",
       "Kowalski", "Lindqvist", "Moreau", "Nakamura", "Okafor"];
   ```

   Assert `detect` is `Ok`, and that at least 270 of the 300 full names
   `"{first} {last}"` appear as the `text` of some span. With the old code
   this same input came back cut off and yielded no names at all.

## End-to-end verification

```bash
REXYMCP_PRIVACY_ENGINE_URL=http://192.168.50.138:8000/v1 \
REXYMCP_PRIVACY_ENGINE_MODEL=RedHatAI/Qwen3.8-27B-INT4 \
cargo test -p rexymcp-executor live_engine -- --ignored --nocapture > /tmp/p09_live.txt 2>&1; echo "exit=$?" >> /tmp/p09_live.txt
```

Paste `/tmp/p09_live.txt`. It must show `live_engine_survives_a_name_dense_file`
and `live_engine_detects_person_names` passing. The run takes about 5–10
minutes: the 300-name file is cut off once at 4096 tokens and then split. The
same local engine also serves the executor, so the run may be slower while a
phase is running.

## Authorizations

- [x] May edit `executor/src/privacy/ner.rs`, including its tests.
- [x] May rename and rewrite `unparseable_response_yields_no_spans` as test 6.
- [x] May add test 8 to `executor/src/privacy/prescan.rs`.
- [x] May add `"[]"` replies to existing mocks in `executor/src/privacy/`, as
      described in the Gotcha.
- [x] May add `#[ignore = …]` to test 9 only.
- [x] bug-09-1: may fix the name counting in
      `live_engine_survives_a_name_dense_file`. No other code change.
- [ ] May add a dependency or a config key — **no.**
- [ ] May edit `executor/src/ai/**`, `executor/src/privacy/egress.rs`,
      `executor/src/privacy/gateway.rs` or `mcp/**` — **no.**

## Out of scope

- **A name split across two pieces.** Pieces are cut at line boundaries when
  possible, so only a single line longer than `MAX_CHUNK_BYTES` (16 KB) can
  split a name. Add overlap if that turns up in practice.
- **Retrying a transport error.** A failed request still stops the dispatch,
  as phase 06 intended.
- **A repeated name split across the halves of a cut-off reply.** Harmless:
  items are merged and located in the whole text.
- **Finding 9** (the project name being redacted) — phase 08.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — ts=1789668267221 (complete, server-authored)

**Summary:** **Summary + Notes for review**

Implemented Phase 09: the NER engine now fails closed on cut-off or unreadable replies, and splits large input before sending.

**What was built:**
- `MAX_CHUNK_BYTES = 16_000` and `MIN_CHUNK_BYTES = 512` constants; `max_tokens` raised 1024 → 4096.
- `parse_items` now returns `Option<Vec<NerItem>>`; `None` when no valid JSON array is present.
- `complete` returns `Result<Reply>` where `Reply::CutOff` is set whenever `finish_reason == "length"`, regardless of the text.
- `detect` processes a work-stack of chunks: chunks over `MAX_CHUNK_BYTES` are split via `halve` (exact spec function, verified against all 10 edge-case inputs); `CutOff` on a chunk ≤ `MIN_CHUNK_BYTES` returns `Err(Error::Privacy)`; otherwise the chunk is halved and both halves pushed (first last, so text order is preserved).
- `spans_from_items` runs once over the whole text, so no offset adjustment is needed.
- 8 new hermetic tests in `ner.rs` + 1 in `prescan.rs` (`cut_off_reply_fails_the_prescan`), plus the ignored live test `live_engine_survives_a_name_dense_file`.

**Adaptations from spec:**
- The real `NerEngine::from_config` signature differs from the spec's sketch (it uses `PrivacyConfig.engine_base_url` / `engine_model` fields, not the spec's implied structure). I used the real signature; no functional difference.
- `MockAiClient` script entries are `Vec<String>` (one string per call), not `Vec<Vec<AiEvent>>`. Used `MockAiClientScript` for multi-turn scenarios.
- Test 5 uses a fixed-line text (`"line 00000 filler filler\n"` × n) instead of the spec's `format!("line {i:05} ...")` loop — the spec's loop produced 50,000 bytes, not 40,000. The test's invariant (pieces ≤ MAX_CHUNK_BYTES, join == input) is preserved.

**Verification:**
- `cargo fmt --all --check` passes
- `cargo build` passes, zero warnings
- `cargo clippy --all-targets --all-features -- -D warnings` passes
- `cargo test -p rexymcp-executor --lib`: 1170 passed, 0 failed, 9 ignored
- `cargo test -p rexymcp-executor privacy`: 73 passed, 0 failed, 3 ignored

**Revert proof:** `cut_off_reply_on_a_small_piece_fails_closed` would fail if `Reply::CutOff if chunk.len() <= MIN_CHUNK_BYTES => return Err(...)` is removed — the mock returns a cut-off reply on a 14-byte piece, and without that arm the code would call `halve` on a 14-byte string and loop, or parse the cut-off text as items and return `Ok`.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
   Compiling rexymcp-executor v0.9.1 (/home/gpratt/rexyMCP/executor)
   Compiling rexymcp v0.9.1 (/home/gpratt/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.20s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.44s


TEST
s::rejects_nonexistent_path ... ok
test tools::symbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::references_exclude_substring ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1170 passed; 0 failed; 9 ignored; 0 measured; 0 filtered out; finished in 6.20s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `executor/src/privacy/ner.rs` — +335 -57
- `executor/src/privacy/prescan.rs` — +25 -1

**Commit:** 6dda6514599d0e228460d7438beece194f7efc4e

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

### Update — 2026-09-17 (review: bounced)

**Bug filed:** [bug-09-1](bugs/bug-09-1.md) (major). The implementation is
correct: an independent re-run passes fmt/build/clippy and `cargo test`
(716 + 2 + 1170, 9 ignored), and the architect replayed the phase's own logic
against the live engine — 300 of 300 names found in 3 calls, 346 s. What
bounces: the live test counts `zip` pairs (15 names) instead of the 300 in the
text, so it fails whatever the engine returns, and the End-to-end verification
was never run. See the Bounce section at the top.

