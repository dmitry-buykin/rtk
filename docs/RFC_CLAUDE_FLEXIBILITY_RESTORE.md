# RFC: Restore Claude-Friendly Command Flexibility in RTK Hook

## Status

Draft

## Date

2026-02-15

## Authors

RTK maintainers

## Context

Recent hardening improved safety but reduced compatibility with common Claude Code command patterns (especially chained/wrapped shell commands). The result is operational regressions such as failed rewrites, fallback noise, and broken user workflows.

This RFC proposes a controlled rollback from strict matching to upstream-compatible flexible matching, then adds explicit Claude-focused improvements.

## Problem Statement

Current behavior is more restrictive than upstream and less tolerant of common shell forms used by Claude Code:

1. Matching is too narrow for wrapped commands (`command <cmd>`, `builtin <cmd>`, env prefixes, compact shell forms).
2. Chained commands (`cmd1 && cmd2`, `cmd1; cmd2`) are not handled robustly for rewrite opportunities.
3. Subcommand-aware logic present upstream for `git`, `cargo`, `docker`, and `kubectl` was reduced.
4. Some useful handlers/patterns (`tree`, `find`, `wget`) are missing relative to upstream behavior.
5. Hook rewrite path can still depend on runtime `PATH` in some code paths, causing `rtk: command not found` in Claude environments.

## Historical Baseline

### Fork history (high level)

Hook evolution in this fork includes commits around:
- `ff1c759` (initial hook addition)
- `9620f66` (hook-first init changes)
- later hardening commits: `a005bb1`, `4aafc83`, `6612f22`, `95cf80b`, `ce6808a`

These changes improved control but introduced compatibility regressions.

### Upstream behavior (reference)

Upstream `rtk-ai/rtk` hook behavior remains more flexible:
- strips env prefix before matching
- supports subcommand-aware matching for `git/cargo/docker/kubectl`
- includes broader pattern coverage (including `tree/find/wget`)
- integrates with Claude via PreToolUse hook, now mostly through `rtk init -g`

## Goals

1. Restore practical compatibility with Claude Code command output and execution styles.
2. Keep security posture through redaction and policy controls, not brittle parsing.
3. Preserve easy installation (`rtk init -g`) with robust runtime path behavior.
4. Keep behavior generic for non-Claude projects.

## Non-Goals

1. Perfect shell parsing of all Bash/Zsh grammar.
2. Blocking all potentially sensitive commands at hook layer.
3. Replacing Claude permission controls.

## Proposal

### 1) Re-align hook matcher to upstream-flex baseline

Reintroduce tolerant first-stage normalization:
- unwrap leading wrappers: `command`, `builtin`
- normalize env-prefix assignments for matching (`FOO=bar cmd ...`)
- keep existing quoting intact (no destructive tokenization)

Reintroduce upstream-style subcommand extraction:
- `git <subcmd>`
- `cargo <subcmd>`
- `docker <subcmd>`
- `kubectl <subcmd>`

Restore missing common patterns:
- `tree`
- `find`
- `wget`

### 2) Add chain-aware rewrite opportunities

For command lines with separators (`&&`, `;`, `||`):
- inspect each segment independently for rewrite candidates
- rewrite only matched segments
- preserve original separators and non-matched segments unchanged

Scope constraints:
- no full shell AST requirement
- start with delimiter-aware segmentation that avoids obvious quoted separators
- if parsing is ambiguous, fail open (keep original segment)

### 3) Harden integration path (Claude-safe runtime)

`rtk init -g` should emit hook invocation with:
- explicit `RTK_BIN=<absolute-path>`
- explicit safe `PATH` prefix including cargo/homebrew/system bins

Hook rewrite output should prefer `${RTK_BIN:-rtk}` and avoid bare `rtk` when `RTK_BIN` is set.

### 4) Move security from strict matching to safe output handling

Introduce/expand built-in redaction in discover/report output:
- redact common secret patterns in args and env assignments
- redact credential-like query params and token values
- avoid logging raw `.env` values

Add hook modes:
- `RTK_HOOK_MODE=flex` (default): Claude-friendly tolerant mode
- `RTK_HOOK_MODE=strict`: optional stricter legacy behavior for high-control environments

## Security Model

This RFC deliberately trades parser strictness for workflow reliability while preserving safety through:

1. Non-destructive matching (unknowns pass through unchanged).
2. Redaction of sensitive substrings in tracked/discovered output.
3. Optional strict mode for teams that require tighter controls.
4. Existing Claude permission gates remain authoritative for tool execution.

## Implementation Plan

1. Hook parity restore
- reintroduce upstream subcommand matching and missing patterns
- restore tolerant wrapper/env normalization

2. Chain-aware parser hardening
- add segment-level matching for `&&`, `;`, `||`
- protect quoted contexts; fail open on ambiguity

3. Runtime integration reliability
- ensure `init -g` always writes RTK_BIN/PATH-safe hook command
- align repo hook and generated hook behavior

4. Redaction expansion
- add built-in rules for discover/report outputs
- cover env-style secrets and common token keys

5. Documentation
- update README/TROUBLESHOOTING for flexible mode vs strict mode
- include migration note for users who saw `rtk not found`

## Acceptance Criteria

The following commands should work in Claude Code without fallback errors and without exposing raw secrets in summaries:

1. `ls -la`
2. `command ls -la /path`
3. `mkdir -p out && gsutil -m rsync -r gs://bucket/path out/`
4. `source .env && uv run python scripts/job.py`
5. `ENV=prod uv run python -c "print('ok')"`
6. `gcloud artifacts repositories describe repo --location=us-central1`
7. `bq query --use_legacy_sql=false "SELECT 1"`
8. `sqlite3 local.db "select count(*) from events;"`
9. `git status && git log -1 --oneline`

## Risks and Mitigations

1. Risk: broader matching may capture unexpected forms.
- Mitigation: segment fail-open behavior and strict mode toggle.

2. Risk: redaction gaps for edge-case secrets.
- Mitigation: centralize redact rules and allow quick hotfix extensions.

3. Risk: behavior drift from upstream over time.
- Mitigation: keep an explicit upstream-parity test fixture set for hook matching.

## Rollout Strategy

1. Land behind default `flex` mode with `strict` fallback.
2. Announce migration in release notes as compatibility restoration.
3. Monitor for false rewrites and redaction misses; patch quickly.

## Open Questions

1. Should chain-aware rewriting include pipe segments (`|`) in phase 1, or defer to phase 2?
2. Should strict mode be runtime env-only, or also exposed via `rtk init` flag?
3. Do we want a dedicated `rtk doctor --claude` check to validate hook wiring and PATH?

## Summary

Adopt upstream-like flexible matching as baseline, add Claude-specific chain tolerance, and keep security by redaction + policy modes instead of brittle command rejection. This restores day-to-day usability while maintaining safe defaults for sensitive environments.
