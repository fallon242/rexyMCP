# Phase 02: Format the server's own bookkeeping writes

**Milestone:** M42 — Bookkeeping Format Hygiene
**Status:** todo
**Depends on:** phase-01 (well-formed output first; this is the belt to that
braces)
**Estimated diff:** ~90 lines
**Tags:** language=rust, kind=feature, size=s

## ⛔ Do not dispatch yet

This phase is **blocked on a human decision** recorded in the milestone README
§ "Pre-dispatch decision required for phase 02". `format_fix` is a whole-repo
command string that cannot be scoped to two paths without parsing it, so running
it at finalize time may reformat files outside the phase's scope — the same hazard
that makes `cargo fmt --all` a standing prohibition in this repo.

The three options are (a) run it whole-repo, (b) gate it behind a new opt-in config
key, (c) ship phase 01 alone and close the milestone. The architect's
recommendation is **(c) pending evidence, then (b)** — run phase 01 against the
reporter's project first and see whether anything is actually left over.

The spec below is written for **(b)**, the opt-in form, because it is the only
option that is safe to run unattended. If the human picks (a) or (c), this doc
must be rewritten or deleted before dispatch — do not adapt it yourself.

## Goal

After the server writes the phase doc and milestone README, run the project's
configured doc-formatter over them, before the bookkeeping commit — so a project
whose formatter has conventions rexyMCP cannot predict still lands a clean
`format` gate.

## Architecture references

Read before starting:

- `docs/dev/milestones/M42-bookkeeping-format-hygiene/README.md` — especially the
  pre-dispatch decision this phase is gated on.
- `docs/architecture.md` § Status #27 — the server-authored finalize contract.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Confirm with the architect that the decision above has been made **and that
   option (b) was chosen**. If that confirmation is not in the phase doc as an
   Update Log entry, stop and file a blocker.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

`finalize_complete` (`mcp/src/finalize.rs:25-52`) writes the phase doc
(`:38`), optionally writes the README (`:46`), collects both into `staged`
(`:40-48`), and commits (`:50`). No formatting happens anywhere in that path.

`FinalizeInput` (`finalize.rs:7-16`) already carries `runner: &dyn CommandRunner`,
so command execution is available — what is missing is the command string.

The per-turn hook to mirror is `run_post_write_hooks`
(`executor/src/agent/command.rs:182-193`): it runs `lint_fix` then `format_fix`
via the runner and ignores the results.

`CommandConfig` (`executor/src/config.rs:473-480`) currently has `format`,
`build`, `lint`, `test`, `lint_fix`, `format_fix`.

The call site that builds `FinalizeInput` is `mcp/src/runner.rs:316-323`, which
has `cfg` in scope.

## Spec

### 1. New opt-in config key

Add `format_docs_fix: Option<String>` to `CommandConfig`. Default `None`, meaning
**no formatting of server writes** — the behavior before this phase. Document it
in the same style as its siblings, stating that it runs over the server's
bookkeeping writes at finalize time and that it is separate from `format_fix`
precisely so a whole-repo formatter is never run implicitly at commit time.

Editing `executor/src/config.rs` is **authorized for this phase** (see
§ Authorizations) — that file is otherwise off-limits.

### 2. Thread it into `FinalizeInput`

Add `format_docs_fix: Option<&'a str>` to `FinalizeInput` and populate it at
`mcp/src/runner.rs:316` from `cfg.commands.format_docs_fix.as_deref()`.

### 3. Run it after the writes, before the commit

In `finalize_complete`, after the README block and **before** `git_commit_docs`,
run the command when it is `Some`, via `inp.runner.run(cmd, inp.repo_root)`.
Ignore the result exactly as `run_post_write_hooks` does — a formatter failure
must never fail finalize or lose the bookkeeping commit.

Order matters: the formatter must run **after** both writes and **before** the
commit, or the commit captures unformatted content.

### 4. Nothing else changes

No change to what is staged, to the commit message, to the entry's content, or to
`run_post_write_hooks`. Do not call `format_fix` from finalize.

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes.
- [ ] With `format_docs_fix` unset, finalize runs exactly the commands it ran
      before — pinned by a test asserting the recorded command list is unchanged.
- [ ] With it set, the command runs once, after both writes and before the commit.

## Test plan

Use a recording `CommandRunner` fake (the existing finalize tests show the
pattern) so command order is observable without running anything real.

- `finalize_runs_no_doc_formatter_when_unset` — the recorded commands contain no
  formatter invocation. The negative case; it is what protects every existing
  project from a behavior change.
- `finalize_runs_doc_formatter_when_set` — the command appears exactly once.
- `finalize_runs_doc_formatter_before_commit` — assert the formatter's index in
  the recorded list is **less than** the `git commit` index. Order is the whole
  point; assert it directly rather than trusting call-site reading.
- `finalize_survives_doc_formatter_failure` — a runner whose formatter returns
  failure still produces the commit and returns `Ok(true)`.

## End-to-end verification

Against a scratch repo with `format_docs_fix` set to a command that leaves an
observable trace (e.g. `sh -c 'echo formatted >> .rexymcp-fmt-marker'`), run a
finalize and show the marker exists and the commit contains the doc changes. Quote
the actual output.

## Authorizations

- [x] May edit `executor/src/config.rs` — **only** to add the `format_docs_fix`
      field to `CommandConfig` and its documentation. No other change to that file.

No new dependencies. No edits to `docs/architecture.md`.

## Out of scope

- Invoking the existing whole-repo `format_fix` from finalize — that is option (a),
  which the human did not choose if you are running this doc.
- Scoping, parsing, or rewriting the user's command string.
- Formatting anything other than the two bookkeeping paths' repo.
- Changing `run_post_write_hooks` or the per-turn hook timing.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->
