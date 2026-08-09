# M45 — Executor Work-Preservation Guards

**Goal:** the identical-repetition detector stops missing trivially-varied
loops, and the MCP server's `rmcp` dependency moves to the current major
version.

**Status:** in-progress (opened 2026-08-09)

**Depends on:** none (builds on M22's self-revert guard and M37's governor
calibration)

**Origin:** DaemonEye's M12 upstream-folds proposal
(`docs/daemoneye-proposed-upstream-folds.md`, §4 "Executor self-sabotage on
delete-heavy rewrites is a runtime concern"), plus a dependency-refresh ask
added at milestone open.

## Current state (re-verified 2026-08-09 at milestone open)

The proposal's first ask — a git self-revert guard — **is already shipped**, and
the "verified 2026-08-09" note in this README's proposal draft was wrong: it
checked only `security/bash_classify.rs` and missed the guard one layer up in
the agent loop, where the design (`docs/architecture.md` §M4) actually puts it.

- **Self-revert guard: done in M22 phase-05.**
  `executor/src/agent/tools.rs:107` `destructive_restore_refusal` refuses
  `git checkout <path>`, `git restore <path>`, and `git checkout HEAD -- <path>`
  when a resolved path token is in the session's edited set, and refuses
  `git stash` push forms (`stashes_working_tree`, `:143`) whenever that set is
  non-empty. `restore_path_tokens` (`:174`) walks `&&` / `;` / `|`-joined
  segments and skips flags and the `--` separator. The refusal is a `String`
  surfaced as a model-visible `ToolResult` — not a `Result::Err` — and it names
  the remedy ("fix forward from the current state, and only commit if you need a
  checkpoint"), satisfying the proposal's third exit criterion too. Covered
  end-to-end by `executor/src/agent/tests.rs:4884
  self_revert_of_edited_file_is_refused`, plus the unit tests at
  `executor/src/agent/tools.rs:1048–1160`. The wholesale forms
  (`git checkout .`, `git restore .`, `git reset --hard`, `git clean -f`) are
  blocked separately by `executor/src/security/bash_classify.rs:83`, `:57`.
  **Do not re-implement any of this.**
- **Read-only stall on N consecutive non-writing calls: done in M37**
  (`read_only_stall_threshold` + the `normalize_target` machinery). Out of
  scope, per the original proposal.
- **Identical-repetition normalization: the real remaining gap.**
  `executor/src/governor/hard_fail.rs:154` compares raw
  `serde_json::Value` arguments, so a loop whose calls vary by whitespace or a
  line break never trips `identical_call_threshold`. Note that *argument
  ordering* is **not** part of the gap: `serde_json` is built without
  `preserve_order`, so its `Map` is a `BTreeMap` and `Value` equality already
  ignores insertion order.

## Exit criteria

- The identical-repetition detector normalizes tool-call arguments (string-leaf
  whitespace) before comparison, and a test demonstrates a loop of
  trivially-varied mutating calls now trips at `identical_call_threshold` where
  the pre-M45 comparison did not — proven by a both-directions mutation pair,
  not asserted.
- Substantively different arguments still compare as distinct: a repeated
  `write_file` whose `content` genuinely differs does **not** trip the detector.
- `rmcp` is on 3.x (currently `2.2` in `mcp/Cargo.toml`), the server builds
  clean under `-D warnings`, and every existing `mcp` test passes unmodified.

## Architecture references

- `docs/architecture.md` §M4 (self-revert refusal — already shipped),
  §M37 (governor read-only calibration), §M45

## Phases

| #  | Phase                                                                                              | Status |
|----|----------------------------------------------------------------------------------------------------|--------|
| 01 | identical-repetition-normalization ([phase-01](phase-01-identical-repetition-normalization.md))     | review        |
| 02 | rmcp 3.1.2 migration (not yet drafted)                                                              | —      |

## Notes

**The hard-block vs. checkpoint-then-allow fork is moot.** M22 already chose
hard-block-with-advisory and it has been in production since. Revisit only if
refusals prove disruptive in practice.

**`generic-array` 0.14.9 is unreachable — dropped from scope, not deferred.**
The bump was requested at milestone open; it does not resolve. `crypto-common
v0.1.7` requires `generic-array = "=0.14.7"` (an exact pin), and 0.1.7 is the
latest release in the 0.1.x line, so `cargo update -p crypto-common` is a no-op.
The chain is `rexymcp → ratatui 0.30 → ratatui-termwiz → termwiz 0.23 → sha2
0.10.9 → digest 0.10.7 → crypto-common 0.1.7 → generic-array =0.14.7`. Verified:

```
$ cargo update --dry-run -p generic-array --precise 0.14.9
error: failed to select a version for the requirement `generic-array = "=0.14.7"`
candidate versions found which didn't match: 0.14.9
required by package `crypto-common v0.1.7`
```

A direct `[dependencies] generic-array = "0.14.9"` cannot force it either — the
exact `=0.14.7` requirement would conflict — and a `[patch.crates-io]` entry at
0.14.9 would not satisfy `=0.14.7`. The only real path is upstream: `termwiz`
moving to the `sha2` 0.11 / `crypto-common` 0.2 line, at which point the bump
happens for free on a `cargo update`. **Reopening trigger:** `cargo tree -i
generic-array` showing a dependent that is not `crypto-common 0.1.x`.

**Phase 02 (`rmcp` 2.2 → 3.1.2) is a major-version migration and needs its own
phase.** Resolution is confirmed feasible — `cargo add -p rexymcp rmcp@3.1.2
--features server,macros,transport-io --dry-run` succeeds — but the blast radius
is 45 `rmcp::` sites across `mcp/src/server.rs`, `mcp/src/main.rs`, and
`mcp/src/server_tests.rs`, including the `#[rmcp::tool_router]` / `#[rmcp::tool]`
macros, a hand-written `ServerHandler` impl, `schema_for_type` /
`schema_for_output`, and `rmcp::serve_server` + `QuitReason`. Per WORKFLOW
§ "Verify external APIs against live docs" the phase doc must carry a Pre-flight
step pointing at the live 3.x docs and the 2.x→3.x migration notes, with the
architect's sketch explicitly subordinate to them. Draft it after phase-01
closes, so the 3.x surface is fetched once against a settled tree.
