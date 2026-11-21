# 0001. Use the dev-pipeline plugin

Status: accepted (2026-11-21)

## Context

The project is developed with Claude Code agents. Work needs a repeatable flow (task -> plan -> code -> tests -> review -> PR)
without copying agent definitions into every repository.

## Decision

Use the pipeline from the `dev-pipeline@gronck-local` plugin. Everything specific to this project lives in the
`## Pipeline contract` section of `CLAUDE.md` and in `docs/`. Branch model: `main` only (base and PR target).
Forge: GitHub via `gh`. PRs are opened by the pipeline; only the owner merges them, by squash, with green CI.

## Consequences

- The plugin must be installed and enabled on the machine that works on the repo.
- Sessions load the plugin's agents at start: after changing the plugin or the contract, restart sessions or `/reload-plugins`.
