//! Irreversible PII redaction for the executor egress boundary (M45). Replaces
//! detected PII with `[REDACTED:kind]` — one-way, no vault, so there is nothing
//! the model can "correct" into fabricated data (the phase-06b failure mode).
//! `RedactingAiClient` wraps a cloud client so every outbound message is redacted
//! before it leaves the machine.

use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedSender;

use super::detector::{detect_deterministic, merge_spans};
use super::terms::LiteralTerms;
use super::{PiiKind, PiiSpan};
use crate::ai::AiClient;
use crate::ai::types::{AiEvent, Message, ToolSchema};

/// Replace every PII artifact in `text` with `[REDACTED:<tag>]`. Deterministic
/// detectors run live; `terms` (the repo pre-scan dictionary) catch names /
/// addresses by exact substring. Longest match wins on overlap.
pub fn redact_pii(text: &str, terms: &[(String, PiiKind)]) -> String {
    let mut spans = detect_deterministic(text);
    for (term, kind) in terms {
        if term.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(rel) = text[from..].find(term.as_str()) {
            let start = from + rel;
            let end = start + term.len();
            spans.push(PiiSpan {
                start,
                end,
                kind: *kind,
                text: term.clone(),
            });
            from = end;
        }
    }
    merge_spans(&mut spans);
    let mut ordered: Vec<&PiiSpan> = spans.iter().collect();
    ordered.sort_by_key(|s| std::cmp::Reverse(s.start));
    let mut out = text.to_string();
    for span in ordered {
        out.replace_range(
            span.start..span.end,
            &format!("[REDACTED:{}]", span.kind.marker_tag()),
        );
    }
    out
}

/// An `AiClient` decorator that redacts PII from every outbound message before
/// forwarding to `inner`. Engaged only for a cloud executor endpoint (see
/// `egress::should_redact_egress`).
pub struct RedactingAiClient {
    inner: Box<dyn AiClient>,
    terms: Vec<(String, PiiKind)>,
    literal: LiteralTerms,
}

impl RedactingAiClient {
    pub fn new(inner: Box<dyn AiClient>, terms: Vec<(String, PiiKind)>) -> Self {
        Self {
            inner,
            terms,
            literal: LiteralTerms::default(),
        }
    }

    /// Add the literal term file (from `[privacy] terms_file`). Set before the
    /// first dispatch; `RedactingAiClient::new` leaves it empty.
    pub fn with_literal_terms(mut self, literal: LiteralTerms) -> Self {
        self.literal = literal;
        self
    }

    fn redact_message(&self, mut msg: Message) -> Message {
        msg.content = redact_pii(&self.literal.mask(&msg.content), &self.terms);
        if let Some(results) = msg.tool_results.as_mut() {
            for result in results.iter_mut() {
                result.content = redact_pii(&self.literal.mask(&result.content), &self.terms);
            }
        }
        msg
    }
}

#[async_trait]
impl AiClient for RedactingAiClient {
    async fn chat(
        &self,
        system_prompt: &str,
        messages: Vec<Message>,
        tx: UnboundedSender<AiEvent>,
        tools: Option<&[ToolSchema]>,
    ) -> anyhow::Result<()> {
        // Redact the system prompt too: it is built from project content and the
        // same terms must not cross the boundary inside it.
        let system = redact_pii(&self.literal.mask(system_prompt), &self.terms);
        let redacted: Vec<Message> = messages
            .into_iter()
            .map(|m| self.redact_message(m))
            .collect();
        self.inner.chat(&system, redacted, tx, tools).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::testing::MockAiClient;
    use crate::ai::types::ToolResult;

    fn name(text: &str) -> Vec<(String, PiiKind)> {
        vec![(text.to_string(), PiiKind::PersonName)]
    }

    #[test]
    fn redacts_deterministic_email() {
        assert_eq!(
            redact_pii("mail a@b.com now", &[]),
            "mail [REDACTED:email] now"
        );
    }

    #[test]
    fn redacts_dictionary_name() {
        assert_eq!(redact_pii("hi Alice", &name("Alice")), "hi [REDACTED:name]");
    }

    #[test]
    fn longer_term_wins_on_overlap() {
        let terms = vec![
            ("John".to_string(), PiiKind::PersonName),
            ("John Smith".to_string(), PiiKind::PersonName),
        ];
        assert_eq!(
            redact_pii("John Smith here", &terms),
            "[REDACTED:name] here"
        );
    }

    #[test]
    fn leaves_clean_text_untouched() {
        assert_eq!(
            redact_pii("just refactor the parser", &[]),
            "just refactor the parser"
        );
    }

    #[tokio::test]
    async fn redacts_outbound_messages_and_system_prompt() {
        let mock = MockAiClient::new(vec![]);
        let client = RedactingAiClient::new(Box::new(mock.clone()), name("Alice"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let messages = vec![Message {
            role: "user".to_string(),
            content: "Alice's email is a@b.com".to_string(),
            tool_calls: None,
            tool_results: None,
            turn: None,
        }];

        client
            .chat("help Alice today", messages, tx, None)
            .await
            .unwrap();

        let call = &mock.calls()[0];
        assert_eq!(call.system_prompt, "help [REDACTED:name] today");
        assert_eq!(
            call.messages[0].content,
            "[REDACTED:name]'s email is [REDACTED:email]"
        );
    }

    #[tokio::test]
    async fn literal_terms_redact_through_the_client() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("terms.json");
        std::fs::write(
            &file,
            r#"{"entries":[{"code":"SITE_1","aliases":["Plant Nine"]}]}"#,
        )
        .unwrap();

        let mock = MockAiClient::new(vec![]);
        let client = RedactingAiClient::new(Box::new(mock.clone()), vec![])
            .with_literal_terms(LiteralTerms::load(&file).unwrap());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let messages = vec![Message {
            role: "user".to_string(),
            content: "Mail ops@example.com about Plant Nine.".to_string(),
            tool_calls: None,
            tool_results: None,
            turn: None,
        }];

        client
            .chat("system about Plant Nine", messages, tx, None)
            .await
            .unwrap();

        let call = &mock.calls()[0];
        assert_eq!(call.system_prompt, "system about [SITE_1]");
        let body = &call.messages[0].content;
        assert!(
            body.contains("[SITE_1]"),
            "the listed term must be masked: {body}"
        );
        assert!(
            !body.contains("Plant Nine"),
            "the plaintext term must not reach the inner client: {body}"
        );
    }

    #[tokio::test]
    async fn redacts_tool_result_content() {
        let mock = MockAiClient::new(vec![]);
        let client = RedactingAiClient::new(Box::new(mock.clone()), name("Bob"));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let messages = vec![Message {
            role: "tool".to_string(),
            content: String::new(),
            tool_calls: None,
            tool_results: Some(vec![ToolResult {
                tool_call_id: "1".to_string(),
                tool_name: "read_file".to_string(),
                content: "owner: Bob, ip 10.0.0.5".to_string(),
            }]),
            turn: None,
        }];

        client.chat("sys", messages, tx, None).await.unwrap();

        let calls = mock.calls();
        let forwarded = &calls[0].messages[0].tool_results.as_ref().unwrap()[0];
        assert_eq!(
            forwarded.content,
            "owner: [REDACTED:name], ip [REDACTED:ip]"
        );
    }
}
