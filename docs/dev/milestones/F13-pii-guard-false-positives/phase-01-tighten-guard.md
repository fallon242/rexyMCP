# Phase 1: tighten guard

**Milestone:** F13 — PII guard false positives
**Status:** done
**Depends on:** none
**Estimated diff:** ~80 lines
**Tags:** language=bash, kind=bugfix, size=s

## Goal

`plugin/hooks/pii-guard.sh` blocks harmless prompts. Replace its detection
block with the version below, which the architect has already run against the
full payload table in the Test plan (all 21 rows `ok`). Update the one
paragraph of `docs/privacy.md` that describes the rules.

## Pre-flight

1. Run the Test plan loop against the **current** script and paste its output
   in a `(progress)` entry. Expected on the current script (measured by the
   architect 2026-09-18): `two epochs`, `telemetry+run id`, `git remote scp`
   and `git remote ssh` show `FAIL` (got 2, want 0), and `tel word+digits`
   shows `FAIL` (got 0, want 2 — the old keyword rule needs `tel:` or `tele`),
   and `all-digit uuid` shows `FAIL` (got 2, want 0); every other row is `ok`.

## Spec

### 0. `plugin/hooks/pii-guard.sh` — remove UUIDs before scanning

Claude Code sends a fresh random `prompt_id` UUID with every prompt (plus
`session_id`, `agent_id` and paths built from them). When a UUID's hex groups
happen to be all digits, the card rule fires, so about 1 prompt in 125 is
blocked at random — "commit" was blocked this way. Replace:

```bash
flat="$(printf '%s' "$payload" | tr -d '\n\r' | sed 's/\\n//g')"
```

with:

```bash
flat="$(printf '%s' "$payload" | tr -d '\n\r' | sed 's/\\n//g')"

# Claude Code puts UUIDs in every payload (session_id, a fresh prompt_id per
# prompt, paths built from them). Their all-digit hex groups can read as a card
# or phone number, so a prompt could be blocked at random. No PII has the
# 8-4-4-4-12 hex shape: remove UUIDs before any check runs.
flat="$(printf '%s' "$flat" | sed -E 's/[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}/ /g')"
```

### 1. `plugin/hooks/pii-guard.sh` — replace the detection block

Replace exactly this block (from `hit=0` up to, not including,
`[ "$hit" -eq 0 ] && exit 0`):

```bash
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

```

with exactly this:

```bash
hit=0
why=""

# Email. An scp-style git remote (git@host:owner/repo, ssh://git@host/...) is
# not an address; drop it from the copy this check scans.
printf '%s' "$flat" | sed -E 's#git@[A-Za-z0-9.-]+[:/]# #g' \
  | grep -Eq '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' && { hit=1; why="email address"; }

# US SSN, punctuated.
[ "$hit" -eq 0 ] && printf '%s' "$flat" | grep -Eq '[0-9]{3}-[0-9]{2}-[0-9]{4}' \
  && { hit=1; why="SSN-shaped number"; }

# Phone, punctuated.
[ "$hit" -eq 0 ] && printf '%s' "$flat" \
  | grep -Eq '(\+?1[-. ])?(\([0-9]{3}\)|[0-9]{3})[-. ][0-9]{3}[-. ][0-9]{4}' \
  && { hit=1; why="phone-shaped number"; }

# Card. A digit run made of groups joined by single spaces or dashes; any run of
# whole consecutive groups totalling 14-19 digits that passes the Luhn check is
# a card. Luhn rejects 9 in 10 random digit strings, and 13-digit runs (epoch
# milliseconds) are below the floor, so timestamps and ids no longer block.
if [ "$hit" -eq 0 ] && printf '%s' "$flat" | awk '
  function luhn(d,   i, c, s, dbl) {
    s = 0; dbl = 0
    for (i = length(d); i >= 1; i--) {
      c = substr(d, i, 1) + 0
      if (dbl) { c *= 2; if (c > 9) c -= 9 }
      s += c; dbl = !dbl
    }
    return s % 10 == 0
  }
  {
    rest = $0
    while (match(rest, /[0-9]+([ -][0-9]+)*/)) {
      run = substr(rest, RSTART, RLENGTH)
      rest = substr(rest, RSTART + RLENGTH)
      n = split(run, g, /[ -]/)
      for (i = 1; i <= n; i++) {
        d = ""
        for (j = i; j <= n; j++) {
          d = d g[j]
          if (length(d) > 19) break
          if (length(d) >= 14 && luhn(d)) { found = 1; exit }
        }
      }
    }
  }
  END { exit found ? 0 : 1 }'; then
  hit=1; why="card number (Luhn-valid)"
fi

# Bare digit runs only when the prompt is talking about them: a 9- or 10-digit
# run is also a timestamp, a run id, a port range. Keyword-gated keeps the
# false-positive rate survivable while still catching a raw extract, whose
# header row carries the word. `tel` is matched as a whole word (or `tel:`) so
# "telemetry" does not open the gate.
if [ "$hit" -eq 0 ] && printf '%s' "$flat" | grep -Eqi 'ssn|social security|phone|mobile|\btel\b|tel:'; then
  printf '%s' "$flat" | grep -Eq '[^0-9][0-9]{9,10}[^0-9]' && { hit=1; why="9-10 digit number next to an SSN/phone keyword"; }
fi

```

### 2. Same file — name the rule in the block message

Replace:

```bash
  echo "rexyMCP PII guard: your prompt appears to contain structured PII"
```

with:

```bash
  echo "rexyMCP PII guard: your prompt appears to contain structured PII"
  echo "(matched: $why)."
```

### 3. `docs/privacy.md` — describe the rules

Replace:

```
a timestamp or a port range in ordinary development prompts.
```

with:

```
a timestamp or a port range in ordinary development prompts. A card number
must total 14–19 digits across whole space- or dash-separated groups and pass
the Luhn check, so timestamps and ids rarely block; a git remote
(`git@host:owner/repo`) is not treated as an email address. The block message
names the rule that matched.
```

**Must NOT:**

- Scan only the `prompt` key, or add `jq`/`python3`. The guard reads the raw
  payload so a renamed key still blocks (the `renamed key` row).
- Echo the matched text in the block message — name the rule only.
- Change the `[privacy] enabled` gate, `hooks.json`, or the exit codes (0 allow,
  2 block).

## Acceptance criteria

- [ ] Every row of the Test plan table prints `ok`.
- [ ] `privacy off` and `no toml` print `0`.
- [ ] A blocked prompt's message includes a `(matched: …)` line.
- [ ] `bash -n plugin/hooks/pii-guard.sh` passes.
- [ ] `cargo fmt --all --check`, `cargo build`,
      `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`
      pass with counts unchanged: **729 / 2 / 1218**.

## Test plan

There is no bash test harness; the table is the test. Run from the repo root
and paste the output:

```bash
T=$(mktemp -d); mkdir -p "$T/on" "$T/off"
printf '[privacy]\nenabled = true\n' > "$T/on/rexymcp.toml"
printf '[privacy]\nenabled = false\n' > "$T/off/rexymcp.toml"
G=plugin/hooks/pii-guard.sh
run() { printf '%s' "$2" | CLAUDE_PROJECT_DIR="$1" bash "$G" >/dev/null 2>&1; echo $?; }
while IFS='|' read -r name want json; do
  got=$(run "$T/on" "$json"); [ "$got" = "$want" ] && ok=ok || ok=FAIL
  printf '%-4s %-22s got %s want %s\n' "$ok" "$name" "$got" "$want"
done <<'TABLE'
email|2|{"prompt":"mail jane@acme.com"}
ssn punctuated|2|{"prompt":"123-45-6789"}
visa|2|{"prompt":"4111 1111 1111 1111"}
visa unspaced|2|{"prompt":"card 4111111111111111 ok"}
amex|2|{"prompt":"3782 822463 10005"}
diners|2|{"prompt":"3056 930902 5904"}
visa then year|2|{"prompt":"4111 1111 1111 1111 2025"}
split email|2|{"prompt":"mail jane@\nacme.com"}
bare ssn+keyword|2|{"prompt":"ssn 123456789 on file"}
tel word+digits|2|{"prompt":"tel 5551234567 x"}
renamed key|2|{"user_input":"jane@acme.com"}
clean|0|{"prompt":"refactor the parser"}
epoch no keyword|0|{"prompt":"ts 1789643833069 ok"}
ports|0|{"prompt":"listen on 8000 and 18790"}
shas|0|{"prompt":"commit f71e2c6 and 5195efe"}
two epochs|0|{"prompt":"timestamps 1789743238937 1789743238938"}
telemetry+run id|0|{"prompt":"telemetry for run 1789743238"}
git remote scp|0|{"prompt":"clone git@github.com:fallon242/rexyMCP.git"}
git remote ssh|0|{"prompt":"ssh://git@github.com/fallon242/rexyMCP"}
empty prompt|0|{"prompt":""}
all-digit uuid|0|{"prompt_id":"12345678-1234-4567-8901-234567890123","prompt":"commit"}
TABLE
echo "privacy off  $(run "$T/off" '{"prompt":"jane@acme.com"}') want 0"
echo "no toml      $(run "$T" '{"prompt":"jane@acme.com"}') want 0"
printf '%s' '{"prompt":"3782 822463 10005"}' | CLAUDE_PROJECT_DIR="$T/on" bash "$G" 2>&1 | head -2
```

**Finish condition:** 21 table rows `ok`, both trailing lines `want 0` with
`0`, and the last command prints `(matched: card number (Luhn-valid)).`

## End-to-end verification

Paste the full output of the Test plan block, run after the change, in a
`### Update — YYYY-MM-DD HH:MM (end-to-end verification)` entry.

## Authorizations

- Editing `plugin/hooks/pii-guard.sh` and one paragraph of `docs/privacy.md`
  as specified.

## Out of scope

- The phone rule, the keyword list other than `tel`, and `hooks.json`.

## Update Log

<!-- entries appended below this line -->

### Update — 2026-09-18 22:04 (progress)

Pre-flight: ran the Test plan table against the current `plugin/hooks/pii-guard.sh` before changing anything.

```
ok   email                  got 2 want 2
ok   ssn punctuated         got 2 want 2
ok   visa                   got 2 want 2
ok   visa unspaced          got 2 want 2
ok   amex                   got 2 want 2
ok   diners                 got 2 want 2
ok   visa then year         got 2 want 2
ok   split email            got 2 want 2
ok   bare ssn+keyword       got 2 want 2
FAIL tel word+digits        got 0 want 2
ok   renamed key            got 2 want 2
ok   clean                  got 0 want 0
ok   epoch no keyword       got 0 want 0
ok   ports                  got 0 want 0
ok   shas                   got 0 want 0
FAIL two epochs             got 2 want 0
FAIL telemetry+run id      got 2 want 0
FAIL git remote scp         got 2 want 0
FAIL git remote ssh         got 2 want 0
ok   empty prompt           got 0 want 0
FAIL all-digit uuid        got 2 want 0
privacy off  0 want 0
no toml      0 want 0
rexyMCP PII guard: your prompt appears to contain structured PII
(email / SSN / phone / card). Anonymize it first, then paste the
```

Matches the architect's expected baseline exactly: `tel word+digits` fails (old keyword rule needs `tel:` or `tele`), and `two epochs`, `telemetry+run id`, `git remote scp`, `git remote ssh`, `all-digit uuid` all show the false positives the new detection block and UUID stripping are meant to fix. Now implementing Spec tasks 0–3.

### Update — 2026-09-18 22:04 (end-to-end verification)

Test plan table, run after the change:

```
ok   email                  got 2 want 2
ok   ssn punctuated         got 2 want 2
ok   visa                   got 2 want 2
ok   visa unspaced          got 2 want 2
ok   amex                   got 2 want 2
ok   diners                 got 2 want 2
ok   visa then year         got 2 want 2
ok   split email            got 2 want 2
ok   bare ssn+keyword       got 2 want 2
ok   tel word+digits        got 2 want 2
ok   renamed key            got 2 want 2
ok   clean                  got 0 want 0
ok   epoch no keyword       got 0 want 0
ok   ports                  got 0 want 0
ok   shas                   got 0 want 0
ok   two epochs             got 0 want 0
ok   telemetry+run id       got 0 want 0
ok   git remote scp         got 0 want 0
ok   git remote ssh         got 0 want 0
ok   empty prompt           got 0 want 0
ok   all-digit uuid         got 0 want 0
privacy off  0 want 0
no toml      0 want 0
rexyMCP PII guard: your prompt appears to contain structured PII
(matched: card number (Luhn-valid)).
```

All 21 table rows `ok`; both trailing lines show `0` (want 0); last command prints `(matched: card number (Luhn-valid)).`.

### Update — 2026-09-18 22:20 (complete, server-authored)

**Summary:** Summary + Notes for review

Implemented all four Spec tasks on `plugin/hooks/pii-guard.sh` and `docs/privacy.md`:
- **Task 0** — UUIDs are stripped from the flattened payload before any check runs (session_id, prompt_id, path-derived UUIDs can have all-digit hex groups that read as cards/phones).
- **Task 1** — replaced the single combined `grep` block with four per-rule checks (email with `git@host:...`/`ssh://git@host/...` stripped, SSN, phone, card) plus the keyword-gated 9–10-digit gate, each recording `why`. The card rule uses an awk Luhn pass over consecutive space/dash-joined groups totalling 14–19 digits. The keyword list now uses `\btel\b|tel:` instead of `tel[:e]` so "telemetry" no longer opens the gate.
- **Task 2** — the block message prints `(matched: $why).` on a second line, naming the rule without echoing matched text.
- **Task 3** — the one-paragraph change in `docs/privacy.md` describing the Luhn/14–19-digit card rule, the git-remote exemption, and the named-rule message.

End-to-end: the Test plan table run against the committed script prints `ok` for all 21 rows, `privacy off 0`, `no toml 0`, and `(matched: card number (Luhn-valid)).` for the Amex sample. `bash -n` passes; `cargo fmt --all --check`, `cargo build`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all pass with counts `729 / 2 / 1218` unchanged. No deviations from the spec. Committed as `59e880d` — 4 files changed, working tree clean.

**Executor:** RedHatAI/Qwen3.8-27B-INT4

**Gates:** format=run, build=run, lint=run, test=run

**Command output tails:**

```
FORMAT


BUILD
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.49s


LINT
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.33s


TEST
jects_nonexistent_path ... ok
test tools::symbols::tests::rejects_path_outside_root ... ok
test tools::symbols::tests::references_no_matches_advisory ... ok
test tools::symbols::tests::single_file_unsupported_extension_advisory_error ... ok
test tools::symbols::tests::type_mismatch_returns_recovery_hint ... ok
test tools::symbols::tests::metadata_carries_definitions_and_files_count ... ok
test tools::update_task::tests::flips_active_task_to_done ... ok
test tools::update_task::tests::flips_pending_task_to_active ... ok
test tools::symbols::tests::references_respects_max_results ... ok
test tools::symbols::tests::references_single_file_path ... ok
test tools::update_task::tests::invalid_args_hint_lists_incomplete_ids ... ok
test tools::update_task::tests::invalid_args_hint_reports_all_complete ... ok
test tools::update_task::tests::invalid_state_returns_advisory_error ... ok
test tools::update_task::tests::malformed_args_returns_advisory_error ... ok
test tools::update_task::tests::metadata_shape_is_unchanged ... ok
test tools::update_task::tests::null_args_returns_recovery_hint ... ok
test tools::update_task::tests::result_flags_redundant_remark ... ok
test tools::update_task::tests::result_lists_remaining_incomplete_ids ... ok
test tools::update_task::tests::result_reports_all_complete_when_last_done ... ok
test tools::update_task::tests::success_output_names_task ... ok
test tools::update_task::tests::unknown_id_returns_advisory_error ... ok
test tools::write_file::tests::append_creates_file_if_missing ... ok
test tools::write_file::tests::append_false_overwrites ... ok
test tools::write_file::tests::appends_to_existing_file ... ok
test tools::write_file::tests::missing_path_returns_recovery_hint ... ok
test tools::write_file::tests::creates_new_file ... ok
test tools::write_file::tests::non_object_args_do_not_panic ... ok
test tools::write_file::tests::rejects_malformed_args ... ok
test tools::write_file::tests::overwrites_existing_file ... ok
test tools::write_file::tests::reports_missing_parent_dir ... ok
test tools::write_file::tests::scope_escape_returns_advisory_error_and_writes_nothing ... ok
test tools::write_file::tests::success_output_includes_line_count ... ok
test tools::symbols::tests::references_truncation_note_omits_kind_filter ... ok
test tools::symbols::tests::references_snippet_shows_source_line ... ok
test tools::symbols::tests::references_across_multiple_files ... ok
test tools::symbols::tests::unsupported_extension_skipped_in_dir_walk ... ok
test tools::symbols::tests::reports_line_and_column ... ok
test tools::symbols::tests::respects_gitignore ... ok
test tools::symbols::tests::finds_rust_struct_and_trait ... ok
test tools::bash::tests::cargo_command_output_is_filtered_through_cargo_filter ... ok
test store::telemetry::tests::append_is_atomic_under_concurrent_appenders ... ok
test ai::backends::openai::tests::midstream_stall_is_not_retried ... ok
test ai::backends::openai::tests::first_token_stall_retries_then_succeeds ... ok
test ai::tests::stream_next_uses_supplied_timeout ... ok
test tools::bash::tests::arg_timeout_overrides_constructor_default ... ok
test tools::bash::tests::default_timeout_used_when_arg_absent ... ok
test tools::bash::tests::times_out_advisory_failure ... ok
test ai::backends::openai::tests::first_token_stall_exhausts_retries_then_errors ... ok
test health::tests::check_returns_unreachable_on_connection_error ... ok

test result: ok. 1218 passed; 0 failed; 10 ignored; 0 measured; 0 filtered out; finished in 6.20s


running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.14s
     Running unittests src/main.rs (target/debug/deps/rexymcp-3de3446ef9b6a3ce)
     Running tests/readme_config_reference.rs (target/debug/deps/readme_config_reference-22f887757c17cb2b)
     Running unittests src/lib.rs (target/debug/deps/executor-0c52bf72536f9e8e)
   Doc-tests executor

```

**Files changed:**

- `docs/dev/milestones/F13-pii-guard-false-positives/README.md` — +1 -1
- `docs/dev/milestones/F13-pii-guard-false-positives/phase-01-tighten-guard.md` — +71 -1
- `docs/privacy.md` — +5 -1
- `plugin/hooks/pii-guard.sh` — +60 -10

**Commit:** 59e880d084e0ad09c3f906bdd192852943648bc2

**Notes:** server-authored completion entry (executor no longer owns the bookkeeping tail; see M27 phase-03).

### Review verdict — 2026-09-18

- **Verdict:** approved_first_try
- **Bounces:** none
- **Executor:** RedHatAI/Qwen3.8-27B-INT4 (local), 56 turns
- **Scope deviations:** none. The committed hook equals the spec applied to the pre-phase file, byte for byte (`59e880d`).
- **Verification:** gates 729 / 2 / 1218; `bash -n` clean. Architect's own run of the Test plan: 21/21 `ok`, `privacy off` and `no toml` 0, the Amex sample prints `(matched: card number (Luhn-valid)).` Random-UUID payloads with prompt `commit`: 0/500 blocked. Mutation — hook without the UUID strip: 4/1500 random payloads blocked, and `{"prompt_id":"41111111-1111-1111-1111-111111111111","prompt":"commit"}` blocks (2) without the strip and passes (0) with it.
- **Calibration:** the spec's `all-digit uuid` row does **not** exercise the strip — it passes with the strip removed, because its digit groups form no Luhn-valid run. Architect error in the test design; the `41111111-…` payload above is the row that does. Spec test that cannot fail against the bug it targets: **2×** (F06 phase-01, F13 phase-01). Executor labelled its first entry `(progress)` rather than `(started)` — nit.
