# Phase 1: drop debug print

**Milestone:** F10 — Remove PII debug print
**Status:** todo
**Depends on:** none
**Estimated diff:** 1 line
**Tags:** language=rust, kind=cleanup, size=xs

## Goal

Delete one debug line from a test in `executor/src/privacy/egress.rs`. It
prints every term the pre-scan detected, which is the contents of a PII
dictionary. `STANDARDS.md` §1 forbids debug prints in code, with no test
exemption.

## Pre-flight

1. `grep -c 'println!' executor/src/privacy/egress.rs` → **1** (measured 2026-09-18).
2. Full `cargo test` → **727 / 2 / 1210 passed**.

## Current state

`executor/src/privacy/egress.rs:579-583`, inside the `#[ignore]`d live test
`live_build_egress_index_keeps_project_names_out`:

```rust
        let idx = build_egress_index(&repo, &privacy).await.unwrap();
        let terms = idx.terms;
        let normalized: Vec<String> = terms.iter().map(|(t, _)| normalize(t)).collect();
        eprintln!("live terms: {terms:?}");

```

The assertions right below already print `terms` and `normalized` when they
fail, so nothing is lost.

## Spec

Delete exactly this line and nothing else:

```rust
        eprintln!("live terms: {terms:?}");
```

**Must NOT:**

- Change any other line, including the assertions and the blank line after.
- Run the `#[ignore]`d live test. It needs the detection engine, which the
  executor does not reach. The architect runs it at review.

## Acceptance criteria

- [ ] `grep -c 'println!' executor/src/privacy/egress.rs` → **0**.
- [ ] `grep -rn 'println!' executor/src/privacy/` → no output.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` pass.

## Test plan

No new tests. **Finish condition:** full `cargo test` → **727 / 2 / 1210
passed**, unchanged. Paste the four `test result:` lines.

## End-to-end verification

Paste the output of both `grep` commands from the acceptance criteria in an
`(end-to-end verification)` entry.

## Authorizations

None beyond the Spec.

## Out of scope

- Anything else in the privacy module.

## Update Log

<!-- entries appended below this line -->
