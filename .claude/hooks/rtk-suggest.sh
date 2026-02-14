#!/bin/bash
# RTK suggest hook for Claude Code PreToolUse:Bash
# Emits system reminders when rtk-compatible commands are detected.
# Outputs JSON with systemMessage to inform Claude Code without modifying execution.

set -euo pipefail

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

if [ -z "$CMD" ]; then
  exit 0
fi

# Extract the first meaningful command (before pipes, &&, etc.)
FIRST_CMD="$CMD"

# Skip if already using rtk
case "$FIRST_CMD" in
  rtk\ *|*/rtk\ *) exit 0 ;;
esac

# Skip commands with heredocs, variable assignments, etc.
case "$FIRST_CMD" in
  *'<<'*) exit 0 ;;
esac

# Strip env var assignments for matching only.
ENV_PREFIX=$(echo "$FIRST_CMD" | grep -oE '^([A-Za-z_][A-Za-z0-9_]*=[^ ]* +)+' || echo "")
if [ -n "$ENV_PREFIX" ]; then
  MATCH_CMD="${FIRST_CMD:${#ENV_PREFIX}}"
else
  MATCH_CMD="$FIRST_CMD"
fi
CMD_BODY="$MATCH_CMD"

# Strip common runner wrappers to suggest based on inner command.
RUNNER_PREFIX=""
if [[ "$MATCH_CMD" == "uv run "* ]]; then
  RUNNER_PREFIX="uv run "
elif [[ "$MATCH_CMD" == "poetry run "* ]]; then
  RUNNER_PREFIX="poetry run "
elif [[ "$MATCH_CMD" == "pipenv run "* ]]; then
  RUNNER_PREFIX="pipenv run "
elif [[ "$MATCH_CMD" == "hatch run "* ]]; then
  RUNNER_PREFIX="hatch run "
elif [[ "$MATCH_CMD" == "rye run "* ]]; then
  RUNNER_PREFIX="rye run "
elif [[ "$MATCH_CMD" == "npm exec -- "* ]]; then
  RUNNER_PREFIX="npm exec -- "
elif [[ "$MATCH_CMD" == "npm exec "* ]]; then
  RUNNER_PREFIX="npm exec "
elif [[ "$MATCH_CMD" == "pnpm exec "* ]]; then
  RUNNER_PREFIX="pnpm exec "
fi

if [ -n "$RUNNER_PREFIX" ]; then
  MATCH_CMD="${MATCH_CMD:${#RUNNER_PREFIX}}"
  CMD_BODY="${CMD_BODY:${#RUNNER_PREFIX}}"
fi

case "$MATCH_CMD" in
  rtk\ *|*/rtk\ *) exit 0 ;;
esac

SUGGESTION=""

# --- Git commands ---
if echo "$MATCH_CMD" | grep -qE '^git\s+status(\s|$)'; then
  SUGGESTION="rtk git status"
elif echo "$MATCH_CMD" | grep -qE '^git\s+diff(\s|$)'; then
  SUGGESTION="rtk git diff"
elif echo "$MATCH_CMD" | grep -qE '^git\s+log(\s|$)'; then
  SUGGESTION="rtk git log"
elif echo "$MATCH_CMD" | grep -qE '^git\s+add(\s|$)'; then
  SUGGESTION="rtk git add"
elif echo "$MATCH_CMD" | grep -qE '^git\s+commit(\s|$)'; then
  SUGGESTION="rtk git commit"
elif echo "$MATCH_CMD" | grep -qE '^git\s+push(\s|$)'; then
  SUGGESTION="rtk git push"
elif echo "$MATCH_CMD" | grep -qE '^git\s+pull(\s|$)'; then
  SUGGESTION="rtk git pull"
elif echo "$MATCH_CMD" | grep -qE '^git\s+branch(\s|$)'; then
  SUGGESTION="rtk git branch"
elif echo "$MATCH_CMD" | grep -qE '^git\s+fetch(\s|$)'; then
  SUGGESTION="rtk git fetch"
elif echo "$MATCH_CMD" | grep -qE '^git\s+stash(\s|$)'; then
  SUGGESTION="rtk git stash"
elif echo "$MATCH_CMD" | grep -qE '^git\s+show(\s|$)'; then
  SUGGESTION="rtk git show"

# --- GitHub CLI ---
elif echo "$MATCH_CMD" | grep -qE '^gh\s+(pr|issue|run|api|release)(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^gh /rtk gh /')

# --- Cargo ---
elif echo "$MATCH_CMD" | grep -qE '^cargo\s+test(\s|$)'; then
  SUGGESTION="rtk cargo test"
elif echo "$MATCH_CMD" | grep -qE '^cargo\s+build(\s|$)'; then
  SUGGESTION="rtk cargo build"
elif echo "$MATCH_CMD" | grep -qE '^cargo\s+clippy(\s|$)'; then
  SUGGESTION="rtk cargo clippy"
elif echo "$MATCH_CMD" | grep -qE '^cargo\s+check(\s|$)'; then
  SUGGESTION="rtk cargo check"
elif echo "$MATCH_CMD" | grep -qE '^cargo\s+install(\s|$)'; then
  SUGGESTION="rtk cargo install"
elif echo "$MATCH_CMD" | grep -qE '^cargo\s+fmt(\s|$)'; then
  SUGGESTION="rtk cargo fmt"

# --- File operations ---
elif echo "$MATCH_CMD" | grep -qE '^cat\s+'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^cat /rtk read /')
elif echo "$MATCH_CMD" | grep -qE '^(rg|grep)\s+'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed -E 's/^(rg|grep) /rtk grep /')
elif echo "$MATCH_CMD" | grep -qE '^ls(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^ls/rtk ls/')
elif echo "$MATCH_CMD" | grep -qE '^tree(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^tree/rtk tree/')
elif echo "$MATCH_CMD" | grep -qE '^find\s+'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^find /rtk find /')
elif echo "$MATCH_CMD" | grep -qE '^diff\s+'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^diff /rtk diff /')
elif echo "$MATCH_CMD" | grep -qE '^head\s+'; then
  # Suggest rtk read with --max-lines transformation
  if echo "$MATCH_CMD" | grep -qE '^head\s+-[0-9]+\s+'; then
    LINES=$(echo "$MATCH_CMD" | sed -E 's/^head +-([0-9]+) +.+$/\1/')
    FILE=$(echo "$MATCH_CMD" | sed -E 's/^head +-[0-9]+ +(.+)$/\1/')
    SUGGESTION="rtk read $FILE --max-lines $LINES"
  elif echo "$MATCH_CMD" | grep -qE '^head\s+--lines=[0-9]+\s+'; then
    LINES=$(echo "$MATCH_CMD" | sed -E 's/^head +--lines=([0-9]+) +.+$/\1/')
    FILE=$(echo "$MATCH_CMD" | sed -E 's/^head +--lines=[0-9]+ +(.+)$/\1/')
    SUGGESTION="rtk read $FILE --max-lines $LINES"
  fi

# --- JS/TS tooling ---
elif echo "$MATCH_CMD" | grep -qE '^(pnpm\s+)?(npx\s+)?vitest(\s|$)'; then
  SUGGESTION="rtk vitest run"
elif echo "$MATCH_CMD" | grep -qE '^pnpm\s+test(\s|$)'; then
  SUGGESTION="rtk vitest run"
elif echo "$MATCH_CMD" | grep -qE '^npm\s+test(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^npm test/rtk npm test/')
elif echo "$MATCH_CMD" | grep -qE '^npm\s+run\s+'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^npm run /rtk npm /')
elif echo "$MATCH_CMD" | grep -qE '^pnpm\s+tsc(\s|$)'; then
  SUGGESTION="rtk tsc"
elif echo "$MATCH_CMD" | grep -qE '^(npx\s+)?(tsc|vue-tsc)(\s|$)'; then
  SUGGESTION="rtk tsc"
elif echo "$MATCH_CMD" | grep -qE '^pnpm\s+lint(\s|$)'; then
  SUGGESTION="rtk lint"
elif echo "$MATCH_CMD" | grep -qE '^(npx\s+)?eslint(\s|$)'; then
  SUGGESTION="rtk lint"
elif echo "$MATCH_CMD" | grep -qE '^(npx\s+)?prettier(\s|$)'; then
  SUGGESTION="rtk prettier"
elif echo "$MATCH_CMD" | grep -qE '^(npx\s+)?playwright(\s|$)'; then
  SUGGESTION="rtk playwright"
elif echo "$MATCH_CMD" | grep -qE '^pnpm\s+playwright(\s|$)'; then
  SUGGESTION="rtk playwright"
elif echo "$MATCH_CMD" | grep -qE '^(npx\s+)?prisma(\s|$)'; then
  SUGGESTION="rtk prisma"

# --- Containers ---
elif echo "$MATCH_CMD" | grep -qE '^docker\s+(compose|ps|images|logs|run|build|exec)(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^docker /rtk docker /')
elif echo "$MATCH_CMD" | grep -qE '^kubectl\s+(get|logs|describe|apply)(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^kubectl /rtk kubectl /')

# --- Network ---
elif echo "$MATCH_CMD" | grep -qE '^curl\s+'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^curl /rtk curl /')
elif echo "$MATCH_CMD" | grep -qE '^wget\s+'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^wget /rtk wget /')

# --- pnpm package management ---
elif echo "$MATCH_CMD" | grep -qE '^pnpm\s+(list|ls|outdated)(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^pnpm /rtk pnpm /')

# --- Python tooling ---
elif echo "$MATCH_CMD" | grep -qE '^pytest(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^pytest/rtk pytest/')
elif echo "$MATCH_CMD" | grep -qE '^python(3)?\s+-m\s+pytest(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed -E 's/^python3? -m pytest/rtk pytest/')
elif echo "$MATCH_CMD" | grep -qE '^ruff(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^ruff/rtk ruff/')
elif echo "$MATCH_CMD" | grep -qE '^python(3)?\s+-m\s+ruff(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed -E 's/^python3? -m ruff/rtk ruff/')
elif echo "$MATCH_CMD" | grep -qE '^pip\s+(list|outdated|install|show)(\s|$)'; then
  SUGGESTION=$(echo "$CMD_BODY" | sed 's/^pip /rtk pip /')
fi

# If no suggestion, allow command as-is
if [ -z "$SUGGESTION" ]; then
  exit 0
fi

# Output suggestion as system message
jq -n \
  --arg suggestion "$SUGGESTION" \
  '{
    "hookSpecificOutput": {
      "hookEventName": "PreToolUse",
      "permissionDecision": "allow",
      "systemMessage": ("⚡ RTK available: `" + $suggestion + "` (60-90% token savings)")
    }
  }'
