# F13 — PII guard false positives

**Goal:** the prompt guard stops blocking ordinary development text, and still
blocks the structured PII it was built for.

**Status:** done — opened and closed 2026-09-18. One phase,
`approved_first_try`.

**Depends on:** none.

**Source:** the user was blocked in two terminals on prompts with no PII.
Reproduced 2026-09-18 against `plugin/hooks/pii-guard.sh`:

| Harmless text | Rule that fired |
|---|---|
| two 13-digit epoch-ms values separated by a space | card — optional separators join them |
| any 14+ digit run (`20260918153012`) | card |
| `telemetry … 1789743238` | keyword gate — `tel[:e]` matches "tele" |
| `git@github.com:owner/repo` | email |
| **any prompt** (e.g. `commit`), about 1 in 125 at random | card — the payload's fresh `prompt_id` UUID has all-digit hex groups |

**Exit criteria:**

- [x] Every case in F05 phase-04's payload table still gives its original result.
- [x] The five cases above pass; UUIDs in the payload never trigger a block.
- [x] The block message names the rule that matched.
- [x] `docs/privacy.md` describes the new card and git-remote rules.

## Phases

| #  | Phase                                                                      | Status |
|----|----------------------------------------------------------------------------|--------|
| 01 | tighten-guard ([phase-01-tighten-guard.md](phase-01-tighten-guard.md))     | done   |

## Notes

- **Routing: local only.** Privacy code.
- **Kept on purpose:** the guard still scans the whole payload, not just the
  `prompt` key (F05's fail-closed decision: a renamed key must still block),
  and still needs no `jq`. UUIDs are stripped from the scanned text first:
  Claude Code 2.1.275 sends `session_id`, `prompt_id` (new per prompt),
  `agent_id` and `session_title` (read from the binary). Simulated: old script
  blocked `commit` in 3/400 payloads; new script 0/1000.
- **Residual false positives, measured:** a 14–19 digit number passes Luhn
  about 1 time in 10 — 31 of 300 random 14-digit datetime stamps still block
  (was 300 of 300). Dotted 3-3-4 numbers (`123.456.7890`) still match the
  phone rule; that shape is a real phone format.

## F13 retrospective

**Closed 2026-09-18 at one phase**, `approved_first_try`: 56 turns on the
local `RedHatAI/Qwen3.8-27B-INT4` (code `59e880d`, approval `c1ba856`). The
committed hook equals the spec byte for byte. Live immediately — the plugin
hook runs from this repo, no rebuild.

**The real cause was not in the prompt.** The first diagnosis (card regex
joining numbers, `tel[:e]`, git remotes) was right but incomplete: a prompt of
just `commit` was blocked mid-drafting. Reading the payload builder in the
Claude Code 2.1.275 binary showed a fresh random `prompt_id` UUID on every
prompt; its all-digit hex groups hit the old card regex about 1 prompt in 125.
Stripping UUIDs before scanning took simulated blocks to 0/1500.

**Measured:** random 14-digit datetime stamps blocked 10% (was 100%); random
UUID payloads 0 blocked (old script 3/400); all F05 phase-04 true positives
still block.

**Architect error — a test that cannot fail.** The spec's `all-digit uuid`
row passes even with the UUID strip removed (its groups form no Luhn-valid
run). Caught by mutation at review; the row that does discriminate is
`{"prompt_id":"41111111-1111-1111-1111-111111111111","prompt":"commit"}`.
Second occurrence after F06 phase-01 — see `NEXT.md` counters.

**Process note.** An attempt to capture the user's real hook payloads to
`/tmp` was denied by the auto-mode classifier as PII handling. The binary and a
simulation answered the question without it; that is the better default.
