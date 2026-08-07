//! Qwen-backed NER for unstructured PII (person names, street addresses,
//! organizations) — the artifacts deterministic detectors cannot catch. The
//! model runs locally (the `[privacy]` engine endpoint), so detection never
//! leaves the LAN. Best-effort by nature: a miss is a leak, so the gateway pairs
//! this with the deterministic detectors and biases toward over-matching.

use std::time::Duration;

use serde::Deserialize;

use crate::ai::types::{AiEvent, Message};
use crate::ai::{AiClient, OpenAiClient, SamplingParams};
use crate::config::PrivacyConfig;
use crate::error::{Error, Result};

use super::{PiiKind, PiiSpan};

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
                max_tokens: 1024,
                enable_thinking: false,
                thinking: None,
            },
        );
        Ok(Self::new(Box::new(client)))
    }

    /// Detect unstructured PII in `text`, emitting a span for every occurrence of
    /// each artifact the model reports. Blank input and unparseable output both
    /// yield no spans (best-effort).
    pub async fn detect(&self, text: &str) -> Result<Vec<PiiSpan>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let raw = self.complete(text).await?;
        Ok(spans_from_items(text, &parse_items(&raw)))
    }

    async fn complete(&self, text: &str) -> Result<String> {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
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
        while let Some(event) = rx.recv().await {
            match event {
                AiEvent::Token(t) => out.push_str(&t),
                AiEvent::Error(e) => return Err(Error::Privacy(format!("NER engine error: {e}"))),
                _ => {}
            }
        }
        Ok(out)
    }
}

/// Extract the JSON array from a model response, tolerating prose or code fences
/// around it. Returns an empty list if nothing parses.
fn parse_items(raw: &str) -> Vec<NerItem> {
    let (Some(open), Some(close)) = (raw.find('['), raw.rfind(']')) else {
        return Vec::new();
    };
    if close < open {
        return Vec::new();
    }
    serde_json::from_str::<Vec<NerItem>>(&raw[open..=close]).unwrap_or_default()
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
    use crate::ai::testing::MockAiClient;

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
    async fn unparseable_response_yields_no_spans() {
        let ner = NerEngine::new(Box::new(MockAiClient::new(vec![
            "I could not find any.".to_string(),
        ])));
        let spans = ner.detect("some text").await.unwrap();
        assert!(spans.is_empty());
    }

    #[tokio::test]
    async fn empty_input_skips_the_model() {
        let mock = MockAiClient::new(vec![]);
        let ner = NerEngine::new(Box::new(mock.clone()));
        let spans = ner.detect("   ").await.unwrap();
        assert!(spans.is_empty());
        assert_eq!(mock.calls().len(), 0);
    }

    #[tokio::test]
    #[ignore = "live: needs Qwen at the [privacy] engine endpoint; run with --ignored"]
    async fn live_qwen_detects_person_names() {
        let cfg = PrivacyConfig {
            enabled: true,
            engine_base_url: Some("http://192.168.50.138:8080/v1".to_string()),
            engine_model: Some("qwen3.5-9b".to_string()),
            vault_dir: None,
            kinds: Vec::new(),
            redact_executor_egress: None,
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
}
