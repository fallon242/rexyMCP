# Phase 1: marker rule

**Milestone:** F16 — Log entry placement
**Status:** in-progress
**Depends on:** none
**Estimated diff:** ~15 lines
**Tags:** language=rust, kind=docs, size=xs

## Goal

Add one sentence to the executor contract saying new Update Log entries go at
the end of the file, below the `<!-- entries appended below this line -->`
marker. Pin it with a test.

**Verified by the architect 2026-09-19** in a scratch worktree at `212ab13`:
format and clippy clean, suite **734 / 2 / 1221**, `contract` **14**. With the
pre-phase contract restored, the new test fails.

## Pre-flight

1. `cargo test -p rexymcp-executor contract` → **13 passed**.
2. Full `cargo test` → **734 / 2 / 1220 passed**.

## Spec

Apply these replace blocks exactly; each "Replace" text occurs once.
Keep the new contract sentence on a single line — the test's `contains` does
not match across a line break.

### `executor/src/agent/contract.rs`

**1.** Replace:

```rust
    }

    #[test]
    fn contract_requires_pasting_pinned_counts() {
```

with:

```rust
    }

    #[test]
    fn contract_places_entries_below_the_marker() {
        let commands = CommandConfig::default();
        let output = assemble_executor_contract(&commands);
        assert!(
            output.contains(
                "below the `<!-- entries appended below this line -->` marker — never above it"
            ),
            "contract must say where new Update Log entries go"
        );
    }

    #[test]
    fn contract_requires_pasting_pinned_counts() {
```

### `executor/templates/executor_contract.md`

**2.** Replace:

```markdown

The Update Log is **append-only**. Never edit prior entries.

### Resuming a phase
```

with:

```markdown

The Update Log is **append-only**. Never edit prior entries.
Add every new entry at the end of the file, below the `<!-- entries appended below this line -->` marker — never above it.

### Resuming a phase
```

**Must NOT:**

- Add any `{…}` text to the contract
  (`placeholder_set_is_exactly_the_four_authorized` fails on it).
- Change any other contract wording.

## Acceptance criteria

- [ ] The assembled contract contains
      `below the \`<!-- entries appended below this line -->\` marker — never above it`.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

**Finish condition:** `cargo test -p rexymcp-executor contract` → **14 passed**;
full `cargo test` → **734 / 2 / 1221 passed**. Paste the `test result:` lines.

**Put your own Update Log entries at the very end of this file.** The marker
text also appears inside this doc's Spec code blocks; do not anchor an edit on
it — append after the last line.

## End-to-end verification

Paste `cargo test -p rexymcp-executor contract_places -- --nocapture` in a
`### Update — YYYY-MM-DD HH:MM (end-to-end verification)` entry.

## Authorizations

- `executor/templates/executor_contract.md` and `executor/src/agent/contract.rs` as specified.

## Out of scope

- Any other contract change.

## Update Log

<!-- entries appended below this line -->

### Update — 2026-09-19 05:19

Started F16 phase-01 (marker rule): flipping status to in-progress, then adding the
"entries go below the `<!-- entries appended below this line -->` marker — never above
it" sentence to the executor contract template and pinning it with
`contract_places_entries_below_the_marker`.

### Update — 2026-09-19 05:19 (end-to-end verification)

Pinned literal landed in the template (grep):

```
executor/templates/executor_contract.md:117:Add every new entry at the end of the file, below the `<!-- entries appended below this line -->` marker — never above it.
```

End-to-end verification — `cargo test -p rexymcp-executor contract_places -- --nocapture`:

```
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.19s
     Running unittests src/lib.rs (target/debug/deps/executor-090557a76b736c2e)

running 1 test
test agent::contract::tests::contract_places_entries_below_the_marker ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1230 filtered out; finished in 0.00s
```
