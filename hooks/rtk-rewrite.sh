#!/bin/bash
# RTK auto-rewrite hook for Claude Code PreToolUse:Bash
# Transparently rewrites raw commands to their rtk equivalents.
# Outputs JSON with updatedInput to modify the command before execution.

# Guards: skip silently if dependencies missing
if ! command -v jq &>/dev/null; then
  exit 0
fi
if [ -z "${RTK_BIN:-}" ] && ! command -v rtk &>/dev/null; then
  exit 0
fi

set -euo pipefail
RTK_BIN="${RTK_BIN:-$(command -v rtk)}"
RTK_CMD="${RTK_BIN}"
HOOK_MODE="${RTK_HOOK_MODE:-flex}"

INPUT=$(cat)
CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

if [ -z "$CMD" ]; then
  exit 0
fi

case "$HOOK_MODE" in
  strict|flex) ;;
  *) HOOK_MODE="flex" ;;
esac

trim_leading() {
  local s="$1"
  printf "%s" "${s#"${s%%[![:space:]]*}"}"
}

trim_trailing() {
  local s="$1"
  local trailing="${s##*[![:space:]]}"
  printf "%s" "${s%"$trailing"}"
}

trim() {
  local s
  s="$(trim_leading "$1")"
  trim_trailing "$s"
}

is_rtk_command() {
  local c
  c="$(trim "$1")"
  case "$c" in
    rtk\ *|*/rtk\ *) return 0 ;;
    *) return 1 ;;
  esac
}

# Globals populated by strip_prefixes_for_match()
ENV_PREFIX=""
PREFIX_CHAIN=""
MATCH_CMD=""

strip_prefixes_for_match() {
  local segment_core="$1"
  local current="$segment_core"
  ENV_PREFIX=""
  PREFIX_CHAIN=""

  # Strip env-like prefixes (sudo/env/VAR=val), preserving exact spacing.
  while true; do
    if [[ "$current" =~ ^([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]+[[:space:]]+)(.*)$ ]]; then
      ENV_PREFIX+="${BASH_REMATCH[1]}"
      current="${BASH_REMATCH[2]}"
      continue
    fi
    if [[ "$current" =~ ^(env[[:space:]]+)(.*)$ ]]; then
      ENV_PREFIX+="${BASH_REMATCH[1]}"
      current="${BASH_REMATCH[2]}"
      continue
    fi
    if [[ "$current" =~ ^(sudo[[:space:]]+)(.*)$ ]]; then
      ENV_PREFIX+="${BASH_REMATCH[1]}"
      current="${BASH_REMATCH[2]}"
      continue
    fi
    break
  done

  # Strip wrappers/runners for pattern matching (preserve for reconstruction).
  while true; do
    if [[ "$current" =~ ^((command|builtin)[[:space:]]+)(.*)$ ]]; then
      PREFIX_CHAIN+="${BASH_REMATCH[1]}"
      current="${BASH_REMATCH[3]}"
      continue
    fi

    if [[ "$current" =~ ^((uv|poetry|pipenv|hatch|rye)[[:space:]]+run[[:space:]]+)(.*)$ ]]; then
      PREFIX_CHAIN+="${BASH_REMATCH[1]}"
      current="${BASH_REMATCH[3]}"
      continue
    fi

    if [[ "$current" =~ ^((npm[[:space:]]+exec([[:space:]]+--)?[[:space:]]+|pnpm[[:space:]]+exec([[:space:]]+--)?[[:space:]]+))(.*)$ ]]; then
      PREFIX_CHAIN+="${BASH_REMATCH[1]}"
      current="${BASH_REMATCH[5]}"
      continue
    fi

    break
  done

  MATCH_CMD="$current"
}

rewrite_inner() {
  local cmd="$1"
  local cmd_trimmed
  cmd_trimmed="$(trim "$cmd")"
  if [ -z "$cmd_trimmed" ]; then
    printf ""
    return
  fi

  if is_rtk_command "$cmd_trimmed"; then
    printf ""
    return
  fi

  local first second third
  read -r first second third _ <<< "$cmd_trimmed"

  case "$first" in
    git)
      case "$second" in
        status|diff|log|add|commit|push|pull|branch|fetch|stash|show|worktree)
          printf "%s %s" "$RTK_CMD" "$cmd_trimmed"
          return
          ;;
      esac
      ;;
    gh)
      case "$second" in
        pr|issue|run|api|release)
          printf "%s %s" "$RTK_CMD" "$cmd_trimmed"
          return
          ;;
      esac
      ;;
    cargo)
      case "$second" in
        test|build|clippy|check|install|fmt)
          printf "%s %s" "$RTK_CMD" "$cmd_trimmed"
          return
          ;;
      esac
      ;;
    docker)
      case "$second" in
        compose|ps|images|logs|run|build|exec)
          printf "%s %s" "$RTK_CMD" "$cmd_trimmed"
          return
          ;;
      esac
      ;;
    kubectl)
      case "$second" in
        get|logs|describe|apply)
          printf "%s %s" "$RTK_CMD" "$cmd_trimmed"
          return
          ;;
      esac
      ;;
    gcloud|bq|sqlite3|gsutil)
      printf "%s proxy %s" "$RTK_CMD" "$cmd_trimmed"
      return
      ;;
    curl|wget)
      printf "%s %s" "$RTK_CMD" "$cmd_trimmed"
      return
      ;;
    cat)
      if [[ "$cmd_trimmed" == cat\ * ]]; then
        printf "%s read %s" "$RTK_CMD" "${cmd_trimmed#cat }"
        return
      fi
      ;;
    rg|grep)
      if [[ "$cmd_trimmed" == "$first "* ]]; then
        printf "%s grep %s" "$RTK_CMD" "${cmd_trimmed#"$first "}"
        return
      fi
      ;;
    ls)
      printf "%s ls%s" "$RTK_CMD" "${cmd_trimmed#ls}"
      return
      ;;
    tree)
      printf "%s ls%s" "$RTK_CMD" "${cmd_trimmed#tree}"
      return
      ;;
    find)
      printf "%s find%s" "$RTK_CMD" "${cmd_trimmed#find}"
      return
      ;;
    diff)
      if [[ "$cmd_trimmed" == diff\ * ]]; then
        printf "%s diff %s" "$RTK_CMD" "${cmd_trimmed#diff }"
        return
      fi
      ;;
    head)
      if [[ "$cmd_trimmed" =~ ^head[[:space:]]+-([0-9]+)[[:space:]]+(.+)$ ]]; then
        printf "%s read %s --max-lines %s" "$RTK_CMD" "${BASH_REMATCH[2]}" "${BASH_REMATCH[1]}"
        return
      fi
      if [[ "$cmd_trimmed" =~ ^head[[:space:]]+--lines=([0-9]+)[[:space:]]+(.+)$ ]]; then
        printf "%s read %s --max-lines %s" "$RTK_CMD" "${BASH_REMATCH[2]}" "${BASH_REMATCH[1]}"
        return
      fi
      ;;
    npm)
      if [[ "$cmd_trimmed" == "npm test" ]]; then
        printf "%s npm test" "$RTK_CMD"
        return
      fi
      if [[ "$cmd_trimmed" == npm\ test\ * ]]; then
        printf "%s npm test%s" "$RTK_CMD" "${cmd_trimmed#npm test}"
        return
      fi
      if [[ "$cmd_trimmed" == npm\ run\ * ]]; then
        printf "%s npm %s" "$RTK_CMD" "${cmd_trimmed#npm run }"
        return
      fi
      ;;
    pnpm)
      if [[ "$second" == "test" ]]; then
        printf "%s vitest run%s" "$RTK_CMD" "${cmd_trimmed#pnpm test}"
        return
      fi
      if [[ "$second" == "tsc" ]]; then
        printf "%s tsc%s" "$RTK_CMD" "${cmd_trimmed#pnpm tsc}"
        return
      fi
      if [[ "$second" == "lint" ]]; then
        printf "%s lint%s" "$RTK_CMD" "${cmd_trimmed#pnpm lint}"
        return
      fi
      if [[ "$second" == "playwright" ]]; then
        printf "%s playwright%s" "$RTK_CMD" "${cmd_trimmed#pnpm playwright}"
        return
      fi
      if [[ "$second" == "vitest" ]]; then
        local tail="${cmd_trimmed#pnpm vitest}"
        tail="$(trim_leading "$tail")"
        if [[ "$tail" == run* ]]; then
          tail="$(trim_leading "${tail#run}")"
        fi
        if [ -n "$tail" ]; then
          printf "%s vitest run %s" "$RTK_CMD" "$tail"
        else
          printf "%s vitest run" "$RTK_CMD"
        fi
        return
      fi
      if [[ "$second" == "list" || "$second" == "ls" || "$second" == "outdated" ]]; then
        printf "%s %s" "$RTK_CMD" "$cmd_trimmed"
        return
      fi
      ;;
    python|python3)
      if [[ "$second" == "-m" && "$third" == "pytest" ]]; then
        printf "%s pytest%s" "$RTK_CMD" "${cmd_trimmed#"$first -m pytest"}"
        return
      fi
      if [[ "$second" == "-m" && "$third" == "ruff" ]]; then
        printf "%s ruff%s" "$RTK_CMD" "${cmd_trimmed#"$first -m ruff"}"
        return
      fi
      printf "%s proxy %s" "$RTK_CMD" "$cmd_trimmed"
      return
      ;;
    pytest)
      printf "%s pytest%s" "$RTK_CMD" "${cmd_trimmed#pytest}"
      return
      ;;
    ruff)
      case "$second" in
        check|format)
          printf "%s ruff%s" "$RTK_CMD" "${cmd_trimmed#ruff}"
          return
          ;;
      esac
      ;;
    pip)
      case "$second" in
        list|outdated|install|show)
          printf "%s pip%s" "$RTK_CMD" "${cmd_trimmed#pip}"
          return
          ;;
      esac
      ;;
    uv)
      if [[ "$second" == "pip" ]]; then
        local uv_sub
        uv_sub="$(trim_leading "${cmd_trimmed#uv pip}")"
        case "${uv_sub%%[[:space:]]*}" in
          list|outdated|install|show)
            printf "%s pip %s" "$RTK_CMD" "$uv_sub"
            return
            ;;
        esac
      fi
      ;;
    go)
      case "$second" in
        test|build|vet)
          printf "%s %s" "$RTK_CMD" "$cmd_trimmed"
          return
          ;;
      esac
      ;;
    golangci-lint)
      printf "%s golangci-lint%s" "$RTK_CMD" "${cmd_trimmed#golangci-lint}"
      return
      ;;
    vitest)
      local vitest_tail="${cmd_trimmed#vitest}"
      vitest_tail="$(trim_leading "$vitest_tail")"
      if [[ "$vitest_tail" == run* ]]; then
        vitest_tail="$(trim_leading "${vitest_tail#run}")"
      fi
      if [ -n "$vitest_tail" ]; then
        printf "%s vitest run %s" "$RTK_CMD" "$vitest_tail"
      else
        printf "%s vitest run" "$RTK_CMD"
      fi
      return
      ;;
    npx)
      if [[ "$second" == "vitest" ]]; then
        local npx_tail="${cmd_trimmed#npx vitest}"
        npx_tail="$(trim_leading "$npx_tail")"
        if [[ "$npx_tail" == run* ]]; then
          npx_tail="$(trim_leading "${npx_tail#run}")"
        fi
        if [ -n "$npx_tail" ]; then
          printf "%s vitest run %s" "$RTK_CMD" "$npx_tail"
        else
          printf "%s vitest run" "$RTK_CMD"
        fi
        return
      fi
      if [[ "$second" == "vue-tsc" ]]; then
        printf "%s tsc%s" "$RTK_CMD" "${cmd_trimmed#npx vue-tsc}"
        return
      fi
      if [[ "$second" == "tsc" ]]; then
        printf "%s tsc%s" "$RTK_CMD" "${cmd_trimmed#npx tsc}"
        return
      fi
      if [[ "$second" == "eslint" ]]; then
        printf "%s lint%s" "$RTK_CMD" "${cmd_trimmed#npx eslint}"
        return
      fi
      if [[ "$second" == "prettier" ]]; then
        printf "%s prettier%s" "$RTK_CMD" "${cmd_trimmed#npx prettier}"
        return
      fi
      if [[ "$second" == "playwright" ]]; then
        printf "%s playwright%s" "$RTK_CMD" "${cmd_trimmed#npx playwright}"
        return
      fi
      if [[ "$second" == "prisma" ]]; then
        printf "%s prisma%s" "$RTK_CMD" "${cmd_trimmed#npx prisma}"
        return
      fi
      ;;
    tsc)
      printf "%s tsc%s" "$RTK_CMD" "${cmd_trimmed#tsc}"
      return
      ;;
    vue-tsc)
      printf "%s tsc%s" "$RTK_CMD" "${cmd_trimmed#vue-tsc}"
      return
      ;;
    eslint)
      printf "%s lint%s" "$RTK_CMD" "${cmd_trimmed#eslint}"
      return
      ;;
    prettier)
      printf "%s prettier%s" "$RTK_CMD" "${cmd_trimmed#prettier}"
      return
      ;;
    playwright)
      printf "%s playwright%s" "$RTK_CMD" "${cmd_trimmed#playwright}"
      return
      ;;
    prisma)
      printf "%s prisma%s" "$RTK_CMD" "${cmd_trimmed#prisma}"
      return
      ;;
  esac

  printf ""
}

rewrite_segment() {
  local segment="$1"

  # Skip heredoc-like commands entirely.
  if [[ "$segment" == *'<<'* ]]; then
    printf "%s" "$segment"
    return
  fi

  local leading_ws core trailing_ws
  leading_ws="${segment%%[![:space:]]*}"
  core="${segment#$leading_ws}"
  trailing_ws="${core##*[![:space:]]}"
  core="${core%"$trailing_ws"}"

  if [ -z "$core" ]; then
    printf "%s" "$segment"
    return
  fi

  strip_prefixes_for_match "$core"

  if is_rtk_command "$MATCH_CMD"; then
    printf "%s" "$segment"
    return
  fi

  local rewritten_inner
  rewritten_inner="$(rewrite_inner "$MATCH_CMD")"
  if [ -z "$rewritten_inner" ]; then
    printf "%s" "$segment"
    return
  fi

  printf "%s%s%s%s%s" "$leading_ws" "$ENV_PREFIX" "$PREFIX_CHAIN" "$rewritten_inner" "$trailing_ws"
}

CHAIN_SEGMENTS=()
CHAIN_SEPARATORS=()

split_chain_with_separators() {
  local input="$1"
  CHAIN_SEGMENTS=()
  CHAIN_SEPARATORS=()

  if [[ "$input" == *'<<'* ]]; then
    CHAIN_SEGMENTS=("$input")
    return 0
  fi

  local len=${#input}
  local i=0
  local start=0
  local in_single=0
  local in_double=0
  local escaped=0

  while [ $i -lt $len ]; do
    local ch="${input:i:1}"
    local two="${input:i:2}"

    if [ $escaped -eq 1 ]; then
      escaped=0
      i=$((i + 1))
      continue
    fi

    if [[ "$ch" == "\\" && $in_single -eq 0 ]]; then
      escaped=1
      i=$((i + 1))
      continue
    fi

    if [[ "$ch" == "'" && $in_double -eq 0 ]]; then
      in_single=$((1 - in_single))
      i=$((i + 1))
      continue
    fi

    if [[ "$ch" == '"' && $in_single -eq 0 ]]; then
      in_double=$((1 - in_double))
      i=$((i + 1))
      continue
    fi

    if [ $in_single -eq 0 ] && [ $in_double -eq 0 ]; then
      if [[ "$two" == "&&" || "$two" == "||" ]]; then
        CHAIN_SEGMENTS+=("${input:start:i-start}")
        CHAIN_SEPARATORS+=("$two")
        i=$((i + 2))
        start=$i
        continue
      fi

      if [[ "$ch" == ";" || "$ch" == $'\n' ]]; then
        CHAIN_SEGMENTS+=("${input:start:i-start}")
        CHAIN_SEPARATORS+=("$ch")
        i=$((i + 1))
        start=$i
        continue
      fi
    fi

    i=$((i + 1))
  done

  CHAIN_SEGMENTS+=("${input:start}")

  # Ambiguous quoting: fail open.
  if [ $in_single -eq 1 ] || [ $in_double -eq 1 ]; then
    return 1
  fi

  return 0
}

rewrite_command_line() {
  local input="$1"

  if ! split_chain_with_separators "$input"; then
    # Ambiguous parsing: fail open.
    printf ""
    return
  fi

  local changed=0
  local i

  if [ "$HOOK_MODE" = "strict" ]; then
    if [ "${#CHAIN_SEGMENTS[@]}" -gt 0 ]; then
      local original="${CHAIN_SEGMENTS[0]}"
      local updated
      updated="$(rewrite_segment "$original")"
      if [ "$updated" != "$original" ]; then
        CHAIN_SEGMENTS[0]="$updated"
        changed=1
      fi
    fi
  else
    for ((i=0; i<${#CHAIN_SEGMENTS[@]}; i++)); do
      local original="${CHAIN_SEGMENTS[$i]}"
      local updated
      updated="$(rewrite_segment "$original")"
      if [ "$updated" != "$original" ]; then
        CHAIN_SEGMENTS[$i]="$updated"
        changed=1
      fi
    done
  fi

  if [ $changed -eq 0 ]; then
    printf ""
    return
  fi

  local rebuilt=""
  for ((i=0; i<${#CHAIN_SEGMENTS[@]}; i++)); do
    rebuilt+="${CHAIN_SEGMENTS[$i]}"
    if [ $i -lt ${#CHAIN_SEPARATORS[@]} ]; then
      rebuilt+="${CHAIN_SEPARATORS[$i]}"
    fi
  done

  printf "%s" "$rebuilt"
}

REWRITTEN="$(rewrite_command_line "$CMD")"

# If no rewrite needed, approve as-is
if [ -z "$REWRITTEN" ]; then
  exit 0
fi

# Build the updated tool_input with all original fields preserved, only command changed
ORIGINAL_INPUT=$(echo "$INPUT" | jq -c '.tool_input')
UPDATED_INPUT=$(echo "$ORIGINAL_INPUT" | jq --arg cmd "$REWRITTEN" '.command = $cmd')

# Output the rewrite instruction
jq -n \
  --argjson updated "$UPDATED_INPUT" \
  '{
    "hookSpecificOutput": {
      "hookEventName": "PreToolUse",
      "permissionDecision": "allow",
      "permissionDecisionReason": "RTK auto-rewrite",
      "updatedInput": $updated
    }
  }'
