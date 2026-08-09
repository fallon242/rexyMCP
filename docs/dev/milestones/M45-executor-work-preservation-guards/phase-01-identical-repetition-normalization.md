# Phase 01: Identical-repetition argument normalization

**Milestone:** M45 — Executor Work-Preservation Guards
**Status:** in-progress
**Depends on:** none
**Estimated diff:** ~170 lines
**Tags:** language=rust, kind=feature, size=s

## Goal

`check_identical_repetition` compares raw `serde_json::Value` arguments, so a
mutating loop whose calls differ only by whitespace — a re-issued `patch` with a
reflowed `old_str`, a `write_file` whose content gained a trailing newline —
never trips `identical_call_threshold` and runs until some other terminator
catches it (downstream, one such loop ran 529 turns before a human stopped it).
Normalize string-leaf whitespace before comparing so the detector sees the loop.

## Architecture references

Read before starting:

- `docs/architecture.md` §M4 — the governor's place in the turn cycle; the
  hard-fail signal is what produces the escalation briefing.
- `docs/architecture.md` §M37 — the read-only calibration this phase must not
  disturb: read-only windows are deliberately exempt from this detector.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

Everything this phase changes lives in
`executor/src/governor/hard_fail.rs` (1366 lines; line numbers are
current as of drafting — re-derive with
`grep -n 'fn check_identical_repetition' executor/src/governor/hard_fail.rs`).

The detector, at `:137–165`:

```rust
/// Identical repetition: the last `threshold` tool calls are all the same
/// `(tool, arguments)` pair. Fires when `threshold` identical calls are seen.
/// Read-only repetitions are exempt — left to `check_read_only_stall`.
fn check_identical_repetition(
    recent: &VecDeque<ToolCallSnapshot>,
    threshold: usize,
) -> Option<HardFailSignal> {
    if recent.len() < threshold {
        return None;
    }
    // Read-only repetition is diagnosis, not thrash — left to check_read_only_stall.
    if !window_has_mutation(recent, threshold) {
        return None;
    }
    let last_n: Vec<_> = recent.iter().rev().take(threshold).collect();
    let first = &last_n[0];
    let all_identical = last_n
        .iter()
        .all(|c| c.tool == first.tool && c.arguments == first.arguments);
    if !all_identical {
        return None;
    }
    Some(HardFailSignal::IdenticalToolCallRepetition {
        tool: first.tool.clone(),
        consecutive_count: threshold as u32,
    })
}
```

The snapshot type, at `:9–14`:

```rust
pub struct ToolCallSnapshot {
    pub tool: String,
    pub arguments: serde_json::Value,
    pub succeeded: bool,
}
```

`GovernorConfig::identical_call_threshold` defaults to **6**
(`executor/src/config.rs:271`).

### Two facts that will otherwise cost you a wrong diagnosis

**1. Argument *ordering* is already normalized — do not write code for it.**
`serde_json` is declared as plain `serde_json = "1"` (`Cargo.toml:14`) with no
`preserve_order` feature anywhere in the workspace, so `serde_json::Map` is a
`BTreeMap` and `Value` equality already ignores insertion order. A test asserting
key-order insensitivity passes today, before any change. The gap is *string-leaf
whitespace*, and only that.

**2. The detector is exempt on windows with no file-mutating call, so your test
fixture must use a mutating tool.** `window_has_mutation` (`:241`) asks
`crate::tools::mutates_files(&c.tool)`, which is true only for
`Category::Write` — `write_file`, `patch`, `patch_lines`, `delete_file`,
`move_file` (`executor/src/tools/router.rs:14–31`). **`bash` is
`Category::Run`, not Write.** A window of six whitespace-varied `bash` calls
returns `None` from this detector by design (M37 routes those to
`check_read_only_stall`), so a `bash`-based fixture makes the new test
unsatisfiable and there is no production bug behind that failure. Build the
fixture from `patch` calls.

**3. If a compile error appears, read the compiler message to locate it.** Do
not hunt for a syntax problem by re-reading the file in a loop.

## Spec

### 1. Add the `normalize_arguments` helper

In `executor/src/governor/hard_fail.rs`, add this function immediately after
`check_identical_repetition`. **Write it verbatim** — Task 5's mutation proof
greps for these exact lines, so a reworded body breaks the evidence chain:

```rust
/// Normalize tool-call arguments for identical-repetition comparison: every
/// string leaf is trimmed and its internal whitespace runs collapsed to a single
/// space, recursively through objects and arrays. Non-string leaves are returned
/// unchanged. Object key order needs no handling — `serde_json::Map` is a
/// `BTreeMap` (the `preserve_order` feature is off), so `Value` equality already
/// ignores insertion order.
fn normalize_arguments(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::String(s) => Value::String(s.split_whitespace().collect::<Vec<_>>().join(" ")),
        Value::Array(items) => Value::Array(items.iter().map(normalize_arguments).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), normalize_arguments(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}
```

`split_whitespace().collect::<Vec<_>>().join(" ")` trims and collapses in one
step: `"  cargo   test\n foo "` becomes `"cargo test foo"`.

### 2. Compare through the normalizer

In `check_identical_repetition`, replace the raw comparison with a normalized
one, hoisting the reference value so it is computed once:

```rust
    let last_n: Vec<_> = recent.iter().rev().take(threshold).collect();
    let first = &last_n[0];
    let first_args = normalize_arguments(&first.arguments);
    let all_identical = last_n
        .iter()
        .all(|c| c.tool == first.tool && normalize_arguments(&c.arguments) == first_args);
```

Update the doc comment on `check_identical_repetition` to say the `(tool,
arguments)` comparison is whitespace-normalized. Change nothing else in the
function — the length guard, the `window_has_mutation` exemption, and the
returned signal all stay exactly as they are.

### 3. Positive tests

In the `#[cfg(test)] mod tests` block at the bottom of the same file
(`:441`), following the existing style of `detects_identical_repetition`
(`:483`) and `identical_repetition_still_fires_for_write_tool` (`:1268`):

- `normalize_arguments_collapses_whitespace_in_string_leaves`
- `normalize_arguments_recurses_through_objects_and_arrays`
- `identical_repetition_fires_on_whitespace_varied_arguments` — six `patch`
  snapshots whose `old_str` differs only in indentation, internal spacing, and a
  trailing newline; `evaluate(&recent, &[], None, &GovernorConfig::default())`
  must return `IdenticalToolCallRepetition { tool, consecutive_count: 6 }` with
  `tool == "patch"`.

### 4. Negative tests

The boundary is where this change can do damage — a false hard_fail kills a run
that was making progress. Pin all three:

- `identical_repetition_ignores_non_whitespace_argument_differences` — six
  `write_file` snapshots whose `content` differs substantively (`"fn a() {}"`,
  `"fn b() {}"`, …) must return `None` from `evaluate`.
- `normalize_arguments_leaves_non_string_leaves_unchanged` — numbers, booleans,
  and `null` round-trip identically; a number is not stringified.
- `identical_repetition_still_exempts_read_only_window` — six `read_file`
  snapshots whose `path` differs only in whitespace must **still** return `None`
  (the M37 exemption is unaffected by normalization). Distinct from the existing
  `identical_repetition_exempts_read_only_window` (`:1221`), which uses
  byte-identical calls; leave that test as it is.

### 5. Mutation — apply, and prove it applied

Use the `patch` tool. **Do not use `sed -i` / `perl -i` / a `>` redirect into
the source file** — the executor contract bans in-place shell edits and `bash`
will refuse them.

Apply this patch to `executor/src/governor/hard_fail.rs`:

- `old_str`: `        Value::String(s) => Value::String(s.split_whitespace().collect::<Vec<_>>().join(" ")),`
- `new_str`: `        Value::String(s) => Value::String(s.clone()),`

Then run, appending to the artifact (the `## End-to-end verification` block
creates it):

```bash
A=.rexymcp/e2e-45-01.txt
echo "=== [4] MUTATED: normalizer neutered ===" >> $A
echo -n "grep-fixed-form=" >> $A
grep -cF 'split_whitespace().collect::<Vec<_>>().join(" ")' executor/src/governor/hard_fail.rs >> $A
echo -n "grep-mutated-form=" >> $A
grep -cF 'Value::String(s.clone())' executor/src/governor/hard_fail.rs >> $A
cargo test -p rexymcp-executor identical_repetition > .rexymcp/t-mut.log 2>&1; echo "exit=$?" >> $A
tail -25 .rexymcp/t-mut.log >> $A
```

`grep-fixed-form` must read `0` and `grep-mutated-form` must read `1` — that is
the proof the mutation landed on the intended line. `exit` must be **non-zero**,
and the tail must show
`identical_repetition_fires_on_whitespace_varied_arguments` failing on its
assertion (not on a compile error). If the test passes here, the test is not
exercising the normalizer: fix the test, do not weaken it, and do not proceed.

### 6. Mutation — restore, and prove the restore applied

Apply the inverse `patch` (swap `old_str` and `new_str` from Task 5). **Do not
use `git checkout`/`git restore` to undo it** — the file holds this round's own
uncommitted work and the runtime will refuse the command. Then:

```bash
A=.rexymcp/e2e-45-01.txt
echo "=== [5] RESTORED ===" >> $A
echo -n "grep-fixed-form=" >> $A
grep -cF 'split_whitespace().collect::<Vec<_>>().join(" ")' executor/src/governor/hard_fail.rs >> $A
echo -n "grep-mutated-form=" >> $A
grep -cF 'Value::String(s.clone())' executor/src/governor/hard_fail.rs >> $A
cargo test -p rexymcp-executor identical_repetition > .rexymcp/t-res.log 2>&1; echo "exit=$?" >> $A
tail -15 .rexymcp/t-res.log >> $A
```

`grep-fixed-form` must read `1`, `grep-mutated-form` `0`, and `exit=0`.

### 7. Capture the end-to-end evidence

Run the block in § End-to-end verification **verbatim and unmodified** (it runs
first and seeds the artifact; Tasks 5 and 6 append to it), then paste the
resulting `.rexymcp/e2e-45-01.txt` into a new Update Log entry headed
`### Update — <date> (end-to-end verification)`, inside a single fenced code
block. The server-authored `(complete)` entry does not satisfy this.

### 8. Prove the paste is byte-identical

```bash
D=docs/dev/milestones/M45-executor-work-preservation-guards/phase-01-identical-repetition-normalization.md
START=$(grep -n '^### Update .*(end-to-end verification)' $D | tail -1 | cut -d: -f1)
tail -n +$START $D | awk '/^```/{n++; next} n==1' > .rexymcp/pasted-45-01.txt
diff .rexymcp/pasted-45-01.txt .rexymcp/e2e-45-01.txt && echo "PASTE MATCH" || echo "PASTE MISMATCH"
```

On `PASTE MISMATCH` the `diff` names the lines that drifted — fix them from
`.rexymcp/e2e-45-01.txt`, do not retype from memory, and re-run until it prints
`PASTE MATCH`. Quote the `PASTE MATCH` line in the completion entry.

## Acceptance criteria

- [ ] `cargo fmt --all --check` reports no diffs.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo test` is green.
- [ ] Test `identical_repetition_fires_on_whitespace_varied_arguments` passes.
- [ ] Test `identical_repetition_ignores_non_whitespace_argument_differences`
      passes.
- [ ] Test `identical_repetition_still_exempts_read_only_window` passes.
- [ ] `grep -cF 'c.arguments == first.arguments'
      executor/src/governor/hard_fail.rs` returns `0` (it returns `1` on the
      pre-phase tree).
- [ ] In the artifact's `[2]` section, the `normalize_arguments` run reports
      **3 passed** and the `identical_repetition` run reports **8 passed**
      (5 pre-existing, measured on the pre-phase tree, + 3 new). A name filter
      that matches no tests
      exits `0` with `0 passed`, so the counts — not the exit codes — are what
      prove the tests exist.
- [ ] The artifact `.rexymcp/e2e-45-01.txt` contains a `[4] MUTATED` section
      with `grep-mutated-form=1` and a non-zero `exit`, and a `[5] RESTORED`
      section with `grep-fixed-form=1` and `exit=0`.
- [ ] An Update Log entry headed `### Update — <date> (end-to-end
      verification)` contains the artifact verbatim.
- [ ] Task 8 prints `PASTE MATCH`.

## Test plan

- `normalize_arguments_collapses_whitespace_in_string_leaves` in
  `executor/src/governor/hard_fail.rs` — asserts `"  a   b\nc "` normalizes to
  `"a b c"`.
- `normalize_arguments_recurses_through_objects_and_arrays` in the same file —
  asserts a nested object and an array of strings are normalized at every depth.
- `normalize_arguments_leaves_non_string_leaves_unchanged` — asserts numbers,
  booleans, and `null` round-trip unchanged.
- `identical_repetition_fires_on_whitespace_varied_arguments` — asserts six
  whitespace-varied `patch` calls yield
  `IdenticalToolCallRepetition { tool: "patch", consecutive_count: 6 }`.
- `identical_repetition_ignores_non_whitespace_argument_differences` — asserts
  six `write_file` calls with substantively different `content` yield `None`.
- `identical_repetition_still_exempts_read_only_window` — asserts six
  whitespace-varied `read_file` calls yield `None`.

## End-to-end verification

The change is library-internal with no CLI surface, so **the mutation pair is
the verification** (Tasks 5 and 6). Run this block first — it creates the
artifact those tasks append to.

```bash
mkdir -p .rexymcp
A=.rexymcp/e2e-45-01.txt
: > $A
echo "=== [1] gates ===" >> $A
cargo fmt --all --check > .rexymcp/g-fmt.log 2>&1; echo "fmt exit=$?" >> $A
tail -5 .rexymcp/g-fmt.log >> $A
cargo clippy --all-targets --all-features -- -D warnings > .rexymcp/g-clippy.log 2>&1; echo "clippy exit=$?" >> $A
tail -5 .rexymcp/g-clippy.log >> $A
cargo test > .rexymcp/g-test.log 2>&1; echo "test exit=$?" >> $A
tail -12 .rexymcp/g-test.log >> $A
echo "=== [2] new + existing detector tests ===" >> $A
cargo test -p rexymcp-executor identical_repetition > .rexymcp/t-fix.log 2>&1; echo "exit=$?" >> $A
tail -20 .rexymcp/t-fix.log >> $A
cargo test -p rexymcp-executor normalize_arguments > .rexymcp/t-norm.log 2>&1; echo "exit=$?" >> $A
tail -12 .rexymcp/t-norm.log >> $A
echo "=== [3] raw comparison is gone ===" >> $A
echo -n "raw-compare-count=" >> $A
grep -cF 'c.arguments == first.arguments' executor/src/governor/hard_fail.rs >> $A
```

`raw-compare-count` must read `0`. Note `grep -c` exits non-zero on zero
matches; that is expected here and does not abort the block.

## Authorizations

None. No dependency changes, no `Cargo.toml` edit, no `#[allow]`. Edits are
confined to `executor/src/governor/hard_fail.rs` and this phase doc's Update
Log.

## Out of scope

- **The git self-revert guard.** Already shipped in M22 phase-05
  (`executor/src/agent/tools.rs:107`). Do not add, extend, or duplicate it, and
  do not touch `executor/src/security/bash_classify.rs`.
- **`check_oscillation`, `normalize_target`, `check_read_only_stall`, and the
  low-novelty detector.** They have their own normalization (M37) and are
  deliberately left alone; do not route them through `normalize_arguments`.
- **The `rmcp` 2.2 → 3.1.2 upgrade** — that is M45 phase-02.
- **Any `Cargo.lock` change**, including `generic-array` (dropped from the
  milestone; see the M45 README's Notes for why it cannot resolve).
- Making the normalization configurable. A `GovernorConfig` knob is not wanted;
  if you think one is needed, file a blocker instead of adding it.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-08-09 18:44 (started)

**Executor:** Claude (Sonnet)
**Action:** Started phase 01 — implementing `normalize_arguments` and 6 new tests in `hard_fail.rs`.

### Update — 2026-08-09 18:44 (end-to-end verification)

```
=== [1] gates ===
fmt exit=0
clippy exit=0
    Checking rexymcp-executor v0.9.1 (/home/matt/src/rexyMCP/executor)
    Checking rexymcp v0.9.1 (/home/matt/src/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.18s
test exit=0
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1068 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.10s

   Doc-tests executor

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

=== [2] new + existing detector tests ===
exit=0
   Compiling rexymcp-executor v0.9.1 (/home/matt/src/rexyMCP/executor)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.49s
     Running unittests src/lib.rs (target/debug/deps/executor-300152e5d9c0cc60)

running 8 tests
test governor::hard_fail::tests::describe_identical_repetition ... ok
test governor::hard_fail::tests::identical_repetition_exempts_read_only_window ... ok
test governor::hard_fail::tests::identical_repetition_window_is_threshold_not_deque_length ... ok
test governor::hard_fail::tests::identical_repetition_still_exempts_read_only_window ... ok
test governor::hard_fail::tests::identical_repetition_still_fires_for_write_tool ... ok
test governor::hard_fail::tests::identical_repetition_ignores_non_whitespace_argument_differences ... ok
test governor::hard_fail::tests::detects_identical_repetition ... ok
test governor::hard_fail::tests::identical_repetition_fires_on_whitespace_varied_arguments ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 1062 filtered out; finished in 0.00s

exit=0
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.05s
     Running unittests src/lib.rs (target/debug/deps/executor-300152e5d9c0cc60)

running 3 tests
test governor::hard_fail::tests::normalize_arguments_collapses_whitespace_in_string_leaves ... ok
test governor::hard_fail::tests::normalize_arguments_leaves_non_string_leaves_unchanged ... ok
test governor::hard_fail::tests::normalize_arguments_recurses_through_objects_and_arrays ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 1067 filtered out; finished in 0.00s

=== [3] raw comparison is gone ===
raw-compare-count=0
=== [4] MUTATED: normalizer neutered ===
grep-fixed-form=0
grep-mutated-form=1
exit=101
running 8 tests
test governor::hard_fail::tests::describe_identical_repetition ... ok
test governor::hard_fail::tests::identical_repetition_still_exempts_read_only_window ... ok
test governor::hard_fail::tests::identical_repetition_window_is_threshold_not_deque_length ... ok
test governor::hard_fail::tests::identical_repetition_exempts_read_only_window ... ok
test governor::hard_fail::tests::identical_repetition_still_fires_for_write_tool ... ok
test governor::hard_fail::tests::identical_repetition_ignores_non_whitespace_argument_differences ... ok
test governor::hard_fail::tests::detects_identical_repetition ... ok
test governor::hard_fail::tests::identical_repetition_fires_on_whitespace_varied_arguments ... FAILED

failures:

---- governor::hard_fail::tests::identical_repetition_fires_on_whitespace_varied_arguments stdout ----

thread 'governor::hard_fail::tests::identical_repetition_fires_on_whitespace_varied_arguments' (3759654) panicked at executor/src/governor/hard_fail.rs:1458:14:
identical repetition must fire on whitespace-varied arguments
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    governor::hard_fail::tests::identical_repetition_fires_on_whitespace_varied_arguments

test result: FAILED. 7 passed; 1 failed; 0 ignored; 0 measured; 1062 filtered out; finished in 0.00s

error: test failed, to rerun pass `-p rexymcp-executor --lib`
=== [5] RESTORED ===
grep-fixed-form=1
grep-mutated-form=0
exit=0
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.51s
     Running unittests src/lib.rs (target/debug/deps/executor-300152e5d9c0cc60)

running 8 tests
test governor::hard_fail::tests::describe_identical_repetition ... ok
test governor::hard_fail::tests::identical_repetition_exempts_read_only_window ... ok
test governor::hard_fail::tests::identical_repetition_still_exempts_read_only_window ... ok
test governor::hard_fail::tests::identical_repetition_window_is_threshold_not_deque_length ... ok
test governor::hard_fail::tests::identical_repetition_ignores_non_whitespace_argument_differences ... ok
test governor::hard_fail::tests::identical_repetition_still_fires_for_write_tool ... ok
test governor::hard_fail::tests::detects_identical_repetition ... ok
test governor::hard_fail::tests::identical_repetition_fires_on_whitespace_varied_arguments ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 1062 filtered out; finished in 0.00s

```
