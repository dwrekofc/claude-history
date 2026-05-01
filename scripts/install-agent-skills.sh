#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

claude_home="${CLAUDE_CONFIG_DIR:-"$HOME/.claude"}"
codex_home="${CODEX_HOME:-"$HOME/.codex"}"
user_bin="$HOME/.local/bin"

install_skill() {
  local source_dir="$1"
  local target_dir="$2"

  mkdir -p "$(dirname "$target_dir")"
  rm -rf "$target_dir"
  cp -R "$source_dir" "$target_dir"
  printf 'Installed %s\n' "$target_dir"
}

install_skill \
  "$repo_root/skills/claude-code/claude-history-export" \
  "$claude_home/skills/claude-history-export"

install_skill \
  "$repo_root/skills/claude-code/codex-live-markdown-preview" \
  "$claude_home/skills/codex-live-markdown-preview"

install_skill \
  "$repo_root/skills/codex/claude-history-export" \
  "$codex_home/skills/claude-history-export"

install_skill \
  "$repo_root/skills/codex/codex-live-markdown-preview" \
  "$codex_home/skills/codex-live-markdown-preview"

mkdir -p "$user_bin"
install -m 755 "$repo_root/scripts/codex-live-preview" "$user_bin/codex-live-preview"
printf 'Installed %s\n' "$user_bin/codex-live-preview"
ln -sf "$user_bin/codex-live-preview" "$user_bin/clivp"
printf 'Installed %s\n' "$user_bin/clivp"

printf '\nRestart Claude Code or Codex CLI if the skill list is already loaded.\n'
