#!/usr/bin/env bash
# rexyMCP PII guard — an opt-in Claude Code UserPromptSubmit safety net.
#
# Claude Code's UserPromptSubmit hook CANNOT rewrite a prompt (confirmed against
# the hook contract) — it can only allow, add context, or BLOCK. So this does the
# only thing that actually prevents a leak: it BLOCKS a prompt that contains
# obvious structured PII and tells you to anonymize it first with
# `rexymcp anonymize`, then paste the tokenized text.
#
# Scope, honestly: this is a catch-your-mistake net for STRUCTURED PII
# (email / US SSN / phone / card), matched by fast local regex — no model, no
# network, so it never fails open. It does NOT catch names or addresses (those
# need the NER model); for full anonymization use `rexymcp anonymize`. See
# docs/privacy.md.
#
# Installed by the plugin (see plugin/hooks/hooks.json); inert unless the
# project's rexymcp.toml sets [privacy] enabled = true.
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

# Scan the RAW payload, newlines removed. No key to rename, no JSON tool to be
# missing, and PII split across a line break still matches. A real newline
# (0x0a), a carriage return, and the JSON escape sequence \n (a real newline
# inside a string is encoded as backslash-n) are all removed before the scan —
# so an email, card, or number split across a line break is re-joined and still
# matches.
flat="$(printf '%s' "$payload" | tr -d '\n\r' | sed 's/\\n//g')"

# Claude Code puts UUIDs in every payload (session_id, a fresh prompt_id per
# prompt, paths built from them). Their all-digit hex groups can read as a card
# or phone number, so a prompt could be blocked at random. No PII has the
# 8-4-4-4-12 hex shape: remove UUIDs before any check runs.
flat="$(printf '%s' "$flat" | sed -E 's/[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}/ /g')"

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

[ "$hit" -eq 0 ] && exit 0

{
  echo "rexyMCP PII guard: your prompt appears to contain structured PII"
  echo "(matched: $why)."
  echo "(email / SSN / phone / card). Anonymize it first, then paste the"
  echo "tokenized text:"
  echo "    printf '%s' \"<your text>\" | rexymcp anonymize"
  echo "Reverse anything you need to read with 'rexymcp reconstitute'."
  echo "rexymcp anonymize needs the same [privacy] engine config."
  echo "(See docs/privacy.md. Names/addresses need the model — use the CLI.)"
} >&2
exit 2
