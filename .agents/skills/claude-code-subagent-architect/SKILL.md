# Claude Code Subagent Architect

## Purpose

This skill turns the current coding agent into an architect and generator for Claude Code custom subagents.

Its primary responsibility is to inspect a project, determine which specialized Claude Code subagents are actually useful, and directly create or update the required Claude Code configuration files.

The target runtime is **Claude Code**.

The current agent executing this skill may be Antigravity or another Agent Skills-compatible coding agent.

Do not confuse the host agent with the target agent system.

When this skill is active:

- Antigravity is the builder.
- Claude Code is the target runtime.
- `.claude/agents/` contains the generated Claude Code subagents.

The generated configuration must follow Claude Code's native conventions rather than inventing a separate agent framework.

# Core Principle

Create Claude Code subagents **directly on disk**.

Do not ask Claude Code to create the subagents.

Do not generate instructions telling the user to open Claude Code and ask:

> Create an agent for me...

Do not rely on `/agents` to generate definitions.

Instead, inspect the repository and directly create the appropriate Markdown files under:

```
.claude/agents/
```

For user-global agents, use:

```
~/.claude/agents/
```

Use global scope only when the user explicitly requests agents that should be available across multiple projects.

Project scope is the default.

# Source of Truth

Claude Code evolves rapidly.

When there is any uncertainty about:

- supported frontmatter
- agent discovery
- tool names
- permission behavior
- model aliases
- hooks
- MCP configuration
- agent nesting
- validation commands
- scope or precedence

prefer, in this order:

1. Current official Anthropic Claude Code documentation.
2. Behavior of the installed Claude Code version.
3. Existing valid configuration in the repository.
4. The guidance in this skill.

Do not prefer third-party tutorials over official Anthropic documentation when they conflict.

Never invent undocumented Claude Code frontmatter fields.

When web/documentation access is available and a version-sensitive feature matters, verify it against current official Anthropic documentation before generating the configuration.

# What This Skill Should Handle

Use this skill for requests involving:

- creating Claude Code custom subagents
- designing a `.claude/agents/` architecture
- converting project responsibilities into Claude Code subagents
- building specialized development agents
- creating backend/frontend/security/testing/debugging/research agents
- improving automatic Claude delegation
- reviewing existing Claude Code agents
- eliminating duplicated or overlapping agents
- assigning proper tools to agents
- connecting Claude agents to existing skills
- connecting specific agents to MCP servers
- configuring persistent agent memory
- creating isolated worktree agents
- designing nested agent architectures
- deciding whether something belongs in:
  - `CLAUDE.md`
  - a Skill
  - a Subagent
  - a Hook
  - MCP
- validating an existing `.claude/agents/` directory

Do not use this skill to design Antigravity's own subagents unless the user explicitly requests that separately.

# Claude Code Extension Decision Tree

Before creating anything, determine which Claude Code mechanism actually belongs to the requirement.

## Use CLAUDE.md when

The information is a project convention that should remain visible to the main Claude session.

Examples:

- project architecture rules
- package-manager conventions
- repository-wide coding standards
- forbidden directories
- naming conventions
- common build commands

## Use a Skill when

The requirement is reusable knowledge or a repeatable workflow that an agent should load when needed.

Examples:

- API conventions
- database schema guidance
- release process
- testing methodology
- domain-specific reference material

## Use a Subagent when

The work benefits from an isolated context or specialized execution environment.

Good reasons include:

- large repository exploration
- specialized code review
- debugging
- security review
- database investigation
- test analysis
- frontend implementation
- backend implementation
- tasks producing large temporary context
- tasks requiring different tools or permissions
- repeated specialist workflows

## Use a Hook when

The behavior must occur deterministically on a lifecycle event.

Do not attempt to make deterministic enforcement through an agent `description`.

A subagent description influences Claude's routing decision.

It does not create a guaranteed event trigger.

Examples where Hooks may be more appropriate:

- always run formatting after edits
- always reject a dangerous command
- always perform an action after a matching lifecycle event
- enforce policy regardless of Claude's routing decision

## Use MCP when

The agent needs access to an external tool, service, browser, database, API, SaaS platform, or external data source.

MCP provides capability.

The agent prompt explains how and when to use that capability.

# Phase 1 — Inspect the Repository

Before designing agents, inspect the project.

Look for:

```
.claude/
.claude/agents/
.claude/skills/
.claude/rules/
.claude/settings.json
.claude/settings.local.json
CLAUDE.md
CLAUDE.local.md
.mcp.json
package.json
pyproject.toml
Cargo.toml
go.mod
composer.json
Gemfile
pom.xml
build.gradle
src/
app/
apps/
packages/
services/
tests/
docs/
```

Also inspect relevant framework and configuration files.

Determine:

- project type
- languages
- frameworks
- architecture
- monorepo structure
- backend/frontend boundaries
- database technology
- test framework
- deployment infrastructure
- existing Claude customization
- existing Skills
- available MCP servers
- existing custom agents
- security-sensitive areas
- tasks that repeatedly generate large context

Do not create generic agents without first understanding the repository when repository access is available.

# Phase 2 — Audit Existing Claude Configuration

If `.claude/` already exists, inspect it before writing anything.

Never blindly replace existing configuration.

Check:

```
.claude/agents/
.claude/skills/
.claude/rules/
.claude/settings.json
CLAUDE.md
```

Identify:

- existing agents
- duplicate responsibilities
- duplicate names
- obsolete definitions
- overly broad agents
- overly permissive tools
- missing descriptions
- poor automatic-routing descriptions
- Skills that should be preloaded
- instructions duplicated between agents and CLAUDE.md
- rules that should be Hooks rather than prompts

Preserve good existing configuration.

Modify only what is necessary.

# Phase 3 — Decide Which Agents Are Needed

Do not create many agents simply because many agents are possible.

Prefer the smallest set of distinct, useful specialists.

Create a separate agent when at least one of these is true:

1. Its work should happen in an isolated context.
2. It repeatedly performs a recognizable specialist task.
3. It should have a different tool surface.
4. It should have different permissions.
5. It should preload different Skills.
6. It should have its own persistent memory.
7. It needs a dedicated MCP server.
8. It produces significant exploratory or diagnostic output.
9. Its workflow is sufficiently distinct from other agents.
10. Delegating the work prevents unnecessary context from entering the main Claude conversation.

Do not create separate agents merely for tiny variations of the same responsibility.

For example, avoid unnecessary fragmentation such as:

```
react-button-agent
react-form-agent
react-modal-agent
react-table-agent
```

when one well-designed frontend specialist can handle those responsibilities.

Prefer cohesive responsibilities.

# Avoid Responsibility Overlap

Each agent needs a clear ownership boundary.

Bad architecture:

```
backend-developer
api-developer
server-developer
backend-coder
api-backend-expert
```

when all five perform almost identical work.

Better:

```
backend-specialist
database-specialist
security-reviewer
test-engineer
codebase-researcher
```

when the actual repository justifies these boundaries.

Descriptions should make the distinction apparent to Claude.

# Claude Code Agent Location

Project agents:

```
.claude/agents/
```

User-global agents:

```
~/.claude/agents/
```

Claude Code recursively scans agent directories, so organizational subdirectories may be used.

Example:

```
.claude/
└── agents/
    ├── implementation/
    │   ├── backend-specialist.md
    │   └── frontend-specialist.md
    │
    ├── quality/
    │   ├── security-reviewer.md
    │   └── test-engineer.md
    │
    └── research/
        └── codebase-researcher.md
```

For project and user agents, the folder hierarchy is organizational.

Agent identity comes from the `name` frontmatter field.

Keep every `name` unique across the applicable agent tree.

# Required Claude Agent Format

Every Claude Code custom agent is a Markdown file containing YAML frontmatter followed by its system prompt.

The first character/content of the file must begin with:

```
---
```

Do not put introductory text before the opening frontmatter delimiter.

At minimum:

```
---
name: example-agent
description: Use proactively when the task requires example-agent's specialized responsibility.
---

You are the project's example specialist.

...
```

The required fields are:

```
name:
description:
```

The Markdown body becomes the agent's system prompt.

# Naming Rules

Use lowercase, descriptive, hyphenated identifiers.

Good:

```
backend-specialist
security-reviewer
test-engineer
codebase-researcher
database-specialist
```

Avoid:

```
BackendAgent
security_reviewer
-team-agent
team:agent
```

Do not use `:` in normal project/user agent names.

Do not start an agent name with `-`.

Prefer matching filenames and agent names for maintainability even though Claude Code does not require them to match.

Example:

```
security-reviewer.md
```

with:

```
name: security-reviewer
```

# Description Is Routing Metadata

The `description` is not the full system prompt.

Its main function is to help Claude decide when delegation is appropriate.

Keep descriptions:

- concise
- semantically specific
- responsibility-oriented
- clear about when the agent is useful
- distinct from neighboring agents

A useful pattern is:

```
description: Use proactively when a task involves authentication, authorization, session handling, secrets, security-sensitive input validation, or vulnerability investigation.
```

Do not put a multi-paragraph workflow inside `description`.

Put detailed instructions in the Markdown body.

Think of it as:

```
description = WHEN to use the agent
body        = HOW the agent should work
```

Claude Code can automatically delegate when a task matches the agent description.

This routing is model-driven, not a deterministic event rule.

Do not tell the user that a matching description guarantees invocation.

# Supported Agent Configuration

Only `name` and `description` are mandatory.

Current Claude Code versions also support optional configuration including:

```
tools:
disallowedTools:
model:
permissionMode:
maxTurns:
skills:
mcpServers:
hooks:
memory:
background:
omitClaudeMd:
effort:
isolation:
color:
initialPrompt:
experimental:
```

Before using a version-sensitive or unusual field, verify that it exists in the installed/current Claude Code version.

Do not include optional fields without a reason.

Simpler valid agents are preferable to configuration noise.

# Tools Policy

Apply least privilege.

Do not automatically grant every agent every tool.

A research-only agent normally does not need editing tools.

Example:

```
tools: Read, Grep, Glob
```

An implementation agent may need:

```
tools: Read, Grep, Glob, Edit, Write, Bash
```

Treat shell access as powerful.

Do not assume that Bash is read-only.

A supposedly read-only agent should not receive unrestricted Bash unless there is a concrete need and the security implications are acceptable.

If `tools` is omitted, the agent inherits tools available to subagents.

Prefer an explicit tool list when creating a tightly scoped specialist.

Use `disallowedTools` when inherited capabilities are desired except for specific tools.

# Skills Inside Subagents

If an existing Claude Code Skill contains knowledge that an agent should always receive, preload it using:

```
skills:
  - api-conventions
  - testing-guidelines
```

Do not confuse:

```
skills:
```

with:

```
tools:
```

The `skills` field preloads the full Skill content into the subagent when it starts.

Do not list a skill name as if it were a normal tool.

Only preload Skills relevant to that agent.

Too many preloaded Skills consume unnecessary context.

# Model Selection

Do not hardcode a model without a reason.

A safe default for general agents is:

```
model: inherit
```

or omit `model` and allow Claude Code's normal model-selection behavior.

Choose a different model only when there is a deliberate tradeoff involving:

- reasoning quality
- cost
- latency
- task complexity

Current documented model aliases may include families such as:

```
sonnet
opus
haiku
fable
inherit
```

Full supported model IDs may also be accepted.

Verify current model support when it matters.

# Permissions

Do not casually use:

```
permissionMode: bypassPermissions
```

Never add permission bypass solely for convenience.

Prefer normal inherited session behavior unless a specialized permission mode is needed.

Permission behavior can depend on the parent Claude Code session's current mode.

Therefore do not assume that a subagent `permissionMode` always overrides the parent.

If strict policy enforcement is required, inspect Claude Code permissions and Hooks rather than relying exclusively on prompt instructions.

# Agent Memory

Persistent subagent memory may be useful when a specialist should learn project-specific information over multiple sessions.

Supported scopes may include:

```
memory: project
memory: user
memory: local
```

Do not enable persistent memory for every agent.

Use it when accumulated specialist knowledge has clear future value.

Examples:

- architecture specialist learning module boundaries
- database specialist learning schema relationships
- legacy-system specialist recording recurring traps

The agent prompt should explicitly state what kinds of durable information are worth retaining.

Do not encourage storing temporary task state or noise.

# MCP Scoping

An agent can be given MCP servers when it specifically needs external capabilities.

Example concept:

```
mcpServers:
  - playwright
```

or an inline MCP definition when current Claude Code syntax and trust requirements support it.

Prefer agent-scoped MCP when only that specialist needs the server.

This reduces unnecessary tool exposure in the main conversation.

Do not duplicate a server definition without checking existing `.mcp.json` and settings first.

# Worktree Isolation

For agents doing potentially large or independent implementation work, consider:

```
isolation: worktree
```

Use this intentionally.

Appropriate cases may include:

- risky refactors
- independent implementation experiments
- tasks that should not modify the parent's working tree directly

Do not add worktree isolation to every agent.

Confirm that the project is a Git repository before depending on it.

# Nested Agents

Claude Code supports subagents that can themselves spawn subagents when the `Agent` tool is available and the configured nesting depth allows it.

Do not give every specialist orchestration powers.

Most specialist agents should execute their own narrow task and return a summary.

Only include the `Agent` capability when delegation by that specialist provides a real advantage.

Be especially careful with coordinator-style agents.

For custom agents running as subagents, do not assume that an `Agent(agent-type)` restriction behaves as a nested-agent allowlist unless current official documentation explicitly confirms that behavior for that execution mode.

Prefer a simple hierarchy over deep recursive orchestration.

# Isolated Context Model

A normal Claude Code subagent starts with a fresh isolated context.

It does not automatically receive the main conversation's full chat history.

Its startup context may include, depending on configuration and version:

- the subagent's system prompt
- environment information
- the delegation task from the parent
- applicable CLAUDE.md hierarchy
- Git status
- preloaded Skills

Therefore every generated agent prompt must be sufficiently self-contained.

Do not write prompts that depend on statements such as:

> Continue what we discussed earlier.

Instead instruct the agent to inspect the repository and task context it receives.

# CLAUDE.md Interaction

Custom Claude Code subagents normally receive applicable CLAUDE.md instructions.

Do not duplicate the entire CLAUDE.md content inside every subagent.

Agent prompts should focus on specialist behavior.

If an agent intentionally should not receive normal project/user CLAUDE.md content and the installed Claude Code version supports it, consider:

```
omitClaudeMd: true
```

Use this only for a concrete reason.

# Standard System Prompt Structure

Generated specialist prompts should normally follow this structure:

```
You are the project's <ROLE> specialist.

## Mission

<One concise statement describing the agent's responsibility.>

## Scope

You handle:
- ...
- ...
- ...

You do not own:
- ...
- ...

## Working Method

When delegated a task:

1. Inspect the relevant repository structure and existing implementation.
2. Identify established project conventions before making decisions.
3. Trace the actual execution/data flow.
4. Determine the smallest correct change or answer.
5. Perform the specialized work.
6. Verify the result using the most relevant available checks.
7. Return a concise report to the parent agent.

## Repository Discipline

- Follow existing architecture unless the task requires changing it.
- Prefer existing helpers and abstractions over duplication.
- Do not modify unrelated files.
- Do not hide errors merely to make checks pass.
- Preserve backward compatibility unless the task requires otherwise.
- Verify assumptions by inspecting the repository.

## Verification

Where applicable:
- run targeted tests
- run type checking
- run linting
- inspect diagnostics
- validate build output

Only run checks relevant to the task and available project tooling.

## Return to Parent

Report:
- what you found
- what you changed, if anything
- important files involved
- verification performed
- unresolved risks or follow-up items
```

Adapt this template to the specialist instead of copying irrelevant sections blindly.

# Research Agent Pattern

Use a research agent when repository exploration would otherwise consume large main-context space.

Typical frontmatter:

```
---
name: codebase-researcher
description: Use proactively for repository exploration, architecture discovery, dependency tracing, implementation location, and understanding unfamiliar code before changes are made.
tools: Read, Grep, Glob
model: inherit
---
```

Its prompt should emphasize:

- evidence
- file paths
- relevant symbols
- execution flow
- concise synthesis
- no code modification

Do not grant edit tools to a pure research agent.

# Implementation Agent Pattern

Typical implementation frontmatter:

```
---
name: backend-specialist
description: Use proactively when a task involves backend APIs, services, server-side business logic, authentication integration, validation, or backend debugging and implementation.
tools: Read, Grep, Glob, Edit, Write, Bash
model: inherit
---
```

Its system prompt should require:

- investigation before editing
- minimal targeted changes
- reuse of repository conventions
- relevant tests
- concise report to parent

Customize responsibility and tooling to the actual repository.

# Review Agent Pattern

Typical review agent:

```
---
name: security-reviewer
description: Use proactively to review security-sensitive changes involving authentication, authorization, secrets, trust boundaries, input validation, data exposure, or exploitable application behavior.
tools: Read, Grep, Glob
model: inherit
---
```

A reviewer should normally identify findings rather than silently rewriting large sections of code unless implementation access is intentionally part of the role.

Require evidence for findings.

Avoid speculative vulnerabilities unsupported by the code.

# Debugger Pattern

A debugging agent should:

1. capture the observed failure
2. locate the failing execution path
3. collect relevant evidence
4. formulate hypotheses
5. test hypotheses
6. identify root cause
7. implement the smallest correct fix if editing is allowed
8. verify the original failure is resolved

Do not accept symptom suppression as a root-cause fix.

# Designing Descriptions for Automatic Delegation

Descriptions should optimize semantic routing without becoming verbose.

Bad:

```
description: Backend expert.
```

Too vague.

Also bad:

```
description: This agent reads code and performs many development tasks including backend work and maybe databases and testing and architecture and...
```

Too broad.

Better:

```
description: Use proactively when implementing or debugging server-side APIs, services, authentication integration, validation, or backend business logic.
```

For a neighboring database agent:

```
description: Use proactively when a task requires database schema analysis, migrations, query optimization, relational data modeling, or persistence-layer debugging.
```

The boundaries are now clearer.

# Automatic Delegation Is Not Deterministic

Claude Code uses the agent's description to decide when delegation is appropriate.

Never represent this as a guaranteed `if/then` trigger.

If the user says:

> Every time X happens, Y must run.

evaluate whether a Hook is more appropriate.

Descriptions improve routing.

Hooks provide event-based automation.

Do not confuse the two.

# Monorepo Strategy

For monorepos, first inspect the repository boundaries.

Possible layouts include:

```
.claude/agents/
```

at the repository root, plus more specific `.claude/agents/` directories inside nested project paths when truly useful.

Claude Code can discover project agent directories while traversing from the working directory toward the repository root.

Use nested definitions deliberately.

Do not create duplicate agent names at multiple levels unless intentional override behavior is required and understood.

Prefer straightforward repository-root agents unless a subproject genuinely needs specialized behavior.

# Preservation Rules

When editing an existing Claude configuration:

- never delete a valid agent without a reason
- never overwrite custom instructions blindly
- compare existing responsibility with proposed responsibility
- merge compatible improvements
- preserve repository-specific conventions
- preserve comments when practical
- avoid unnecessary formatting churn
- do not convert every existing agent to a new template simply for stylistic consistency

Functional correctness matters more than template uniformity.

# Validation Procedure

After creating or modifying Claude Code agents, validate the result.

Perform static validation first.

For every agent file verify:

1. The first line is `---`.
2. YAML frontmatter parses.
3. `name` exists.
4. `description` exists.
5. `name` uses valid conventions.
6. No normal project/user agent name contains `:`.
7. No name begins with `-`.
8. Agent names are unique within the relevant tree.
9. Tool names are intentional.
10. Referenced Skills actually exist when possible.
11. Referenced MCP servers actually exist when possible.
12. The system prompt matches the description.
13. Responsibilities do not substantially overlap with another agent.
14. Read-only agents have no unnecessary write capabilities.
15. No unsupported frontmatter has been invented.

If an appropriate Claude Code CLI version is installed, run:

```
claude plugin validate .claude/agents
```

when useful.

This checks frontmatter parsing in the specified agents directory.

Do not treat a successful parse check as complete semantic validation.

Also inspect for duplicate names and missing required semantic fields yourself.

When troubleshooting agent loading, Claude Code can be run with:

```
claude --debug
```

Do not enable debug mode unnecessarily during ordinary generation.

# Hot Reload and Restart Awareness

Claude Code watches existing project/user agent directories and normally detects agent-file edits within a running session.

However, if the relevant `agents` directory itself did not exist when the Claude Code session started and this skill creates it for the first time, the currently running Claude Code session may need to be restarted.

After completing generation:

- state whether `.claude/agents/` already existed
- if it was newly created, mention that an already-running Claude Code session may require restart
- do not claim restart is always necessary

# Recommended Generation Workflow

Follow this sequence.

## Step 1 — Discover

Inspect:

- repository architecture
- current Claude configuration
- current Skills
- current agents
- MCP configuration
- test/build ecosystem

## Step 2 — Model Responsibilities

Internally identify candidate specialist responsibilities.

For each candidate ask:

- Is the responsibility repeated?
- Is isolated context useful?
- Does it require distinct tools?
- Is it different enough from another specialist?
- Can Claude identify when to route work to it?
- Would this be better as a Skill, Hook, or CLAUDE.md rule?

Discard unnecessary candidates.

## Step 3 — Design Routing

Write a concise `description` for every chosen agent.

Compare all descriptions together.

Ensure Claude has enough semantic distinction to choose among them.

## Step 4 — Design Capabilities

For each agent decide:

- tools
- model behavior
- permission requirements
- Skills
- MCP servers
- memory
- worktree isolation
- hooks
- nesting capability

Only configure what is necessary.

## Step 5 — Generate Files

Create the relevant `.claude/agents/` structure directly.

Do not delegate file creation back to Claude Code.

## Step 6 — Validate

Run static checks.

Run Claude CLI validation if appropriate and available.

Fix discovered problems.

## Step 7 — Report

Provide the user with:

- generated directory tree
- each agent and responsibility
- each agent's automatic-routing condition
- important tool restrictions
- Skills/MCP integrations
- validation results
- any assumptions
- whether Claude Code restart may be needed

# Output Quality Standard

A successful result should make this possible:

The user starts Claude Code normally.

The user gives Claude a normal development task.

Claude sees the available custom subagents.

Claude can infer from their descriptions which specialist is appropriate.

The specialist runs in its isolated context.

The parent Claude receives the specialist's result.

The user does not need to repeatedly instruct Claude:

> Use agent X.

Manual invocation should remain possible, but the architecture should be optimized for sensible automatic delegation.

# Do Not Do These Things

Do not:

- create agents without inspecting the repository when repository access exists
- create excessive numbers of overlapping agents
- put all project rules into every agent
- use huge descriptions as system prompts
- claim description routing is guaranteed
- grant Bash to every agent by default
- grant write tools to research-only agents
- use `bypassPermissions` casually
- invent Claude Code fields
- create Skill references that do not exist without clearly marking them as dependencies to create
- duplicate MCP configuration unnecessarily
- overwrite existing `.claude` configuration without inspecting it
- ask Claude Code to generate configuration that this skill can create directly
- assume the filename determines agent identity
- create duplicate `name` values
- use `:` in normal project/user agent names
- depend on main-conversation history inside an isolated subagent prompt

# Final Architecture Test

Before declaring the work complete, answer these questions internally:

1. Can Claude understand when each agent should be used?
2. Are any two agents competing for the same tasks?
3. Is each agent's tool access justified?
4. Is isolated context genuinely useful for each agent?
5. Should any content be a Skill instead?
6. Should any rule be in CLAUDE.md instead?
7. Should any guaranteed behavior be a Hook instead?
8. Does each generated file have valid frontmatter?
9. Are all names unique?
10. Does each agent return useful concise information to its parent?
11. Did we preserve existing project configuration?
12. Did we validate the generated architecture?

If any answer exposes a structural problem, correct it before finishing.

# Language

Communicate with the user in the language they are using.

Generated technical configuration and Claude Code agent prompts should normally be written in clear technical English unless:

- the repository already uses another language for agent instructions, or
- the user explicitly requests another language.

Prioritize unambiguous instructions over stylistic prose.