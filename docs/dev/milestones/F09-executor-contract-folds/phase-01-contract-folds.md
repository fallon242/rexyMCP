# Phase 1: contract folds

**Milestone:** F09 — Executor-contract calibration folds
**Status:** todo
**Depends on:** none
**Estimated diff:** ~40 lines
**Tags:** language=rust, kind=docs, size=xs

## Goal

Change four passages of `executor/templates/executor_contract.md` so the
executor (1) stops naming a model it cannot know, and (2) pastes result lines
instead of claiming a pinned count "matches". Pin both with tests.

## Pre-flight

1. `cargo test -p rexymcp-executor contract` → **9 passed** (measured 2026-09-18).
2. Full `cargo test` → **727 / 2 / 1208 passed**.

## Spec

Make exactly these four replacements in `executor/templates/executor_contract.md`.
Every other line of the file stays byte-identical. Keep each quoted phrase
the tests check on a single line — `contains` does not match across a line break.

### 1. Step 2 (lines 75-77)

Replace:

```
2. **Started entry:** append **one** progress entry to the phase's Update Log
   naming yourself. This is your attribution in the doc and the only Update Log
   entry you write.
```

with:

```
2. **Started entry:** append **one** progress entry to the phase's Update Log
   stating what you are about to do. Do **not** name yourself or a model: you
   cannot know which model you are, and the server writes the dispatched model
   into the completion entry. This is the only Update Log entry you write.
```

### 2. Step 8 (ends at line 103)

Replace:

```
   account reaches the reviewer now that you no longer hand-write the entry — so
   make it substantive; do not make it a bare "done."
```

with:

```
   account reaches the reviewer now that you no longer hand-write the entry — so
   make it substantive; do not make it a bare "done."
   When the phase pins an exact count (tests passed, records, lines), paste the
   command's own result line — for example `test result: ok. 727 passed; 0 failed`
   — and name any difference from the pinned number.
   Never write "matches" in place of the output: a count you did not paste is a
   count you did not check.
```

### 3. Resuming a phase (lines 115-116)

Replace:

```
the prior run stopped. The Update Log's prior entries stay as they are — append a
new started entry naming yourself, then continue. Everything else in this
```

with:

```
the prior run stopped. The Update Log's prior entries stay as they are — append a
new started entry (no model name — see step 2), then continue. Everything else in this
```

### 4. Completion checklist (line 128)

Replace:

```
[ ] Your final message is a substantive Summary + Notes for review (what you built, deviations, E2E result).
```

with:

```
[ ] Your final message is a substantive Summary + Notes for review (what you built, deviations, E2E result).
[ ] Every pinned count in your Summary is a pasted result line, not a restatement.
```

**Must NOT:**

- Add any `{…}` text. `placeholder_set_is_exactly_the_four_authorized`
  (`contract.rs`) fails on any curly-brace word other than the four command
  placeholders.
- Edit any other part of the contract, `WORKFLOW.md`, or `STANDARDS.md`.
- Touch Rust code other than adding the two tests below.

## Acceptance criteria

- [ ] The assembled contract does not contain `naming yourself`.
- [ ] It contains `Do **not** name yourself or a model`.
- [ ] It contains `Never write "matches" in place of the output`.
- [ ] It contains `Every pinned count in your Summary is a pasted result line`.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

Add to `mod tests` in `executor/src/agent/contract.rs`, in the shape of the
existing `contract_contains_resuming_a_phase`:

```rust
    #[test]
    fn contract_contains_resuming_a_phase() {
        let commands = CommandConfig::default();
        let output = assemble_executor_contract(&commands);
        assert!(
            output.contains("Resuming a phase"),
            "contract must contain the 'Resuming a phase' section"
        );
    }
```

1. `contract_does_not_ask_executor_to_name_itself` — asserts
   `!output.contains("naming yourself")` and
   `output.contains("Do **not** name yourself or a model")`.
2. `contract_requires_pasting_pinned_counts` — asserts
   `output.contains("Never write \"matches\" in place of the output")` and
   `output.contains("Every pinned count in your Summary is a pasted result line")`.

Write test 1 first, run it, and quote the failure in the Update Log.

**Finish condition:** `cargo test -p rexymcp-executor contract` → **11 passed**;
full `cargo test` → **727 / 2 / 1210 passed**. Paste the `test result:` lines.

## End-to-end verification

Run `cargo test -p rexymcp-executor contract -- --nocapture` and paste the
output in a `(end-to-end verification)` entry.

## Authorizations

- Editing `executor/templates/executor_contract.md` as specified.

## Out of scope

- The contract's "only Update Log entry you write" wording, which conflicts
  with `WORKFLOW.md`'s end-to-end entry. Noted for a later look.

## Update Log

<!-- entries appended below this line -->
