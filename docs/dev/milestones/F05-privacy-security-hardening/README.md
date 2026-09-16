# F05 — Privacy and security hardening

**Goal:** close the gaps a real deployment found between what the privacy
features promise and what they do. Seven findings, each with evidence, each
small. Together they are the difference between a safety net and a belief.

**Status:** in-progress — opened 2026-09-16 on human sign-off. Phase 01 is
drafted and dispatchable; phases 02–05 are named and drafted on demand.

**Depends on:** F02 (ingestion gate), F03 (egress protection), F04 (pre-scan
efficiency). All done.

**Source:** the CRS modernisation deployment, 2026-09. Every finding below was
observed there, not imagined.

**Exit criteria:**

- [ ] A repo can declare literal terms that must never reach a cloud model, and
      they are redacted on the existing chokepoint.
- [ ] A dispatch leaves no unredacted record of what the executor read.
- [ ] The vault's container is protected like its key.
- [ ] The prompt guard cannot fail open, and is installed rather than copied.
- [ ] No privacy setting is inert while appearing to work.
- [ ] `docs/privacy.md` states one thing about egress protection, not two.
- [ ] A failed PII pre-scan stops a cloud dispatch instead of running it with
      reduced protection.
- [ ] All four gates pass; each mechanism has a test that fails when reverted.

## The findings

### 1. Literal terms are invisible to the pre-scan

The dictionary is built from what the NER engine *recognises*. Short site codes
and system acronyms are returned by no NER pass; a product name that is also an
ordinary English word cannot be masked blindly without corrupting prose; and a
system name that prefixes legacy program identifiers appears inside identifiers
far more often than in prose — 207 of 216 occurrences in the deployment. A flat
`Vec<(String, PiiKind)>` matched by substring cannot express any of that.

**Phase 01.**

### 2. Session logs store unredacted tool output

`.rexymcp/sessions/*.jsonl` records each `tool_result` with an
`output_preview`. In the deployment, five previews from the turns that read a
private extract were stored raw; none carried a redaction marker. The directory
is `0755` and the files `0644`.

So redaction protects the wire to the model and leaves a world-readable
plaintext record of the same content on disk. An operator who trusts
`[privacy]` has no reason to expect that.

**Phase 02** — redact previews with the same chokepoint, and create the session
directory `0700`, files `0600`.

### 3. The vault is half-hardened

`seal.rs` sets `0600` on the key. The vault directory, the encrypted index and
the registry are created with no explicit mode — `0755` and `0644` in practice.
The default location is inside the repository, for a store the documentation
itself calls a honeypot because it concentrates every original.

**Phase 03** — create the vault `0700` and its contents `0600`; default it
outside the repo, or refuse to run when it sits inside one that is a git work
tree without being ignored.

### 4. The prompt guard fails open three ways

`plugin/hooks/pii-guard.sh` is the last line before a typed prompt reaches the
architect. As shipped it: reads only `.user_input`, so a payload-key rename
silently disables it; exits non-zero when `jq` is absent, which the harness
treats as an error rather than a block; and matches line by line, so PII pasted
across a line break is missed. Its card pattern matches 16 digits only (AmEx is
15, Diners 14) and its SSN and phone patterns require punctuation, which raw
report extracts do not carry. It is also not installed by the plugin — the
operator must find and copy it.

All five were fixed downstream in the deployment; the fixes are small.

**Phase 04.**

### 5. `privacy.kinds` is inert

Declared on `PrivacyConfig`, referenced only by a test asserting it is empty.
An operator who sets it believes they have scoped detection. Nothing reads it.
Implement it or delete it; either is safer than a setting that looks like a
control.

**Phase 05.**

### 6. The privacy doc contradicts itself

`docs/privacy.md` describes executor egress as automated under F03, and its
Limitations section still says egress "is not automated … a documented residual
risk; use a local executor or pre-scrub with the CLI". A reader acts on
whichever they find first. One of them is two milestones stale.

**Phase 05**, with finding 5 — both are documentation-truth fixes.

### 7. A failed pre-scan runs the cloud dispatch anyway

Found 2026-09-16 during F06 phase-01. The NER engine was unreachable
(`curl` to `engine_base_url` exits 7, "couldn't connect"), `build_egress_index`
failed (`executor/src/privacy/egress.rs:114-124`), and the run continued:

```rust
// mcp/src/runner.rs:402-408
Err(e) => {
    egress_warning = Some(format!(
        "executor-egress redaction engaged but the PII pre-scan failed ({e}); only \
         structured PII is redacted live and the write-guard is off"
    ));
}
```

The run made 48 turns against `api.deepseek.com` with no dictionary redaction
and no write-guard. The only signal was a line in `PhaseResult.warnings`. The
comment at `runner.rs:385` says this fallback is deliberate. The stated privacy
rule says the opposite: "When the detection engine is unavailable, cloud work
stops rather than proceeding unprotected"
(`plugin/skills/architect/SKILL.md:177`).

The warning is also hard to diagnose. `scrub_phase_result`
(`mcp/src/server.rs:248`) redacts the engine's IP address in the error, so it
reads `http://Ip_1:8080`. That is redaction working correctly, not a bug, but
the message does not say which setting to check.

**Phase 06** — a failed pre-scan fails the dispatch before turn 1, and the
error names `privacy.engine_base_url`. Whether to keep the reduced mode behind
an explicit opt-in is a question for drafting. Small: one match arm plus a test.

## Why literal masking is not the abandoned reversible round-trip

Phase-06b proved a *reversible* executor round-trip corrupts files: asked to
normalise a tokenised value, the model replaces the token with fabricated data.
F03's answer was one-way redaction.

This milestone keeps that answer. Literal terms are redacted one-way like every
other dictionary hit, and the executor is never asked to restore one. Where a
human needs real names back — a report, a committee paper — restoration happens
locally from the same term file. The executor boundary gains no reversibility.

## Phases

Expanded on demand, per WORKFLOW. Phases 01 and 06 are drafted. Phase 06 runs
first, on human instruction: until it lands, a dead NER engine lets a cloud
dispatch run with reduced protection.

| #  | Phase | Status |
|----|-------|--------|
| 01 | terms-file ([phase-01-terms-file.md](phase-01-terms-file.md)) | todo |
| 02 | session-log redaction + permissions | not drafted |
| 03 | vault container hardening | not drafted |
| 04 | prompt guard: fail closed, wider patterns, installed | not drafted |
| 05 | inert config + doc truth | not drafted |
| 06 | prescan-fails-closed ([phase-06-prescan-fails-closed.md](phase-06-prescan-fails-closed.md)) | in-progress |

## Reference implementation

`tools/mask.py` in the CRS modernisation repo implements the phase-01 matcher,
with its limits measured on real documents: restoring a masked document
normalises alias spelling and casing, so a restored copy is not byte-identical;
and an entry not flagged to match inside identifiers is invisible there by
design. Its shell prompt-guard carries the phase-04 fixes.

## Estimate

About **three to four executor days** for all six phases, plus review. Phase 01
is the only one with real design content; 02 and 03 are permissions and one
call to an existing chokepoint; 04 is a shell script; 05 is a deletion and a
paragraph; 06 is one match arm.
