//! Qwen-backed NER for unstructured PII (person names, street addresses,
//! organizations) — the artifacts deterministic detectors cannot catch. The
//! model runs locally (the `[privacy]` engine endpoint), so detection never
//! leaves the LAN. Best-effort by nature: a miss is a leak, so the gateway pairs
//! this with the deterministic detectors and biases toward over-matching.

use std::time::Duration;

use serde::Deserialize;
use tokio::sync::mpsc;

use crate::ai::types::{AiEvent, Message};
use crate::ai::{AiClient, OpenAiClient, SamplingParams};
use crate::config::PrivacyConfig;
use crate::error::{Error, Result};

use super::{PiiKind, PiiSpan};

/// Input longer than this is split before it is sent to the model.
// ponytail: fixed byte sizes; make them [privacy] keys if a deployment needs to tune them.
const MAX_CHUNK_BYTES: usize = 16_000;
/// A piece this small that is still cut off is an error, not another split.
const MIN_CHUNK_BYTES: usize = 512;

const SYSTEM_PROMPT: &str = "You are a PII detector. Find every person name, \
street address, and organization name in the user's text. Return ONLY a JSON \
array; each element is {\"text\": <the exact substring>, \"type\": <\"person_name\" \
| \"street_address\" | \"organization\">}. Copy each text exactly as it appears \
in the input. If there is none, return []. No prose, no markdown, no code fences.";

#[derive(Debug, Deserialize)]
struct NerItem {
    text: String,
    #[serde(rename = "type", default)]
    label: String,
}

/// NER over a local chat model. Wraps an [`AiClient`] so tests inject a mock.
pub struct NerEngine {
    client: Box<dyn AiClient>,
}

impl NerEngine {
    pub fn new(client: Box<dyn AiClient>) -> Self {
        Self { client }
    }

    /// Build an engine from `[privacy]` config: the local Qwen endpoint with
    /// thinking disabled (a reasoning model otherwise spends its whole token
    /// budget on `reasoning_content` and returns empty text). Errors if the
    /// endpoint or model is unset.
    pub fn from_config(cfg: &PrivacyConfig) -> Result<Self> {
        let base_url = cfg
            .engine_base_url
            .clone()
            .ok_or_else(|| Error::Privacy("privacy.engine_base_url is unset".into()))?;
        let model = cfg
            .engine_model
            .clone()
            .ok_or_else(|| Error::Privacy("privacy.engine_model is unset".into()))?;
        let client = OpenAiClient::new(
            String::new(),
            model,
            base_url,
            Duration::from_secs(600),
            Duration::from_secs(240),
            SamplingParams {
                temperature: Some(0.0),
                seed: None,
                max_tokens: 4096,
                enable_thinking: false,
                thinking: None,
            },
        );
        Ok(Self::new(Box::new(client)))
    }

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

    /// One model call on `text`, with the reply classified before any parsing.
    async fn complete(&self, text: &str) -> Result<Reply> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let messages = vec![Message {
            role: "user".to_string(),
            content: text.to_string(),
            tool_calls: None,
            tool_results: None,
            turn: None,
        }];
        self.client
            .chat(SYSTEM_PROMPT, messages, tx, None)
            .await
            .map_err(|e| Error::Privacy(format!("NER engine call failed: {e}")))?;
        let mut out = String::new();
        let mut cut_off = false;
        while let Some(event) = rx.recv().await {
            match event {
                AiEvent::Token(t) => out.push_str(&t),
                AiEvent::Completion { finish_reason, .. } => {
                    if finish_reason.as_deref() == Some("length") {
                        cut_off = true;
                    }
                }
                AiEvent::Error(e) => return Err(Error::Privacy(format!("NER engine error: {e}"))),
                _ => {}
            }
        }
        if cut_off {
            return Ok(Reply::CutOff);
        }
        match parse_items(&out) {
            Some(items) => Ok(Reply::Items(items)),
            None => Err(Error::Privacy(
                "NER engine returned no JSON array; refusing to treat the text as PII-free".into(),
            )),
        }
    }
}

/// What one model call produced.
enum Reply {
    Items(Vec<NerItem>),
    /// The reply hit the token limit (`finish_reason == "length"`).
    CutOff,
}

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

fn spans_from_items(text: &str, items: &[NerItem]) -> Vec<PiiSpan> {
    let mut spans = Vec::new();
    for item in items {
        if item.text.is_empty() {
            continue;
        }
        let kind = kind_from_label(&item.label);
        let mut from = 0;
        while let Some(rel) = text[from..].find(&item.text) {
            let start = from + rel;
            let end = start + item.text.len();
            spans.push(PiiSpan {
                start,
                end,
                kind,
                text: item.text.clone(),
            });
            from = end;
        }
    }
    spans
}

/// Map a model label to a `PiiKind`. An unrecognized-but-flagged artifact falls
/// back to `PersonName` — still anonymized (the safe, over-matching direction)
/// rather than leaked.
fn kind_from_label(label: &str) -> PiiKind {
    let l = label.to_ascii_lowercase();
    if l.contains("email") {
        PiiKind::Email
    } else if l.contains("phone") {
        PiiKind::Phone
    } else if l.contains("address") {
        PiiKind::StreetAddress
    } else if l.contains("org") {
        PiiKind::Org
    } else {
        PiiKind::PersonName
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testing::{MockAiClient, MockAiClientScript};

    /// Live local NER engine. Not in CI: needs a reachable engine plus the
    /// REXYMCP_PRIVACY_ENGINE_URL / REXYMCP_PRIVACY_ENGINE_MODEL env vars.
    #[tokio::test]
    #[ignore = "live: set REXYMCP_PRIVACY_ENGINE_URL + REXYMCP_PRIVACY_ENGINE_MODEL; run with --ignored"]
    async fn live_engine_detects_person_names() {
        let cfg = PrivacyConfig {
            enabled: true,
            engine_base_url: Some(
                std::env::var("REXYMCP_PRIVACY_ENGINE_URL")
                    .expect("set REXYMCP_PRIVACY_ENGINE_URL to run this live test"),
            ),
            engine_model: Some(
                std::env::var("REXYMCP_PRIVACY_ENGINE_MODEL")
                    .expect("set REXYMCP_PRIVACY_ENGINE_MODEL to run this live test"),
            ),
            ..Default::default()
        };
        let ner = NerEngine::from_config(&cfg).unwrap();
        let spans = ner
            .detect("John Smith met Maria Gonzalez at the downtown office")
            .await
            .unwrap();
        let texts: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        assert!(
            texts.iter().any(|t| t.contains("John")),
            "expected John Smith, got {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("Maria")),
            "expected Maria Gonzalez, got {texts:?}"
        );
    }

    fn reply(text: &str, finish: &str) -> Vec<AiEvent> {
        vec![
            AiEvent::Token(text.to_string()),
            AiEvent::Completion {
                finish_reason: Some(finish.to_string()),
                model: None,
            },
        ]
    }

    #[test]
    fn halve_splits_near_the_middle_without_empty_parts() {
        for input in [
            "aaa\nbbb\n",
            "abcdef",
            "ééé",
            "abcdef\n",
            "\nabcdef",
            "ab",
            "é\n",
            "x\ny",
            "\n\n",
            "a😀",
        ] {
            let (a, b) = halve(input);
            assert!(!a.is_empty(), "{input:?}: empty first part");
            assert!(!b.is_empty(), "{input:?}: empty second part");
            assert_eq!(format!("{a}{b}"), input, "{input:?}: parts do not rejoin");
        }
        assert_eq!(halve("aaa\nbbb\n"), ("aaa\n", "bbb\n"));
        assert_eq!(halve("abcdef"), ("abc", "def"));
    }

    #[test]
    fn parse_items_distinguishes_empty_from_unreadable() {
        let empty = parse_items("[]");
        assert!(empty.is_some(), "[] must parse");
        assert!(empty.as_ref().unwrap().is_empty(), "[] must be empty");
        for raw in ["I could not find any.", "", "[{\"text\": \"Al", "] ["] {
            assert!(parse_items(raw).is_none(), "{raw:?} must not parse");
        }
    }

    #[tokio::test]
    async fn cut_off_reply_on_a_small_piece_fails_closed() {
        let mock = MockAiClientScript::new(vec![reply("[{\"text\": \"Al", "length")]);
        let ner = NerEngine::new(Box::new(mock));
        let err = ner.detect("Alice met Bob").await.unwrap_err();
        match err {
            Error::Privacy(m) => assert!(m.contains("cut off"), "{m}"),
            other => panic!("expected Privacy error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cut_off_reply_is_split_and_retried() {
        let pad = "x".repeat(400 - "Alice ".len());
        let text = format!("Alice {pad}\nBob {pad}\n");
        let mock = MockAiClientScript::new(vec![
            reply("[{\"text\": \"Al", "length"),
            reply(r#"[{"text":"Alice","type":"person_name"}]"#, "stop"),
            reply(r#"[{"text":"Bob","type":"person_name"}]"#, "stop"),
        ]);
        let ner = NerEngine::new(Box::new(mock.clone()));
        let spans = ner.detect(&text).await.unwrap();
        let texts: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        assert!(texts.contains(&"Alice"), "Alice missing: {texts:?}");
        assert!(texts.contains(&"Bob"), "Bob missing: {texts:?}");
        let calls = mock.calls();
        assert_eq!(calls.len(), 3, "expected 3 calls");
        assert_eq!(
            calls[0].messages[0].content, text,
            "call 1 must be the whole text"
        );
        assert!(
            calls[1].messages[0].content.starts_with("Alice"),
            "call 2 starts with first half"
        );
        assert!(
            calls[2].messages[0].content.starts_with("Bob"),
            "call 3 starts with second half"
        );
        assert_eq!(
            format!(
                "{}{}",
                calls[1].messages[0].content, calls[2].messages[0].content
            ),
            text,
            "the two halves must rejoin to the text"
        );
    }

    #[tokio::test]
    async fn large_input_is_sent_in_ordered_pieces() {
        let line = "line 00000 filler filler\n";
        let n = (40_000 / line.len()) + 1;
        let text = vec![line; n].concat();
        assert!(text.len() > 40_000);
        let replies = vec!["[]".to_string(); 16];
        let mock = MockAiClient::new(replies);
        let ner = NerEngine::new(Box::new(mock.clone()));
        let spans = ner.detect(&text).await.unwrap();
        assert!(spans.is_empty());
        let calls = mock.calls();
        assert!(
            calls.len() >= 3,
            "expected at least 3 calls, got {}",
            calls.len()
        );
        let mut joined = String::new();
        for call in &calls {
            let len = call.messages[0].content.len();
            assert!(
                len <= MAX_CHUNK_BYTES,
                "piece of {len} bytes exceeds the cap"
            );
            joined.push_str(&call.messages[0].content);
        }
        assert_eq!(joined, text, "pieces must join in order back to the input");
    }

    #[tokio::test]
    async fn unparseable_response_is_an_error() {
        let mock = MockAiClient::new(vec!["I could not find any names.".to_string()]);
        let ner = NerEngine::new(Box::new(mock));
        let err = ner.detect("some text").await.unwrap_err();
        match err {
            Error::Privacy(m) => assert!(m.contains("no JSON array"), "{m}"),
            other => panic!("expected Privacy error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn empty_array_reply_means_no_pii() {
        let mock = MockAiClient::new(vec!["[]".to_string()]);
        let ner = NerEngine::new(Box::new(mock));
        let spans = ner.detect("hello world").await.unwrap();
        assert!(spans.is_empty());
    }

    /// Live: a name-dense file whose JSON reply runs long. With the 1024-token
    /// reply cap the reply was cut off mid-array and every name was lost; with
    /// 4096 tokens plus split-and-retry the reply must survive.
    #[tokio::test]
    #[ignore = "live: set REXYMCP_PRIVACY_ENGINE_URL + REXYMCP_PRIVACY_ENGINE_MODEL; run with --ignored"]
    async fn live_engine_survives_a_name_dense_file() {
        let cfg = PrivacyConfig {
            enabled: true,
            engine_base_url: Some(
                std::env::var("REXYMCP_PRIVACY_ENGINE_URL")
                    .expect("set REXYMCP_PRIVACY_ENGINE_URL to run this live test"),
            ),
            engine_model: Some(
                std::env::var("REXYMCP_PRIVACY_ENGINE_MODEL")
                    .expect("set REXYMCP_PRIVACY_ENGINE_MODEL to run this live test"),
            ),
            ..Default::default()
        };
        let ner = NerEngine::from_config(&cfg).unwrap();

        const FIRSTS: [&str; 20] = [
            "Alice", "Bruno", "Carmen", "Dmitri", "Elena", "Farid", "Greta", "Hiro", "Ines",
            "Jonas", "Keiko", "Lars", "Mei", "Nadia", "Omar", "Priya", "Quentin", "Rosa", "Sven",
            "Tariq",
        ];
        const LASTS: [&str; 15] = [
            "Abbott",
            "Brennan",
            "Castillo",
            "Dubois",
            "Eriksen",
            "Fischer",
            "Gallagher",
            "Haddad",
            "Ivanova",
            "Jensen",
            "Kowalski",
            "Lindqvist",
            "Moreau",
            "Nakamura",
            "Okafor",
        ];
        let mut text = String::new();
        for (i, first) in FIRSTS.iter().enumerate() {
            for (j, last) in LASTS.iter().enumerate() {
                text.push_str(&format!("Customer {}: {} {}\n", i * 15 + j, first, last));
            }
        }

        let spans = ner.detect(&text).await.unwrap();
        let found = spans
            .iter()
            .filter(|s| {
                FIRSTS
                    .iter()
                    .zip(LASTS.iter())
                    .any(|(f, l)| s.text == format!("{f} {l}"))
            })
            .count();
        assert!(
            found >= 270,
            "only {found} of 300 names found; reply must survive the name-dense file"
        );
    }

    #[tokio::test]
    async fn detects_names_from_json_response() {
        let json = r#"[{"text":"Alice","type":"person_name"},{"text":"Bob","type":"person_name"}]"#;
        let ner = NerEngine::new(Box::new(MockAiClient::new(vec![json.to_string()])));
        let spans = ner.detect("Alice met Bob at noon").await.unwrap();
        assert_eq!(spans.len(), 2);
        assert!(spans.iter().all(|s| s.kind == PiiKind::PersonName));
        assert_eq!(spans[0].text, "Alice");
    }

    #[tokio::test]
    async fn extracts_json_wrapped_in_prose_and_fences() {
        let raw = "Sure, here you go:\n```json\n[{\"text\":\"Acme Corp\",\"type\":\"organization\"}]\n```";
        let ner = NerEngine::new(Box::new(MockAiClient::new(vec![raw.to_string()])));
        let spans = ner.detect("I work at Acme Corp downtown").await.unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, PiiKind::Org);
    }

    #[tokio::test]
    async fn maps_address_label_and_locates_every_occurrence() {
        let json = r#"[{"text":"42 Baker St","type":"street_address"}]"#;
        let ner = NerEngine::new(Box::new(MockAiClient::new(vec![json.to_string()])));
        let spans = ner
            .detect("ship to 42 Baker St; bill to 42 Baker St")
            .await
            .unwrap();
        assert_eq!(spans.len(), 2);
        assert!(spans.iter().all(|s| s.kind == PiiKind::StreetAddress));
    }

    #[tokio::test]
    async fn empty_input_skips_the_model() {
        let mock = MockAiClient::new(vec!["[]".to_string()]);
        let ner = NerEngine::new(Box::new(mock.clone()));
        let spans = ner.detect("   ").await.unwrap();
        assert!(spans.is_empty());
        assert_eq!(mock.calls().len(), 0);
    }
}
