//! PII detection and reversible tokenization — the M44 privacy gate's engine.
//!
//! `detector` finds structured PII deterministically (regex + validators);
//! `tokenizer` maps each detected artifact to a stable, reversible pseudonym.
//! This is the reversible counterpart to [`crate::security::redact`], whose
//! masking is deliberately one-way.

pub mod detector;
pub mod tokenizer;

/// A class of personally identifiable information. The variant fixes the token
/// prefix a detected artifact is masked with (`Email` → `Email_1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PiiKind {
    PersonName,
    Email,
    Phone,
    Ssn,
    CreditCard,
    Ipv4,
    Mac,
    StreetAddress,
    Org,
}

impl PiiKind {
    /// The stable prefix used when minting a pseudonym token for this kind.
    pub fn token_prefix(self) -> &'static str {
        match self {
            PiiKind::PersonName => "Person",
            PiiKind::Email => "Email",
            PiiKind::Phone => "Phone",
            PiiKind::Ssn => "Ssn",
            PiiKind::CreditCard => "Card",
            PiiKind::Ipv4 => "Ip",
            PiiKind::Mac => "Mac",
            PiiKind::StreetAddress => "Address",
            PiiKind::Org => "Org",
        }
    }
}

/// A detected PII artifact, located by byte offset into the source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiiSpan {
    pub start: usize,
    pub end: usize,
    pub kind: PiiKind,
    pub text: String,
}
