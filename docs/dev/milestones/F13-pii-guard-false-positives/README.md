# F13 — PII guard false positives

**Goal:** the prompt guard stops blocking ordinary development text, and still
blocks the structured PII it was built for.

**Status:** open — opened 2026-09-18 on human go-ahead.

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

- [ ] Every case in F05 phase-04's payload table still gives its original result.
- [ ] The five cases above pass; UUIDs in the payload never trigger a block.
- [ ] The block message names the rule that matched.
- [ ] `docs/privacy.md` describes the new card and git-remote rules.

## Phases

| #  | Phase                                                                      | Status |
|----|----------------------------------------------------------------------------|--------|
| 01 | tighten-guard ([phase-01-tighten-guard.md](phase-01-tighten-guard.md))     | review        |

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
