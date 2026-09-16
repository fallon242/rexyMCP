# Phase 1: `[privacy] terms_file`

**Milestone:** F05 — Privacy and security hardening
**Status:** todo
**Depends on:** none (F03 and F04 are done)
**Estimated diff:** ~320 lines
**Tags:** language=rust, kind=feature, size=m

## Goal

Let a repository declare literal terms that must never reach a cloud model, and
redact them on the existing outbound chokepoint alongside the NER pre-scan's
findings. Terms that a name detector cannot find — short site codes, system
acronyms, product names that are ordinary words — are the whole point.

## Architecture references

Read before starting:

- `docs/dev/milestones/F05-privacy-security-hardening/README.md` — why the existing
  dictionary does not cover this, and why it is not the abandoned reversible
  round-trip.
- `docs/dev/milestones/F03-executor-egress-protection/README.md` §"Why
  irreversible redaction" — the constraint this phase must not break.
- `executor/src/privacy/egress.rs` — `build_egress_index`, which returns the
  dictionary this phase extends.
- `executor/src/privacy/redact.rs` — `redact_pii` and `RedactingAiClient`, the
  one chokepoint every outbound message passes through.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

`build_egress_index` (`executor/src/privacy/egress.rs`) ends with:

```rust
let terms = index.redaction_terms();
let pii_files = index.files().cloned().collect();
Ok((terms, pii_files))
```

`redaction_terms()` (`executor/src/privacy/prescan.rs`) flattens the NER index
into `Vec<(String, PiiKind)>` sorted longest-first. `redact_pii`
(`executor/src/privacy/redact.rs`) matches each term by **plain substring** —
no word boundaries, no case handling — and replaces it with
`[REDACTED:<tag>]`.

`PrivacyConfig` (`executor/src/config.rs`) carries `enabled`,
`engine_base_url`, `engine_model`, `vault_dir`, `kinds`, `redact_executor_egress`
and `scan_globs`. **`kinds` is dead** — referenced only by a test asserting it
is empty. Leave it alone; it is out of scope.

## Spec

### 1. Config key

Add `terms_file: Option<PathBuf>` to `PrivacyConfig`. Document it in the config
comment block and in `rexymcp.toml.example` the way `scan_globs` is documented.
No default path: absent means the feature is off.

### 2. Term file loader

New module `executor/src/privacy/terms.rs`. Parse this shape:

```json
{ "entries": [
  { "code": "SITE_1", "aliases": ["Ashford", "ASHF"] },
  { "code": "SYS_1",  "aliases": ["Quillsys"], "embed": true },
  { "code": "SYS_2",  "aliases": ["Ledgers"],  "match": "case-sensitive" }
] }
```

- `code` and `aliases` are required; `aliases` must be non-empty.
- `embed` (default false) — the alias may match inside an identifier.
- `match` (default absent) — when present, the entry matches case-sensitively.
- A missing file, unreadable file, malformed JSON, or an entry missing `code`
  or `aliases` is an **error returned to the caller**, which fails the
  dispatch. Do not warn-and-continue: a term file that silently does not load
  is worse than none, because the operator believes they are protected.

### 3. Matcher

In the same module, build a matcher over the loaded entries:

- Aliases are tried **longest first**, so a specific alias wins over a prefix.
- Default matching requires a word boundary on **both** sides.
- `embed` entries require a boundary only on the left, so the alias matches
  when it prefixes a longer identifier.
- Entries with `match` are case-sensitive; all others are case-insensitive.
- An alias containing spaces matches any whitespace run between its words,
  including a newline, and **the replacement re-emits the line breaks it
  spanned** so the text is not reflowed.
- The replacement is the entry's code in square brackets: `[SITE_1]`.

### 4. Merge into the dictionary

`build_egress_index` loads the term file when `privacy.terms_file` is set and
returns the literal terms alongside the NER terms. The literal matcher runs
**before** the substring dictionary in `redact_pii`, so an entry's own rules
decide its matches rather than the plain substring pass.

Keep the existing return type stable if you can; if a new type is clearer,
change the two call sites rather than bending the shape.

### 5. Documentation

Update `docs/privacy.md`: the new key, the three flags, the failure behaviour,
and one sentence stating that the executor is never asked to restore a term.

## Acceptance criteria

- [ ] `cargo build` succeeds with zero new warnings.
- [ ] With `terms_file` unset, the dictionary and the redacted output are
      byte-identical to today's — proven by a test.
- [ ] A term listed in the file is replaced with `[CODE]` in an outbound
      message, verified through `RedactingAiClient`, not only in the matcher.
- [ ] A missing or malformed term file fails the dispatch with an error naming
      the file.
- [ ] An `embed` entry matches inside an identifier; a default entry does not.
- [ ] A `match` entry masks the capitalised product name and leaves the
      lower-case common noun alone.
- [ ] A term wrapped across a newline is masked and the line count is unchanged.

## Test plan

In `executor/src/privacy/terms.rs` unit tests unless noted:

- `loads_entries_and_flags` — the three shapes above parse, flags default false.
- `missing_file_is_an_error` / `malformed_json_is_an_error` /
  `entry_without_aliases_is_an_error` — each returns `Err`, none panics.
- `default_entry_requires_both_boundaries` — matches the bare word, does **not**
  match inside a longer word.
- `embed_entry_matches_inside_an_identifier` — and round-trips the surrounding
  identifier text unchanged apart from the code.
- `case_sensitive_entry_spares_the_common_noun` — capitalised form masked,
  lower-case form untouched.
- `wrapped_alias_matches_and_keeps_the_break` — asserts the newline count is
  unchanged.
- `longest_alias_wins` — a short alias that is a prefix of a longer one does not
  shadow it.
- In `redact.rs`: `literal_terms_redact_through_the_client` — a message through
  `RedactingAiClient` with a loaded term file comes out carrying `[CODE]`.
- In `egress.rs`: `unset_terms_file_changes_nothing` — the regression guard for
  the acceptance criterion above.

## End-to-end verification

Run a real dispatch against a cloud endpoint with a term file set, and quote
the outbound message from the session log showing `[CODE]` where the term was.
The F03 phase-05 dogfood is the pattern to follow. If no cloud endpoint is
available, say so in the Update Log and quote the `RedactingAiClient` test
output instead — and mark the phase `blocked` on the dogfood, not `done`.

## Authorizations

- [x] May add a module under `executor/src/privacy/`.
- [x] May edit `executor/src/config.rs` (the `PrivacyConfig` struct and its
      documentation only), `executor/src/privacy/egress.rs`,
      `executor/src/privacy/redact.rs`, `rexymcp.toml.example`, `docs/privacy.md`.
- [ ] May add a dependency — **no.** `serde_json` and `regex` are already here.
- [ ] May touch `docs/architecture.md` — **no.**
- [ ] May change `privacy.kinds` — **no**, it is out of scope.

## Out of scope

- **Reversibility at the executor boundary.** Nothing asks the model to restore
  a term. See the F03 decision.
- **A CLI to mask a file on demand.** A later phase, if wanted.
- **Masking the architect's own return path.** `scrub_phase_result` handles
  structured PII only; widening it is a separate decision.
- **Deciding the fate of `privacy.kinds`.**

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
