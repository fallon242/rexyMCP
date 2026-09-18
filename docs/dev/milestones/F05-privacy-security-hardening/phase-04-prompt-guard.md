# Phase 4: make the prompt guard fail closed, and ship it

**Milestone:** F05 — Privacy and security hardening
**Status:** todo
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
