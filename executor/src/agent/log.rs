use crate::privacy::redact::EgressScrub;
use crate::security::redact::Redactor;
use crate::store::sessions::event::SessionEvent;
use crate::store::sessions::jsonl::{SessionLogHandle, session_log};

/// What the session log scrubs each record with: the secret `Redactor` always,
/// plus the egress scrub when redaction is engaged for this dispatch.
pub(super) struct LogScrub {
    redactor: Redactor,
    egress: Option<EgressScrub>,
}

impl LogScrub {
    pub(super) fn new(redactor: Redactor, egress: Option<EgressScrub>) -> Self {
        Self { redactor, egress }
    }
}

pub(super) fn log_event(
    handle: &Option<SessionLogHandle>,
    scrub: &LogScrub,
    clock: &dyn Fn() -> u64,
    turn: usize,
    event: SessionEvent,
) {
    let Some(handle) = handle else {
        return;
    };
    session_log(handle, clock(), turn, redact_event(scrub, event));
}

pub(super) fn log_session_end(
    handle: &Option<SessionLogHandle>,
    scrub: &LogScrub,
    clock: &dyn Fn() -> u64,
    status: &str,
    turns: usize,
) {
    log_event(
        handle,
        scrub,
        clock,
        turns,
        SessionEvent::SessionEnd {
            status: status.to_string(),
            turns,
        },
    );
}

/// Round-trip an event through the secret `Redactor` and, when egress redaction
/// is engaged, the egress scrub (literal terms, then the PII dictionary). The
/// secret pass runs on the serialized JSON — one-way secret patterns, which are
/// safe to apply to an encoded string. The egress pass walks the parsed value
/// and scrubs every string leaf, because it must see real newlines: an alias
/// wrapped across a line break is `"at Plant\nNine"` in the record, but in
/// serialized JSON that newline is the two characters `\` and `n`, so a
/// serialized-string pass would miss it. The `[CODE]` / `[REDACTED:kind]`
/// markers are JSON-safe, so the round-trip parses. On the can't-happen serde
/// failure, fall back to the un-scrubbed event's structure only after the
/// secret pass was attempted — serialization of these types is effectively
/// infallible, so this is a safety net, not a swallow.
fn redact_event(scrub: &LogScrub, event: SessionEvent) -> SessionEvent {
    let Ok(json) = serde_json::to_string(&event) else {
        return event;
    };
    let redacted = scrub.redactor.redact(&json);
    let Some(egress) = &scrub.egress else {
        return serde_json::from_str(&redacted).unwrap_or(event);
    };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&redacted) else {
        return event;
    };
    scrub_strings(&mut value, &|s| egress.scrub(s));
    serde_json::from_value(value).unwrap_or(event)
}

/// Apply `f` to every string leaf of `value`, in place.
fn scrub_strings(value: &mut serde_json::Value, f: &dyn Fn(&str) -> String) {
    match value {
        serde_json::Value::String(s) => *s = f(s),
        serde_json::Value::Array(items) => {
            for item in items {
                scrub_strings(item, f);
            }
        }
        serde_json::Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                scrub_strings(v, f);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::PiiKind;
    use crate::privacy::terms::LiteralTerms;
    use crate::security::redact::Redactor;

    fn scrub_with_egress() -> LogScrub {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("terms.json");
        std::fs::write(
            &path,
            r#"{"entries":[{"code":"SITE_1","aliases":["Plant Nine"]}]}"#,
        )
        .unwrap();
        let literal = LiteralTerms::load(&path).unwrap();
        let egress = EgressScrub::new(literal, vec![("Alice".to_string(), PiiKind::PersonName)]);
        LogScrub::new(Redactor::new(), Some(egress))
    }

    #[test]
    fn scrub_strings_reaches_every_leaf() {
        let mut v = serde_json::json!({
            "top": "x",
            "nested": { "inner": "x", "num": 5 },
            "list": ["x", "keep", 7]
        });
        scrub_strings(&mut v, &|s| s.replace("x", "y"));
        assert_eq!(v["top"], "y");
        assert_eq!(v["nested"]["inner"], "y");
        assert_eq!(v["nested"]["num"], 5);
        assert_eq!(v["list"][0], "y");
        assert_eq!(v["list"][1], "keep");
        assert_eq!(v["list"][2], 7);
    }

    #[test]
    fn redact_event_masks_terms_and_literals() {
        let scrub = scrub_with_egress();
        let event = SessionEvent::ToolResult {
            name: "read_file".into(),
            succeeded: true,
            output_preview: "owner Alice at Plant Nine".into(),
            output_bytes: 25,
        };
        let out = redact_event(&scrub, event);
        match out {
            SessionEvent::ToolResult { output_preview, .. } => {
                assert!(
                    output_preview.contains("[SITE_1]"),
                    "literal masked: {output_preview}"
                );
                assert!(
                    output_preview.contains("[REDACTED:name]"),
                    "dictionary term masked: {output_preview}"
                );
                assert!(!output_preview.contains("Alice"));
                assert!(!output_preview.contains("Plant Nine"));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn redact_event_masks_an_alias_across_a_newline() {
        let scrub = scrub_with_egress();
        let event = SessionEvent::Prompt {
            rendered: "at Plant\nNine today".into(),
        };
        let out = redact_event(&scrub, event);
        match out {
            SessionEvent::Prompt { rendered } => {
                assert!(
                    rendered.contains("[SITE_1]"),
                    "wrapped alias masked: {rendered}"
                );
                assert!(!rendered.contains("Plant"));
            }
            other => panic!("expected Prompt, got {other:?}"),
        }
    }

    #[test]
    fn redact_event_without_egress_is_unchanged() {
        let scrub = LogScrub::new(Redactor::new(), None);
        let out = redact_event(
            &scrub,
            SessionEvent::Completion {
                raw: "owner Alice".into(),
            },
        );
        match out {
            SessionEvent::Completion { raw } => assert_eq!(raw, "owner Alice"),
            other => panic!("expected Completion, got {other:?}"),
        }

        let secret = format!("sk-{}", "a".repeat(32));
        let out = redact_event(
            &scrub,
            SessionEvent::Completion {
                raw: format!("key is {secret}"),
            },
        );
        match out {
            SessionEvent::Completion { raw } => {
                assert!(!raw.contains("sk-"), "secret pattern still redacted: {raw}");
            }
            other => panic!("expected Completion, got {other:?}"),
        }
    }
}
