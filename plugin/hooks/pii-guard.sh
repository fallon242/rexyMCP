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

{
  echo "rexyMCP PII guard: your prompt appears to contain structured PII"
  echo "(email / SSN / phone / card). Anonymize it first, then paste the"
  echo "tokenized text:"
  echo "    printf '%s' \"<your text>\" | rexymcp anonymize"
  echo "Reverse anything you need to read with 'rexymcp reconstitute'."
  echo "rexymcp anonymize needs the same [privacy] engine config."
  echo "(See docs/privacy.md. Names/addresses need the model — use the CLI.)"
} >&2
exit 2
