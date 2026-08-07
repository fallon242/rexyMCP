//! Deterministic PII detectors — regex + validators for structured PII an LLM
//! fumbles (emails, phones, SSNs, cards, IPv4, MACs). A privacy gate biases
//! toward false positives (over-anonymize) over false negatives (leak), so the
//! patterns err broad; unstructured PII (names, addresses, orgs) is the NER
//! engine's job in a later phase.

use std::sync::LazyLock;

use regex::Regex;

use super::{PiiKind, PiiSpan};

static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
        .expect("static email regex is valid")
});

static SSN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("static ssn regex is valid"));

static PHONE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Requires separators/parens so it cannot swallow a dashed SSN (3-2-4).
    Regex::new(r"(?:\+?1[-.\s])?(?:\(\d{3}\)\s?|\d{3}[-.\s])\d{3}[-.\s]\d{4}")
        .expect("static phone regex is valid")
});

static CARD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b\d{4}(?:[ -]?\d{4}){2,4}\b").expect("static card regex is valid")
});

static IPV4_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)\b")
        .expect("static ipv4 regex is valid")
});

static MAC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b").expect("static mac regex is valid")
});

/// Luhn checksum over a bare digit string — the standard credit-card validator.
fn luhn_valid(digits: &str) -> bool {
    let mut sum = 0u32;
    let mut double = false;
    for c in digits.chars().rev() {
        let mut d = match c.to_digit(10) {
            Some(d) => d,
            None => return false,
        };
        if double {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
        double = !double;
    }
    sum.is_multiple_of(10)
}

fn push_matches(re: &Regex, text: &str, kind: PiiKind, out: &mut Vec<PiiSpan>) {
    for m in re.find_iter(text) {
        out.push(PiiSpan {
            start: m.start(),
            end: m.end(),
            kind,
            text: m.as_str().to_string(),
        });
    }
}

/// Detect all structured PII in `text` deterministically, returning
/// non-overlapping spans in source order.
pub fn detect_deterministic(text: &str) -> Vec<PiiSpan> {
    let mut spans = Vec::new();
    push_matches(&EMAIL_RE, text, PiiKind::Email, &mut spans);
    push_matches(&SSN_RE, text, PiiKind::Ssn, &mut spans);
    push_matches(&PHONE_RE, text, PiiKind::Phone, &mut spans);
    push_matches(&IPV4_RE, text, PiiKind::Ipv4, &mut spans);
    push_matches(&MAC_RE, text, PiiKind::Mac, &mut spans);
    // Cards need Luhn validation on the digits, not just the 4-group shape.
    for m in CARD_RE.find_iter(text) {
        let digits: String = m.as_str().chars().filter(char::is_ascii_digit).collect();
        if (13..=19).contains(&digits.len()) && luhn_valid(&digits) {
            spans.push(PiiSpan {
                start: m.start(),
                end: m.end(),
                kind: PiiKind::CreditCard,
                text: m.as_str().to_string(),
            });
        }
    }
    merge_spans(&mut spans);
    spans
}

/// Sort spans into source order and drop any that overlaps one already kept.
/// Ordering is `(start asc, length desc)`, so the longest match at a given start
/// wins — keeping the result deterministic and safe to splice.
pub fn merge_spans(spans: &mut Vec<PiiSpan>) {
    spans.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| (b.end - b.start).cmp(&(a.end - a.start)))
    });
    let mut kept: Vec<PiiSpan> = Vec::with_capacity(spans.len());
    let mut last_end = 0usize;
    for span in spans.drain(..) {
        if kept.is_empty() || span.start >= last_end {
            last_end = span.end;
            kept.push(span);
        }
    }
    *spans = kept;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<PiiKind> {
        detect_deterministic(text)
            .into_iter()
            .map(|s| s.kind)
            .collect()
    }

    #[test]
    fn detects_email() {
        let spans = detect_deterministic("reach me at a@b.com");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, PiiKind::Email);
        assert_eq!(spans[0].text, "a@b.com");
    }

    #[test]
    fn detects_dashed_ssn() {
        assert_eq!(kinds("ssn 123-45-6789 on file"), vec![PiiKind::Ssn]);
    }

    #[test]
    fn detects_separated_phone() {
        assert_eq!(kinds("call 555-123-4567 today"), vec![PiiKind::Phone]);
    }

    #[test]
    fn phone_regex_does_not_match_ssn() {
        // 3-2-4 grouping is an SSN, never a phone (3-3-4).
        let ks = kinds("123-45-6789");
        assert!(ks.contains(&PiiKind::Ssn), "expected Ssn, got {ks:?}");
        assert!(!ks.contains(&PiiKind::Phone), "must not be Phone: {ks:?}");
    }

    #[test]
    fn detects_valid_credit_card() {
        // 4111 1111 1111 1111 is a Luhn-valid test number.
        assert_eq!(
            kinds("card 4111 1111 1111 1111 exp"),
            vec![PiiKind::CreditCard]
        );
    }

    #[test]
    fn rejects_credit_card_failing_luhn() {
        // Same shape, last digit broken → fails Luhn → not flagged.
        assert!(kinds("card 4111 1111 1111 1112 exp").is_empty());
    }

    #[test]
    fn detects_ipv4() {
        assert_eq!(kinds("host 192.168.1.1 up"), vec![PiiKind::Ipv4]);
    }

    #[test]
    fn rejects_out_of_range_ipv4() {
        assert!(kinds("not 999.999.999.999 real").is_empty());
    }

    #[test]
    fn detects_mac() {
        assert_eq!(kinds("nic 00:1b:44:11:3a:b7 seen"), vec![PiiKind::Mac]);
    }

    #[test]
    fn finds_nothing_in_clean_text() {
        assert!(detect_deterministic("the quick brown fox jumps over it").is_empty());
    }

    #[test]
    fn merge_drops_overlapping_spans() {
        let mut spans = vec![
            PiiSpan {
                start: 0,
                end: 10,
                kind: PiiKind::Email,
                text: "x".into(),
            },
            PiiSpan {
                start: 5,
                end: 8,
                kind: PiiKind::Phone,
                text: "y".into(),
            },
            PiiSpan {
                start: 10,
                end: 14,
                kind: PiiKind::Ssn,
                text: "z".into(),
            },
        ];
        merge_spans(&mut spans);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].kind, PiiKind::Email);
        assert_eq!(spans[1].kind, PiiKind::Ssn);
    }
}
