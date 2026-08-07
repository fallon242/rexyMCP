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
# Enable (opt-in) by copying this script into your project and adding to
# .claude/settings.json:
#   { "hooks": { "UserPromptSubmit": [ { "type": "command",
#       "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/pii-guard.sh",
#       "timeout": 30 } ] } }
#
# Requires: jq.
set -euo pipefail

input="$(cat)"
prompt="$(printf '%s' "$input" | jq -r '.user_input // empty')"
[ -z "$prompt" ] && exit 0

if printf '%s' "$prompt" | grep -Eq \
  -e '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' \
  -e '[0-9]{3}-[0-9]{2}-[0-9]{4}' \
  -e '(\+?1[-. ])?(\([0-9]{3}\)|[0-9]{3})[-. ][0-9]{3}[-. ][0-9]{4}' \
  -e '[0-9]{4}([ -]?[0-9]{4}){3}'; then
  {
    echo "rexyMCP PII guard: your prompt appears to contain structured PII"
    echo "(email / SSN / phone / card). Anonymize it first, then paste the"
    echo "tokenized text:"
    echo "    printf '%s' \"<your text>\" | rexymcp anonymize"
    echo "Reverse anything you need to read with 'rexymcp reconstitute'."
    echo "(See docs/privacy.md. Names/addresses need the model — use the CLI.)"
  } >&2
  exit 2
fi
exit 0
