# Phase 9: a cut-off or unreadable NER reply fails closed, and large input is split

**Milestone:** F05 — Privacy and security hardening
**Status:** todo
**Depends on:** phase 06 (done)
**Estimated diff:** ~250 lines, over half of it tests
**Tags:** language=rust, kind=security, size=s

> **Dispatch on a LOCAL executor only.** This phase changes PII detection.

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
      E2E).
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
