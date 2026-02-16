# RTK + Claude Code Setup (Global, Stable)

Use this setup when you want one shared RTK hook for all projects.

## 1. Install/upgrade RTK

```bash
cargo install --path . --force
```

## 2. Install global hook and patch Claude settings

```bash
rtk init -g --auto-patch
```

This installs:
- `~/.claude/hooks/rtk-rewrite.sh`
- `~/.claude/RTK.md`
- `@RTK.md` reference in `~/.claude/CLAUDE.md`
- RTK `PreToolUse` hook in `~/.claude/settings.json`

## 3. Verify hook registration (global only)

```bash
jq -r '..|objects|select(has("command"))|.command' ~/.claude/settings.json | rg 'rtk-rewrite|RTK_BIN'
jq -r '..|objects|select(has("command"))|.command' .claude/settings.json 2>/dev/null | rg 'rtk-rewrite|RTK_BIN' || true
jq -r '..|objects|select(has("command"))|.command' .claude/settings.local.json 2>/dev/null | rg 'rtk-rewrite|RTK_BIN' || true
```

Expected:
- Global file shows one RTK hook command.
- Project-local files show no RTK hook entries.

## 4. Restart Claude Code and smoke test

In Claude Code, run:
- `ls -la`
- `git status`

Expected:
- Commands execute normally.
- Output is compacted via RTK rewrite.

## 5. If you see `command not found: rtk`

Ensure cargo bin is on PATH:

```bash
echo $PATH | rg '\.cargo/bin' || echo 'missing'
```

If missing (zsh):

```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

Then run again:

```bash
rtk --version
rtk init -g --auto-patch
```
