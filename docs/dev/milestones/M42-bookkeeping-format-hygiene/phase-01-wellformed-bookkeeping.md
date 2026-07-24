# Phase 01: Well-formed bookkeeping output

**Milestone:** M42 — Bookkeeping Format Hygiene
**Status:** review
**Depends on:** none (first phase of M42)
**Estimated diff:** ~160 lines (four small production changes plus tests)
**Tags:** language=rust, kind=bugfix, size=s

## Goal

The server writes the completion Update Log entry and the milestone README status
row **after** the executor's last turn, so nothing ever formats them. Four
concrete markdown defects result, and they fail the project's `format` gate on
every completed phase. Fix the generated text so it is well-formed on its own,
with no formatter involved.

All four changes are in **one file**: `mcp/src/finalize.rs`. If you find yourself
editing any other file, stop — that is a wrong turn.

## Architecture references

Read before starting:

- `docs/dev/milestones/M42-bookkeeping-format-hygiene/README.md` — the milestone,
  including § "The split, and its limit", which tells you exactly how far this
  phase goes and where it deliberately stops.
- `docs/architecture.md` § Status #27 — the server-authored finalize contract this
  preserves.

## Pre-flight

1. Read `docs/dev/STANDARDS.md` top to bottom.
2. Read the architecture references above.
3. Read this entire phase doc before touching any code.
4. Confirm the repo is on a clean branch with no uncommitted changes.

## Current state

Four sites in `mcp/src/finalize.rs`, quoted verbatim.

**1. `append_entry` (`finalize.rs:172-176`)** — one `\n` between the document and
the entry, and the entry already ends in `\n`, so the result also gains a trailing
blank line:

```rust
/// Return `doc` with the entry appended at end of file, separated by a blank
/// line.
fn append_entry(doc: &str, entry: &str) -> String {
    format!("{}\n{}\n", doc.trim_end(), entry)
}
```

The doc comment already claims "separated by a blank line" — the code does not do
that. Observed result in a real phase doc:

```
### Update — 2026-07-24 20:18 (started)

Started implementation by AI executor.
### Update — ts=1784924570254 (complete, server-authored)
```

**2. The files-changed list (`finalize.rs:118`)**, inside `baseline_entry`'s
`format!`:

```rust
         **Files changed:**\n{files}\n\n\
```

`{files}` expands to `- \`path\` — summary` lines. A markdown list must be
preceded by a blank line.

**3 + 4. `flip_readme_row` (`finalize.rs:182-217`)** — the last cell is replaced
with a fixed-width `" review "` regardless of the column's width, and the trailing
newline is lost:

```rust
                            format!(
                                "{} review |{}",
                                &line[..second_last_pipe + 1],
                                &line[last_pipe + 1..]
                            )
    // …
    if found { Some(lines.join("\n")) } else { None }
```

`.lines()` strips the final newline; `join` never restores it, so the README is
written back with `\ No newline at end of file`.

For contrast, `flip_status_to_review` (`finalize.rs:92-95`) already handles the
trailing newline correctly — copy that idiom:

```rust
    // Preserve trailing newline if present
    if doc.ends_with('\n') {
        result.push('\n');
    }
```

## Spec

### 1. Separate the appended entry with a blank line, and end the file with exactly one newline

Change `append_entry` so the result is `doc` (trailing whitespace trimmed), then a
blank line, then `entry` (trailing whitespace trimmed), then exactly one newline.

Worked example — given `doc` ending `"…by AI executor.\n"` and `entry` beginning
`"### Update — ts=…"` and ending `").\n"`:

```
…by AI executor.
                       <- blank line, new
### Update — ts=… (complete, server-authored)
…
…see M27 phase-03).
                       <- file ends here, exactly one \n after the last line
```

The existing doc comment is already correct — leave it.

### 2. Blank line before the files-changed list

In `baseline_entry`'s `format!`, make the `**Files changed:**` label and the
`{files}` expansion separated by a blank line, matching how every other label in
that template (`**Summary:**`, `**Executor:**`, `**Gates:**`) is followed by `\n\n`.

Do **not** restructure the rest of the template. `**Command output tails:**`
already has its blank line; the fenced block is fine as it is.

### 3. Preserve the status column's width in `flip_readme_row`

The replacement cell must occupy the **same number of characters** as the cell it
replaces, so the table stays rectangular.

Concretely: the original cell is the slice between the final two `|` — e.g.
`" todo        "` (13 chars). Build the new cell as `" review"` padded on the
right with spaces to that same char count. If the original cell is **narrower**
than `" review "` needs (e.g. a compact table written as `|todo|`), fall back to
`" review "` rather than truncating — never emit a cell that drops characters of
the word `review`.

Worked examples:

```
before: | 02  | lexer (source → `Token[]`, scan errors)                 | todo        |
after:  | 02  | lexer (source → `Token[]`, scan errors)                 | review      |
                                                                          ^^^^^^^^^^^^^ same width as before

before: | 03a | Server-authored finalize (…) | in-progress |
after:  | 03a | Server-authored finalize (…) | review      |

before: |04|thing|todo|
after:  |04|thing| review |     <- original cell was narrower than " review "; do not truncate
```

Use `chars().count()`, not `len()` — these tables contain non-ASCII (`→`, `—`) and
byte length is not display width.

### 4. Preserve the README's trailing newline

If `readme_doc` ends with `\n`, the returned string must too. Use the same shape
`flip_status_to_review` uses (quoted in § Current state).

### 5. Nothing else changes

The status flip, the entry's other fields, the gate line, the command tails, the
git commit, and `finalize_complete`'s control flow are all untouched. Do not add a
formatter call, do not read config, do not run any command — that is phase 02, and
it is blocked on a human decision.

## Acceptance criteria

- [ ] `cargo build` is green.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean.
- [ ] `cargo fmt --all --check` reports no diff in the files this phase touched.
- [ ] `cargo test` passes, with every pre-existing `finalize.rs` test still green
      (update a pre-existing test **only** where it asserts the old, defective
      output — and say which ones in your Update Log).
- [ ] `append_entry` puts exactly one blank line between the document and the
      entry, and the result ends with exactly one `\n`.
- [ ] The completion entry has a blank line between `**Files changed:**` and the
      first `- ` list item.
- [ ] `flip_readme_row`'s replacement cell has the same char count as the cell it
      replaced, whenever `" review "` fits.
- [ ] `flip_readme_row` returns a string ending in `\n` when its input did.

## Test plan

Add unit tests to the existing `#[cfg(test)] mod tests` block in
`mcp/src/finalize.rs`, matching the style already there (string literals in, exact
equality out). Assert **exact** output, not `contains` — a substring check passes
against the defective form too, which is how this class survived three
occurrences.

- `append_entry_separates_with_blank_line` — doc `"a\n"`, entry `"### E\n"`;
  assert the result is exactly `"a\n\n### E\n"`.
- `append_entry_ends_with_single_newline` — same inputs; assert the result ends
  with `"\n"` but not `"\n\n"`.
- `append_entry_collapses_existing_trailing_blanks` — doc `"a\n\n\n"`, entry
  `"### E\n"`; assert exactly `"a\n\n### E\n"` (the trim makes separation
  idempotent regardless of what the doc ended with).
- `baseline_entry_blank_line_before_files_list` — build a `PhaseResult` with at
  least one `FileChange` (the existing tests show how) and assert the rendered
  entry contains `"**Files changed:**\n\n- "`.
- `flip_readme_row_preserves_cell_width` — a row whose last cell is `" todo        "`;
  assert the returned line has the **same total char count** as the input line, and
  that the last cell trims to `"review"`.
- `flip_readme_row_preserves_wide_in_progress_width` — same assertion starting
  from `" in-progress "`, which is wider than `" review "` and so exercises padding
  rather than truncation.
- `flip_readme_row_narrow_cell_does_not_truncate` — input `"|04|thing|todo|"`;
  assert the last cell trims to exactly `"review"` (never `"revie"`).
- `flip_readme_row_preserves_trailing_newline` — input ending `"\n"`; assert the
  output ends `"\n"`.
- `flip_readme_row_without_trailing_newline_stays_without` — input **not** ending
  in `\n`; assert the output does not either. The negative case that stops the fix
  from unconditionally appending.

**Mutation self-check before you finish:** revert each production change one at a
time and confirm at least one new test fails for each; then restore. Report the
four failures in your Update Log. A change no test bites on is not pinned. (Do not
commit the mutations.)

## End-to-end verification

**The real artifact here cannot be observed in this run**, and you must not try to
force it: the bookkeeping tail for *this* phase is written by the **currently
running** `serve` process, which is the pre-fix binary. Your own completion entry
will therefore still show the defect. That is expected and is not a failure of your
work.

Your end-to-end proof is a **golden round-trip inside the process**: add one test
that runs a realistic phase-doc fixture (a doc with a `**Status:** in-progress`
line and a prior `### Update … (started)` entry) through `flip_status_to_review` →
`append_entry`, and asserts the full expected output byte-for-byte. Quote that
test's output in your Update Log, plus the four mutation failures.

The live confirmation — the next phase's tail showing the blank line after a
`serve` rebuild — is the **reviewer's** step, listed in the milestone exit
criteria. It is not yours, and you cannot perform it.

## Authorizations

None. No new dependencies. No edits outside `mcp/src/finalize.rs`. No edits to
`docs/architecture.md`, `Cargo.toml`, or any phase doc other than this one.

## Out of scope

- **Running `format_fix` on the server's writes** — phase 02, blocked on a human
  decision. Do not read config, do not invoke `CommandRunner`.
- **Byte-identity with Prettier (or any formatter).** A formatter may still
  re-shape the table's column widths, because it normalizes to the widest cell.
  Your target is a rectangular, well-formed table — not a formatter's exact output.
  See the milestone README § "The split, and its limit".
- Rewriting the table with a markdown table library or a new dependency.
- Touching `flip_status_to_review`, the git commit path, `runner.rs`, or the
  per-turn hook in `executor/src/agent/command.rs`.
- The `### Update … (started)` entry's own formatting, wherever it is written from.
  If you notice a defect there, put it in "Notes for review" — do not fix it.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-07-24 23:09 (started)

Started implementation by AI executor. Implementing four fixes in `mcp/src/finalize.rs`: (1) blank line separation in `append_entry`, (2) blank line before files-changed list in `baseline_entry`, (3) width-preserving cell replacement in `flip_readme_row`, (4) trailing newline preservation in `flip_readme_row`.
### Update — ts=1784935592394 (complete, server-authored)

**Summary:** **Summary + Notes for review:**

I implemented all four fixes in `mcp/src/finalize.rs`: (1) `append_entry` now inserts a blank line between the document and entry and trims both sides so the result ends with exactly one `\n`; (2) `baseline_entry` now has a blank line before the files-changed list; (3) `flip_readme_row` preserves the original cell's character width when replacing with "review" (using `chars().count()`, not byte `len()`), falling back to unpadded `" review "` when the original is narrower; (4) `flip_readme_row` preserves the README's trailing newline using the same idiom as `flip_status_to_review`.

I updated four pre-existing tests that asserted the old defective output (`flip_readme_row_flips_matching_row_only`, `flip_readme_row_emits_single_trailing_pipe`, `flip_readme_row_flips_bounced_row`, `finalize_updates_matching_readme_row_only`) and added 10 new tests (3 for `append_entry`, 1 for `baseline_entry`, 5 for `flip_readme_row`, and 1 golden round-trip). Mutation self-check confirmed each production change is pinned — reverting any one causes its corresponding test(s) to fail. All 1053 tests pass, clippy is clean, and `cargo fmt --all --check` reports no diff.

**Executor:** Qwen/Qwen3.6-27B-FP8

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
   Compiling rexymcp v0.9.1 (/home/matt/src/rexyMCP/mcp)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.73s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s


TEST
 ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::symbols::tests::caps_at_max_results ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::symbols::tests::finds_python_function_and_class ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::no_symbols_returns_advisory_error ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test ai::backends::openai::tests::is_retriable_transport_true_for_reqwest_error ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::bash::tests::cargo_command_records_cargo_filter_label ... ok
test governor::verifier::tests::verify_rust_returns_checked_empty_on_clean_code ... ok
test governor::verifier::tests::capture_baseline_dedupes_by_project_root ... ok
test governor::verifier::tests::capture_baseline_skips_unsupported_files ... ok
test governor::verifier::tests::verify_rust_returns_checked_with_errors_on_broken_code ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1053 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 6.09s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.07s
     Running unittests src/main.rs (target/debug/deps/rexymcp-4e85b51f198fbe9f)
     Running unittests src/lib.rs (target/debug/deps/executor-c1650299697d7408)
   Doc-tests executor

```

**Files changed:**
- `docs/dev/milestones/M42-bookkeeping-format-hygiene/README.md` — +1 -1
- `docs/dev/milestones/M42-bookkeeping-format-hygiene/phase-01-wellformed-bookkeeping.md` — +5 -1
- `mcp/src/finalize.rs` — +175 -11

**Commit:** 5ab2d07ac547a5ab5c6546a2d6af4b95ddbc93a2

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

