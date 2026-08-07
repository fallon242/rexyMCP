# Phase 1: Privacy foundation — config, deterministic detectors, stable tokenizer

**Milestone:** M44 — PII Ingestion Gate
**Status:** review
**Depends on:** none
**Estimated diff:** ~400 lines (module + tests)
**Tags:** language=rust, kind=feature, size=l

## Goal

Stand up `executor/src/privacy/` with the two pure, dependency-free halves of the
PII gate — **deterministic detectors** for structured PII and a **stable,
reversible tokenizer** — plus the `[privacy]` config section that later phases
read. No I/O, no crypto, no model calls; those are phases 02–03.

## Architecture references

Read before starting:

- `docs/dev/milestones/M44-pii-ingestion-gate/README.md` — threat model, the
  deterministic-first / bias-to-false-positive principle, where this module sits.
- `executor/src/security/redact.rs` — the sibling *irreversible* redactor; mirror
  its `LazyLock<Regex>` style. This module is its reversible counterpart.
- `docs/dev/STANDARDS.md` §2.1 (error model), §3 (test coverage).

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the M44 README.
3. Read this entire phase doc before touching code.
4. Confirm a clean branch.

## Design principle (load-bearing)

A privacy gate must bias toward **false positives** (over-anonymize) over **false
negatives** (leak). A mis-flagged non-PII token only over-redacts, which is safe;
a missed PII artifact is the exact leak the gate exists to stop. Detectors err
aggressive. This justifies broad patterns.

## Current state

- No `privacy` module exists. `executor/src/lib.rs` declares the module list.
- `executor/src/config.rs` `Config` (line 336) has no `privacy` field. Top-level
  `Config` derives `#[serde(default)]` **without** `deny_unknown_fields`
  (confirmed: the only mention in the file is a test comment noting its absence),
  so a `[privacy]` section is purely additive.
- `regex` is already a workspace dependency (used by `security/redact.rs`).

## Spec

1. **`PiiKind` + `PiiSpan`** — in new `executor/src/privacy/mod.rs`. `PiiKind` is
   a `Copy` enum: `PersonName, Email, Phone, Ssn, CreditCard, Ipv4, Mac,
   StreetAddress, Org`. Add `fn token_prefix(self) -> &'static str` (`PersonName`
   → `"Person"`, `Email` → `"Email"`, `Phone` → `"Phone"`, `Ssn` → `"Ssn"`,
   `CreditCard` → `"Card"`, `Ipv4` → `"Ip"`, `Mac` → `"Mac"`, `StreetAddress` →
   `"Address"`, `Org` → `"Org"`). `PiiSpan { start: usize, end: usize, kind:
   PiiKind, text: String }` where `start`/`end` are byte offsets into the source.
   Declare `pub mod detector; pub mod tokenizer;`.

2. **Deterministic detectors** — in `executor/src/privacy/detector.rs`. One
   `LazyLock<Regex>` per class plus `pub fn detect_deterministic(text: &str) ->
   Vec<PiiSpan>` that runs all of them, then merges overlaps. Classes for this
   phase: `Email`, `Phone`, `Ssn`, `CreditCard`, `Ipv4`, `Mac` (IPv6, person
   names, addresses, orgs are later phases).
   - Email: `[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}`.
   - Ssn (US, dashed): `\b\d{3}-\d{2}-\d{4}\b`.
   - Phone: requires separators/parens so it cannot swallow an SSN — e.g.
     `(?:\+?1[-.\s])?(?:\(\d{3}\)\s?|\d{3}[-.\s])\d{3}[-.\s]\d{4}`.
   - CreditCard: a candidate regex (`\b\d{4}(?:[ -]?\d{4}){2,4}\b`) whose digits
     are then **Luhn-validated** — a candidate that fails Luhn is **not** a span.
     Add `fn luhn_valid(digits: &str) -> bool`.
   - Ipv4: octet-range-validated regex
     `\b(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)\b`.
   - Mac: `\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b`.
   - `merge_spans(&mut Vec<PiiSpan>)`: sort by `(start asc, len desc)` and drop any
     span that overlaps one already kept. Keeps detection deterministic and
     splice-safe.

3. **Stable tokenizer** — in `executor/src/privacy/tokenizer.rs`. `TokenMap` holds
   `forward: HashMap<String,String>` (original → token), `reverse:
   HashMap<String,(String,PiiKind)>` (token → original+kind), and per-kind
   counters. Methods:
   - `intern(&mut self, original: &str, kind: PiiKind) -> String` — returns the
     existing token if `original` was seen, else mints `"{prefix}_{n}"` with a
     monotonic per-kind `n` (so same original → same token; distinct originals →
     distinct tokens; never a collision).
   - `anonymize(&mut self, text: &str, spans: &[PiiSpan]) -> String` — splice each
     span's `text` → its interned token, applied right-to-left by byte offset so
     earlier offsets stay valid.
   - `anonymize_text(&mut self, text: &str) -> String` — convenience: run
     `detect_deterministic` then `anonymize`.
   - `reconstitute(&self, text: &str) -> String` — replace every known token with
     its original. Build a single `\b(tok1|tok2|…)\b` regex from `reverse` keys so
     `Person_1` never matches inside `Person_12` (the trailing `\b` fails against a
     following digit). No-op when the map is empty.

4. **`[privacy]` config** — in `executor/src/config.rs`. Add `PrivacyConfig`
   mirroring the `ContextConfig` pattern (`#[derive(Debug, Clone, Serialize,
   Deserialize)]` + `#[serde(default)]` + a manual `impl Default`). Fields:
   `enabled: bool` (default **false** — the gate is opt-in until enforcement lands
   in phase-06), `engine_base_url: Option<String>`, `engine_model:
   Option<String>` (the Qwen engine, used in phase-03; default `None`), `vault_dir:
   Option<PathBuf>` (phase-02; default `None` → resolved later), `kinds:
   Vec<String>` (default empty → "all"). Add `pub privacy: PrivacyConfig` to
   `Config` (with `#[serde(default)]`).

5. **Module declaration** — add `pub mod privacy;` to `executor/src/lib.rs`.

## Acceptance criteria

- [ ] `cargo build` succeeds, zero new warnings.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes.
- [ ] `cargo fmt --all --check` reports no diff (fix with `rustfmt` on touched files).
- [ ] `cargo test` passes with the new tests, baseline + new.
- [ ] `detect_deterministic` finds the email, phone, SSN, valid card, IPv4, and
      MAC in a mixed fixture string, and finds **nothing** in a PII-free string.
- [ ] A 16-digit number that fails Luhn is **not** flagged as a card.
- [ ] `reconstitute(anonymize_text(s))` round-trips `s` for a fixture with repeats.
- [ ] A `[privacy]` section in a TOML loads into `Config.privacy`; its absence
      yields the documented defaults (`enabled = false`).

## Test plan

In `#[cfg(test)] mod tests` at the bottom of each file:

- `detector.rs`: `detects_email`, `detects_dashed_ssn`, `detects_separated_phone`,
  `phone_regex_does_not_match_ssn`, `detects_valid_credit_card`,
  `rejects_credit_card_failing_luhn`, `detects_ipv4`, `rejects_out_of_range_ipv4`,
  `detects_mac`, `finds_nothing_in_clean_text`, `merge_drops_overlapping_spans`.
- `tokenizer.rs`: `same_original_gets_same_token`,
  `distinct_originals_get_distinct_tokens`, `reconstitute_inverts_anonymize`,
  `reconstitute_leaves_token_prefix_of_longer_token_intact` (`Person_1` vs
  `Person_12`), `anonymize_replaces_every_occurrence`,
  `reconstitute_is_noop_on_empty_map`.
- `config.rs`: `loads_privacy_section`, `privacy_defaults_when_absent`.

## End-to-end verification

This phase ships no runtime-loadable artifact beyond the library itself (no CLI,
no binary entrypoint — those are phase-05). The `[privacy]` config is exercised by
`Config::load` in a `loads_privacy_section` test against a real `TempDir` TOML
file, which is the closest to end-to-end this phase reaches. Quote the
`cargo test` summary in the completion Update Log.

## Authorizations

- No new dependencies (`regex` already present).
- No `docs/architecture.md` edit (M44's Status entry is added at milestone close,
  matching the M43 precedent).
- New files: `executor/src/privacy/{mod,detector,tokenizer}.rs`.

## Out of scope

- Vault / any persistence or encryption — phase-02.
- Any model call / NER / the Qwen engine — phase-03.
- Content-hash change tracking / the ingestion registry — phase-04.
- CLI subcommands — phase-05.
- Executor/`PhaseResult` boundary wiring — phase-06.
- IPv6, person-name, address, and org detection — later (person/address/org are
  the NER engine's job in phase-03; IPv6 is a deferred detector refinement).
- Making tokens robust to being echoed back mangled by DeepSeek — phase-06.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-07 14:56 (complete)

**Summary:** Built the `executor/src/privacy/` foundation, architect-implemented
(hybrid build). `mod.rs` defines `PiiKind` (with `token_prefix`) and `PiiSpan`
(byte-offset located). `detector.rs` deterministically finds email, dashed SSN,
separator/paren phone, Luhn-validated credit card, range-validated IPv4, and MAC
via `LazyLock<Regex>` (mirroring `security/redact.rs` style), with `merge_spans`
dropping overlaps `(start asc, len desc)`. `tokenizer.rs` provides `TokenMap` —
`intern` (stable per-kind monotonic tokens, same original → same token, no
collisions), `anonymize`/`anonymize_text` (right-to-left byte splicing), and
`reconstitute` (word-boundary-anchored so `Person_1` never matches inside
`Person_12`). Added the `[privacy]` `PrivacyConfig` section (opt-in, default
`enabled = false`) to `config.rs` and `pub mod privacy;` to `lib.rs`. No new
dependencies. Two deviations from the draft, both clippy-driven and behavior-
neutral: `PrivacyConfig` uses `#[derive(Default)]` (all field defaults are the
type defaults) rather than a manual impl, and the `privacy_defaults_when_absent`
test writes a `[project]` config rather than a partial `[executor]` block (the
loader rejects an executor block missing `base_url`/`provider`, unrelated to this
phase).

**Acceptance criteria:** all ticked above.

**Commands:**

```
$ cargo fmt --all --check
(no output — clean)

$ cargo build 2>&1 | tail -2
   Compiling rexymcp v0.9.1 (/home/gpratt/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.66s

$ cargo clippy --all-targets --all-features -- -D warnings 2>&1 | tail -2
    Checking rexymcp v0.9.1 (/home/gpratt/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.27s

$ cargo test 2>&1 | grep "^test result"
test result: ok. 685 passed; 0 failed; 0 ignored; ...   (mcp)
test result: ok. 2 passed; 0 failed; 0 ignored; ...     (readme_config_reference)
test result: ok. 1093 passed; 0 failed; 2 ignored; ...  (executor lib)
```

Baseline was 1761 (685 + 2 + 1074); now 1780 (+19 = 17 detector/tokenizer + 2
config). Zero failures.

**End-to-end verification:** Phase ships no runtime-loadable artifact beyond the
library (no CLI/binary — those are phase-05). The `[privacy]` config is exercised
end-to-end by `loads_privacy_section`, which writes a real `rexymcp.toml`
(`[privacy] enabled = true`, `engine_model = "qwen3.5-9b"`) to a `TempDir` and
asserts `Config::load` reads it back — the closest to end-to-end this phase
reaches.
