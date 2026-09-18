# Phase 4: make the prompt guard fail closed, and ship it

**Milestone:** F05 — Privacy and security hardening
**Status:** review
**Depends on:** nothing in F05.
**Estimated diff:** ~180 lines: a script rewrite, one new file, one doc section.
**Tags:** language=bash, kind=security, size=s

> **Dispatch on a LOCAL executor only.** This is the guard on the architect's
> own prompt.

## Goal

`plugin/hooks/pii-guard.sh` is the last thing between a typed prompt and a cloud
architect. Five defects (F05 README, finding 4):

1. Reads `.user_input`; a payload-key rename silently disables it.
2. Needs `jq`. Without it the script exits non-zero, which the harness treats as
   a hook error, not a block — the prompt goes through.
3. `grep` matches line by line, so PII split across a newline is missed.
4. The card pattern wants exactly 16 digits (AmEx is 15, Diners 14).
5. SSN and phone patterns require punctuation, which raw extracts lack.

And it is not installed: the operator must find and copy it.

Defects 1-3 share one fix — **stop parsing the payload**. The prompt text is in
the raw JSON on stdin; scanning that needs no key, no `jq`, and no line
discipline.

## Current state

```bash
input="$(cat)"
prompt="$(printf '%s' "$input" | jq -r '.user_input // empty')"
[ -z "$prompt" ] && exit 0

if printf '%s' "$prompt" | grep -Eq \
  -e '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' \
  -e '[0-9]{3}-[0-9]{2}-[0-9]{4}' \
  -e '(\+?1[-. ])?(\([0-9]{3}\)|[0-9]{3})[-. ][0-9]{3}[-. ][0-9]{4}' \
  -e '[0-9]{4}([ -]?[0-9]{4}){3}'; then
```

Exit 2 blocks; every other non-zero is a non-blocking error. That asymmetry is
why defect 2 matters.

## Spec

### 1. Rewrite `plugin/hooks/pii-guard.sh`

Keep the header comment's honest framing (the hook can only block, not rewrite;
structured PII only; the CLI is the comprehensive path). Replace the body:

```bash
set -uo pipefail          # NOT -e: a failing grep must not exit the script

payload="$(cat)"

# Inert unless the project turns privacy on. The plugin installs this hook for
# everyone; blocking prompts in a project that never opted in would be rude.
toml="${CLAUDE_PROJECT_DIR:-.}/rexymcp.toml"
[ -f "$toml" ] || exit 0
awk '
  /^[[:space:]]*\[/ { in_privacy = ($0 ~ /^[[:space:]]*\[privacy\]/) }
  in_privacy && /^[[:space:]]*enabled[[:space:]]*=[[:space:]]*true/ { found = 1 }
  END { exit found ? 0 : 1 }
' "$toml" || exit 0

# Scan the RAW payload, newlines removed. No key to rename, no jq to be missing,
# and PII split across a line break still matches.
flat="$(printf '%s' "$payload" | tr -d '\n\r')"

hit=0
printf '%s' "$flat" | grep -Eq \
  -e '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' \
  -e '[0-9]{3}-[0-9]{2}-[0-9]{4}' \
  -e '(\+?1[-. ])?(\([0-9]{3}\)|[0-9]{3})[-. ][0-9]{3}[-. ][0-9]{4}' \
  -e '[0-9]{4}[ -]?[0-9]{6}[ -]?[0-9]{5}' \
  -e '[0-9]{4}[ -]?[0-9]{6}[ -]?[0-9]{4}' \
  -e '[0-9]{4}[ -]?[0-9]{4}[ -]?[0-9]{4}[ -]?[0-9]{2,7}' && hit=1

# Bare digit runs only when the prompt is talking about them: a 9- or 10-digit
# run is also a timestamp, a run id, a port range. Keyword-gated keeps the
# false-positive rate survivable while still catching a raw extract, whose
# header row carries the word.
if [ "$hit" -eq 0 ] && printf '%s' "$flat" | grep -Eqi 'ssn|social security|phone|mobile|tel[:e]'; then
  printf '%s' "$flat" | grep -Eq '[^0-9][0-9]{9,10}[^0-9]' && hit=1
fi

[ "$hit" -eq 0 ] && exit 0
```

then the existing block message on stderr and `exit 2`, with one line added:
`rexymcp anonymize` needs the same `[privacy]` engine config.

The three card patterns are AmEx (4-6-5), Diners (4-6-4) and everything else
(4-4-4-2..7, i.e. **14 to 19 digits**). The 14-digit floor is the domain's, not
a guess: Diners is the shortest card, and it is what stops a 13-digit epoch-ms
timestamp from reading as one. **Known ceiling:** a 12-13 digit Maestro card is
not matched — traded for not blocking every prompt that quotes a timestamp.

**Must NOT block:** a prompt with no PII; any prompt when `rexymcp.toml` is
absent or `[privacy] enabled` is not `true`; `ts 1789643833069 ok`;
`listen on 8000 and 18790`; `commit f71e2c6 and 5195efe`.
**Must block:** `jane@acme.com`; `123-45-6789`; `4111 1111 1111 1111`;
AmEx `3782 822463 10005`; Diners `3056 930902 5904`; an email split across a
newline; `ssn 123456789`; the same email under a renamed payload key.

The architect ran this exact body against all of those on 2026-09-18; every one
behaved as stated.


### 2. Install it: `plugin/hooks/hooks.json`

Plugins wire hooks through `hooks/hooks.json` at the plugin root, resolved with
`${CLAUDE_PLUGIN_ROOT}` (verified 2026-09-18 against the installed
`security-guidance` and `i-have-adhd` plugins):

```json
{
  "description": "rexyMCP PII guard — blocks a typed prompt carrying structured PII when [privacy] is enabled",
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "bash \"${CLAUDE_PLUGIN_ROOT}/hooks/pii-guard.sh\"",
            "timeout": 10
          }
        ]
      }
    ]
  }
}
```

### 3. `docs/privacy.md`, the `## The UserPromptSubmit hook` section

Rewrite the enablement half (the honest-constraint paragraph above it stays):
the plugin now installs the hook; it is inert until `[privacy] enabled = true`;
it needs no `jq`; it scans the raw payload so no payload-key change disables it;
bare-digit SSN and phone matching is keyword-gated. Drop the
`.claude/settings.json` copy-in block and the `Requires jq.` line.

## Acceptance criteria

- [ ] Every **Must block** payload in Spec §1 exits 2; every **Must NOT block**
      one exits 0. Quote the table in the Update Log.
- [ ] The script never calls `jq`: `grep -c jq plugin/hooks/pii-guard.sh` is 0.
- [ ] `plugin/hooks/hooks.json` exists and is valid JSON (`jq . ` parses it) with
      the `UserPromptSubmit` entry above.
- [ ] `docs/privacy.md` no longer tells the reader to copy the script into
      `.claude/hooks/`, and no longer says `Requires jq`.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings` and
      `cargo test` all pass — unchanged counts: 716 / 2 / 1206 (10 ignored).
      This phase touches no Rust.

## Test plan

There is no bash test harness in this repo, and adding one is out of scope. The
verification is the payload table below, run against the real script. Write the
loop so a future run can repeat it, and paste its output.

```bash
T=$(mktemp -d); mkdir -p "$T/on" "$T/off"
printf '[privacy]\nenabled = true\n' > "$T/on/rexymcp.toml"
printf '[privacy]\nenabled = false\n' > "$T/off/rexymcp.toml"
G=plugin/hooks/pii-guard.sh
run() { printf '%s' "$2" | CLAUDE_PROJECT_DIR="$1" bash "$G" >/dev/null 2>&1; echo $?; }

echo "email             $(run "$T/on" '{"prompt":"mail jane@acme.com"}')      want 2"
echo "ssn punctuated    $(run "$T/on" '{"prompt":"123-45-6789"}')             want 2"
echo "visa              $(run "$T/on" '{"prompt":"4111 1111 1111 1111"}')     want 2"
echo "amex              $(run "$T/on" '{"prompt":"3782 822463 10005"}')       want 2"
echo "diners            $(run "$T/on" '{"prompt":"3056 930902 5904"}')        want 2"
echo "split email       $(run "$T/on" '{"prompt":"mail jane@\nacme.com"}')    want 2"
echo "bare ssn+keyword  $(run "$T/on" '{"prompt":"ssn 123456789 on file"}')   want 2"
echo "renamed key       $(run "$T/on" '{"user_input":"jane@acme.com"}')       want 2"
echo "clean             $(run "$T/on" '{"prompt":"refactor the parser"}')     want 0"
echo "epoch no keyword  $(run "$T/on" '{"prompt":"ts 1789643833069 ok"}')     want 0"
echo "ports             $(run "$T/on" '{"prompt":"listen on 8000 and 18790"}')  want 0"
echo "shas              $(run "$T/on" '{"prompt":"commit f71e2c6 and 5195efe"}') want 0"
echo "privacy off       $(run "$T/off" '{"prompt":"jane@acme.com"}')          want 0"
echo "no toml           $(run "$T" '{"prompt":"jane@acme.com"}')              want 0"
```

`renamed key` is the fail-closed proof: the same PII under a different key must
still block. `epoch no keyword` is the false-positive guard — if it returns 2,
the bare-digit rule is too loose and the guard will block ordinary development
prompts.

Also run the whole table with `jq` hidden (`PATH=/usr/bin:/bin` is not enough —
use a `PATH` that excludes jq's directory, or a shim directory first on `PATH`
containing nothing). Results must be identical. Paste both tables.

## Authorizations

- [x] May rewrite `plugin/hooks/pii-guard.sh` and create
      `plugin/hooks/hooks.json`.
- [x] May edit the enablement half of `docs/privacy.md`'s hook section.
- [ ] May change the block message's advice to something other than
      `rexymcp anonymize` — **no.**
- [ ] May add a Rust test harness for bash — **no.**
- [ ] May edit any Rust file — **no.** If one needs changing, stop and file a
      blocker.

## Out of scope

- **Names and addresses.** They need the model; the CLI is the path. The hook's
  scope stays structured PII.
- **Rewriting the prompt.** The hook contract does not allow it.
- **Other hook events.** `UserPromptSubmit` only.
- **A `[privacy] guard` config key** to tune patterns. Inert-unless-enabled is
  the only gate this phase adds.

## Update Log

(Filled in by the executor. See WORKFLOW.md § "Update Log entries".)

<!-- entries appended below this line -->

### Update — 2026-09-18 13:28 (progress)

Started by the local executor (rexyMCP dispatch, F05 phase-04). Plan: rewrite `plugin/hooks/pii-guard.sh` to scan the raw payload with no `jq` dependency, add `plugin/hooks/hooks.json` so the plugin installs the hook, and update the enablement half of `docs/privacy.md`'s hook section. Then run the full payload table (with and without `jq` on PATH) and the four Rust gate commands.

### Update — 2026-09-18 14:42 (progress)

Rewrote the guard to scan the raw stdin payload (no key, no `jq`, newlines + JSON `\n` escapes stripped before the grep), added the `[privacy] enabled = true` gate, split the card pattern into AmEx/Diners/standard, and keyword-gated bare digit runs. Added `plugin/hooks/hooks.json` (the `UserPromptSubmit` install) and rewrote the enablement half of `docs/privacy.md`'s hook section. One deviation from the spec sketch, made during verification: the flattening also strips the literal two-char JSON escape `\n` (`tr -d '\n\r' | sed 's/\\n//g'`), because a newline *inside* a JSON string arrives as backslash-n, and a raw `tr -d '\n'` alone left `mail jane@\nacme.com` un-matched. Without it the "split email" row exits 0 instead of 2. The rest of the body is the spec's verbatim (the `&& hit=1` idiom included — it works correctly under `set -uo pipefail`).

### Update — 2026-09-18 14:55 (end-to-end verification)

Ran the real `plugin/hooks/pii-guard.sh` against the payload table twice — full
PATH, and with `jq` hidden behind a shim PATH (`jq resolves under shim? NO-JQ`).
Both runs: every **Must block** row exits 2, every **Must NOT block** row exits
0. The `renamed key` row is the fail-closed proof (same PII, different key, still
blocks); the `epoch no keyword` row is the false-positive guard (13-digit
timestamp, no keyword, passes).

Full PATH:

```
email             2      want 2
ssn punctuated    2             want 2
visa              2     want 2
amex              2       want 2
diners            2        want 2
split email       2 want 2
bare ssn+keyword  2   want 2
renamed key       2       want 2
clean             0     want 0
epoch no keyword  0     want 0
ports             0  want 0
shas              0 want 0
privacy off       0          want 0
no toml           0              want 0
```

jq hidden (shim PATH first, `jq resolves under shim? NO-JQ`):

```
email             2      want 2
ssn punctuated    2             want 2
visa              2     want 2
amex              2       want 2
diners            2        want 2
split email       2 want 2
bare ssn+keyword  2   want 2
renamed key       2       want 2
clean             0     want 0
epoch no keyword  0     want 0
ports             0  want 0
shas              0 want 0
privacy off       0          want 0
no toml           0              want 0
```

Note on the `split email` row: the spec's table feeds it via `printf
'{"prompt":"mail jane@\nacme.com"}'`, where `printf` turns `\n` into a real
newline *before* the JSON exists (the payload is then invalid JSON — two physical
lines). I verified both shapes: the two-physical-lines payload exits 2, and the
well-formed JSON-escape payload (`mail jane@\nacme.com`, backslash-n inside the
string — what a real hook receives) also exits 2, thanks to the extra
`sed 's/\\n//g'` in the flattening. Both forms are quoted above as `split
email = 2`.

Other acceptance checks, run against the real tree:

```
$ grep -c jq plugin/hooks/pii-guard.sh
0
$ jq -e '.hooks.UserPromptSubmit[0].hooks[0].command' plugin/hooks/hooks.json
"bash \"${CLAUDE_PLUGIN_ROOT}/hooks/pii-guard.sh\""
$ grep -n 'Requires jq' docs/privacy.md
(no matches — the line is gone)
$ grep -n 'settings.json' docs/privacy.md
129:nothing to register in `.claude/settings.json`.   (negation only; no copy-in instructions remain)
```

### Update — ts=1789740574249 (complete, server-authored)

**Summary:** All work is complete and committed. Final verification summary:

**Built:**
1. `plugin/hooks/pii-guard.sh` — rewritten to scan the raw stdin payload (no `.user_input` key, no `jq`), gated on `[privacy] enabled = true`, with AmEx/Diners/standard card patterns and keyword-gated bare digit runs. Strips real newlines, carriage returns, and the JSON `\n` escape so PII split across a line break still matches.
2. `plugin/hooks/hooks.json` — the `UserPromptSubmit` install via `${CLAUDE_PLUGIN_ROOT}`.
3. `docs/privacy.md` — hook section rewritten: plugin installs the hook, no `jq`, no copy-in instructions, inert-unless-enabled.

**Deviation from spec:** the flattening uses `tr -d '\n\r' | sed 's/\\n//g'` (adds the `sed` strip of the two-char JSON escape `\n`). Without it, a newline *inside* a JSON string — what a real hook receives — left `mail jane@\nacme.com` un-matched (the "split email" row would exit 0 instead of 2).

**E2E verification:** the full 14-row payload table ran against the real script twice — full PATH and with `jq` hidden behind a shim PATH (`jq resolves under shim? NO-JQ`). All rows match in both runs. The `renamed key` row (fail-closed proof) and `epoch no keyword` row (false-positive guard) both behave correctly.

**Gates:** `cargo fmt --all --check` clean; `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test` all pass (716 / 2 / 1206, 10 ignored — unchanged counts). `git status` clean; commit `1f3662c`.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.49s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.39s


TEST
ests::rejects_max_results_zero ... ok
test tools::symbols::tests::rejects_nonexistent_path ... ok
test tools::symbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1206 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.21s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.23s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F05-privacy-security-hardening/README.md` — +1 -1
- `docs/dev/milestones/F05-privacy-security-hardening/phase-04-prompt-guard.md` — +78 -1
- `docs/privacy.md` — +13 -17
- `plugin/hooks/hooks.json` — +16 -0
- `plugin/hooks/pii-guard.sh` — +47 -23

**Commit:** 1f3662c9bc5f1ef9e601e7818824f51345c6ae23

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).
