# Codex Support Plan

## Objective

Enable Codex to use rexyMCP as a native set of Codex skills with a configured
MCP server, while preserving Claude Code and Google Antigravity compatibility.

The initial integration must be additive. It must not change executor behavior,
the SDLC lifecycle, review rigor, or existing Claude/Antigravity packaging unless
an implementation test proves a narrowly scoped change is necessary.

## Proposed architecture

The Rust MCP server already uses a compatible stdio transport and structured MCP
tools. Codex support should therefore live primarily in a Codex-specific plugin
and skill adapter layer:

```text
Existing Claude/Antigravity package       New Codex package
plugin/skills/*                           plugin/codex-skills/*
plugin/.claude-plugin/plugin.json         plugin/.codex-plugin/plugin.json
plugin/.mcp.json                          Codex MCP manifest/config
CLAUDE.md / .agents/AGENTS.md             root AGENTS.md shim
```

The existing Claude and Antigravity assets remain intact. Codex-specific skills
adapt host-facing metadata, invocation syntax, workspace discovery, and
orchestration vocabulary while preserving the same workflow.

## Current codebase findings

### Already portable

- `rexymcp serve` is an `rmcp` v2 stdio server.
- All ten tools expose JSON-schema inputs and structured results.
- Async dispatch through `execute_phase` followed by `get_run_status` is
  client-neutral.
- The local executor, governor, verifier, telemetry, CLI, and phase-result
  contract do not depend on Claude APIs.
- The MCP server does not call a cloud architect model.
- `REXYMCP.md`, `STANDARDS.md`, `WORKFLOW.md`, phase docs, and milestone state
  are fundamentally agent-neutral.
- The existing plugin templates can be reused by Codex without duplication.

### Claude-specific coupling

The existing coupling is concentrated in the plugin skills and distribution
layer:

- Skill frontmatter uses Claude-specific fields:
  - `model: opus|sonnet`
  - `argument-hint`
  - `allowed-tools`
- Invocation language assumes `/rexymcp:<skill>`, while Codex skills are
  explicitly invoked as `$skill-name`.
- Skills resolve the workspace using `CLAUDE_PROJECT_DIR` or
  `ANTIGRAVITY_PROJECT_DIR`.
- The `auto` skill explicitly uses Claude's `Agent` and `Skill` tools and Claude
  model names from `dispatch_model` and `review_model`.
- Bootstrap writes `CLAUDE.md` and `.agents/AGENTS.md`, while Codex's durable
  repository instruction surface is a root or nested `AGENTS.md`.
- Architect and reviewer attribution says `Claude Code (direct)`.
- Transcript harvesting understands Claude Code JSONL layouts.
- Documentation and installation instructions present Claude Code as the
  principal architect.
- The current plugin manifests are Claude/Antigravity manifests, not a Codex
  `.codex-plugin/plugin.json`.

### MCP integration risks to verify

1. `plugin/.mcp.json` starts the server with:

   ```json
   {
     "command": "rexymcp",
     "args": ["serve", "--config", "./rexymcp.toml"]
   }
   ```

   This works only if Codex launches the bundled stdio server with the target
   repository as its working directory. An installed-plugin smoke test must
   prove this behavior.

2. Server root corroboration currently initializes the MCP roots list as empty
   and checks only `CLAUDE_PROJECT_DIR` and `ANTIGRAVITY_PROJECT_DIR`. Under
   Codex, neither may exist. Calls currently pass when no corroboration source
   exists, so functionality should work, but the security check is weaker and
   the tool description overstates roots support.

3. The server's `get_info()` advertises tools but no server-wide
   `instructions`. Codex can consume MCP initialization instructions, but adding
   them is an optional enhancement rather than a prerequisite.

4. `get_run_status` uses a bounded approximately 15-second poll. This fits
   within Codex's normal MCP tool timeout, but must be confirmed in the smoke
   test.

## Implementation phases

### Phase 1 — Add a Codex manifest without touching existing manifests

Add:

- `plugin/.codex-plugin/plugin.json`
- A Codex-specific MCP companion file only if the existing `.mcp.json` cannot
  be safely shared.

The Codex manifest must:

- Retain the `rexymcp` plugin identity.
- Point to Codex-adapted skills rather than the Claude skill directory.
- Register the existing `rexymcp serve` stdio command.
- Reuse current author, version, license, and description metadata.
- Avoid apps, hooks, UI, and unrelated capabilities.

Preserve:

- `plugin/.claude-plugin/plugin.json`
- `plugin/plugin.json`
- `plugin/.mcp.json`
- `.claude-plugin/marketplace.json`

#### Acceptance criteria

- The Codex plugin validator passes.
- Existing Claude manifests are byte-for-byte unchanged.
- Claude's five skills remain discoverable under their current names.

### Phase 2 — Add five thin Codex-native skill adapters

Create distinct Codex skills:

- `rexymcp-architect`
- `rexymcp-dispatch`
- `rexymcp-review`
- `rexymcp-escalate`
- `rexymcp-auto`

Prefixed names prevent collisions with generic skills such as `review` or
`architect`.

Each skill must have Codex-native frontmatter containing only `name` and
`description`, plus `agents/openai.yaml` metadata. Adapt only host-facing
instructions:

| Claude assumption | Codex adapter |
|---|---|
| `/rexymcp:architect` | `$rexymcp-architect` |
| `CLAUDE_PROJECT_DIR` | Current workspace or repository root |
| `Read`, `Glob`, `Grep`, `Bash` tool names | Capability-oriented wording |
| Claude interactive prompt | Normal Codex user-input behavior |
| `Claude Code (direct)` | `Codex (direct)` or neutral `Architect (direct)` |
| Claude `Agent` tool | Codex subagent/delegation capability |
| Claude transcript harvest | Skip with explicit "architect usage unavailable" |
| Claude plugin-dir environment variables | Resolve resources relative to the loaded skill path or plugin root |

The adapters must preserve:

- The phase lifecycle.
- The phase contract.
- Review rigor.
- Escalation levers.
- Assist limits.
- Milestone human gates.

To limit drift, the adapters must reuse:

- `plugin/templates/STANDARDS.md`
- `plugin/templates/WORKFLOW.md`
- The target repository's `REXYMCP.md`

They must not fork those documents.

#### Acceptance criteria

- All five skills pass Codex skill validation.
- Each skill can be explicitly selected.
- Dispatch calls the bundled MCP tools rather than shelling out to
  `run-phase`.
- Claude skill files remain unchanged.

### Phase 3 — Add Codex bootstrap behavior

Extend only the Codex architect adapter to support a root `AGENTS.md` shim:

```markdown
# rexyMCP workflow

Before working in this repository, read `REXYMCP.md` and follow its workflow,
commands, standards, and milestone state.
```

Rules:

- Create the shim only when root `AGENTS.md` is absent.
- If root `AGENTS.md` exists, append a minimal `REXYMCP.md` reference only when
  one is missing.
- Never overwrite existing user guidance.
- Do not place the executor contract in `AGENTS.md`.
- Continue generating `CLAUDE.md` and `.agents/AGENTS.md` when supporting those
  hosts.
- Treat `REXYMCP.md` as the single shared contract.

#### Acceptance criteria

- A newly bootstrapped Codex project automatically receives the workflow
  orientation.
- Existing root `AGENTS.md` content remains unchanged except for the minimal
  additive reference.
- No second executor contract is created.

### Phase 4 — Adapt autonomous orchestration conservatively

For `$rexymcp-auto`:

- Compose the other four Codex skills exactly as the current auto skill
  composes the Claude skills.
- Use Codex subagents only when available.
- Keep draft and escalation judgment in the main session.
- Delegate dispatch and review as bounded tasks.
- Preserve all five stop conditions.
- Do not assume Claude model identifiers are valid Codex model identifiers.

For the initial Codex release:

- If `dispatch_model` or `review_model` contains a Claude-specific identifier,
  inherit the active Codex model and report that the role-model override was
  not applicable.
- Do not alter `rexymcp.toml` or introduce parallel Codex model configuration.
- Consider architect-model configuration generalization only as a separately
  approved future change.

#### Acceptance criteria

- Interactive and autonomous executions use the same underlying procedures.
- The loop never crosses a milestone boundary.
- Review rigor and assist limits remain unchanged.
- Reports identify the actual delegation behavior and never claim an
  unsupported model switch.

### Phase 5 — Validate MCP startup and repository scoping

Run an installed-plugin smoke test from a temporary target repository:

1. Install the local Codex plugin through a repo-local test marketplace.
2. Start a fresh Codex session in the target repository.
3. Confirm the five skills appear.
4. Confirm all ten MCP tools appear.
5. Call `executor_health`.
6. Verify the server loads the target repository's `rexymcp.toml`.
7. Dispatch a hermetic mock or deliberately bounded test phase.
8. Confirm:
   - `{run_id}` is returned.
   - `get_run_status` reaps the result.
   - Structured result fields survive intact.
   - Stop and resume remain callable.
   - The server process remains alive.

Only if this proves that Codex launches the plugin server from the wrong
directory may the MCP startup configuration change. Preferred fixes, in order:

1. Codex manifest `cwd` support, if available and project-relative.
2. A small launcher that resolves the active repository before running the
   unchanged binary.
3. A narrowly scoped server/config-path enhancement as the last resort.

#### Acceptance criteria

- The plugin works from a target repository rather than only from the rexyMCP
  checkout.
- The correct target `rexymcp.toml` is loaded.
- All ten MCP tools remain available throughout the session.
- No Claude or Antigravity startup behavior regresses.

### Phase 6 — Harden repository corroboration only if required

Test whether Codex provides MCP roots or a documented project-directory
environment variable.

- If Codex roots can be read through `rmcp`, wire the existing
  `roots::corroborate` function to the real roots list.
- If Codex provides a documented project-dir variable, recognize it alongside
  the Claude and Antigravity variables.
- If neither exists, document the limitation and consider canonical
  current-working-directory corroboration.

This is the only likely Rust change. It must remain a separate phase because:

- It changes a security boundary.
- It requires dedicated tests.
- It is not needed merely to expose the MCP tools.
- Claude and Antigravity behavior must remain unchanged.

Tests must cover:

- Claude project-directory corroboration.
- Antigravity project-directory corroboration.
- Codex corroboration, if a supported source exists.
- MCP roots.
- Mismatches.
- Symlinks.
- No-source behavior.

#### Approval boundary

Any Rust change in this phase requires a second explicit review based on the
evidence collected during Phase 5.

### Phase 7 — Documentation and compatibility guards

Update `README.md` additively:

- Describe Codex as a supported architect.
- Add Codex installation and invocation examples.
- Provide the mapping between Claude and Codex skill names.
- Explain that Claude transcript cost harvesting is unavailable under Codex
  initially.
- Keep all existing Claude Code and Antigravity instructions.

Add automated guards for:

- JSON validity of all three host manifests.
- Codex plugin validation.
- Codex skill validation.
- Required MCP server command and ten-tool documentation.
- No accidental removal or mutation of Claude manifests or skills.
- Shared template references remaining valid.
- Version alignment between manifests.

## Validation matrix

| Surface | Required result |
|---|---|
| Rust workspace | Existing format, build, clippy, and test gates pass |
| MCP protocol | Ten tools list and call successfully |
| Claude Code | Existing plugin install and slash skills remain unchanged |
| Antigravity | Existing manifest, `.mcp.json`, and rules remain functional |
| Codex CLI | Plugin installs; five `$rexymcp-*` skills and MCP tools load |
| Codex IDE | Standalone/repo skills work and MCP configuration loads |
| Bootstrap | Existing user instruction files are preserved |
| Auto loop | Same review gates and stop conditions on every host |
| Telemetry | Executor telemetry remains intact; unavailable Codex architect usage is reported, never estimated |

## Explicit non-goals

- No executor, governor, verifier, parser, or phase-result changes.
- No change to the SDLC lifecycle or review standard.
- No replacement of Claude packaging.
- No renaming of existing Claude slash commands.
- No new dependencies unless a later phase proves one unavoidable.
- No Codex transcript parser in the initial integration.
- No generalized architect pricing or model schema in the initial integration.
- No public marketplace submission until local installation and cross-host
  tests pass.

## Implementation approval sequence

1. Implement Phases 1–3.
2. Validate their static packaging and skill behavior.
3. Perform Phase 5's installed-plugin smoke test.
4. Implement Phase 4 after the basic Codex skill and MCP surfaces are proven.
5. Return for explicit review before making any Phase 6 Rust/security-boundary
   change.
6. Complete Phase 7 documentation and compatibility guards after runtime
   behavior is established.

The governing rule throughout is compatibility first: prefer additive
host-specific adapters, reuse the shared workflow contract and templates, and
change rexyMCP core functionality only when a failing integration test proves
that no packaging-level solution is sufficient.
