# Phase 1: marker rule

**Milestone:** F16 — Log entry placement
**Status:** todo
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
