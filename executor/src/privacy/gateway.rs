//! The detection + anonymization front door for a single text: deterministic
//! detectors ∪ the Qwen NER engine, tokenized through a `TokenMap`. Deterministic
//! spans win wherever they overlap an NER span — a regex/validator beats a
//! probabilistic guess.

use crate::error::Result;

use super::PiiSpan;
use super::detector::{detect_deterministic, merge_spans};
use super::ner::NerEngine;
use super::tokenizer::TokenMap;

pub struct Gateway {
    ner: NerEngine,
}

impl Gateway {
    pub fn new(ner: NerEngine) -> Self {
        Self { ner }
    }

    /// Detect every PII artifact in `text` (deterministic ∪ NER) and replace it
    /// with a stable token from `map`. Deterministic spans take precedence where
    /// they overlap an NER span.
    pub async fn anonymize(&self, text: &str, map: &mut TokenMap) -> Result<String> {
        let mut spans = detect_deterministic(text);
        for span in self.ner.detect(text).await? {
            if !spans.iter().any(|kept| overlaps(kept, &span)) {
                spans.push(span);
            }
        }
        merge_spans(&mut spans);
        Ok(map.anonymize(text, &spans))
    }
}

fn overlaps(a: &PiiSpan, b: &PiiSpan) -> bool {
    a.start < b.end && b.start < a.end
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testing::MockAiClient;

    fn gateway_returning(json: &str) -> Gateway {
        Gateway::new(NerEngine::new(Box::new(MockAiClient::new(vec![
            json.to_string(),
        ]))))
    }

    #[tokio::test]
    async fn anonymizes_deterministic_and_ner_together() {
        let gw = gateway_returning(r#"[{"text":"Alice","type":"person_name"}]"#);
        let mut map = TokenMap::new();
        let out = gw
            .anonymize("Alice emailed a@b.com", &mut map)
            .await
            .unwrap();
        assert_eq!(out, "Person_1 emailed Email_1");
        assert_eq!(map.reconstitute(&out), "Alice emailed a@b.com");
    }

    #[tokio::test]
    async fn deterministic_wins_over_overlapping_ner_span() {
        // The model wrongly flags the email as a person name; the deterministic
        // Email span must win, so the token is Email_*, not Person_*.
        let gw = gateway_returning(r#"[{"text":"a@b.com","type":"person_name"}]"#);
        let mut map = TokenMap::new();
        let out = gw.anonymize("mail a@b.com", &mut map).await.unwrap();
        assert_eq!(out, "mail Email_1");
    }

    #[tokio::test]
    async fn ner_only_text_is_anonymized() {
        let gw = gateway_returning(r#"[{"text":"Bob","type":"person_name"}]"#);
        let mut map = TokenMap::new();
        let out = gw.anonymize("hello Bob", &mut map).await.unwrap();
        assert_eq!(out, "hello Person_1");
    }
}
