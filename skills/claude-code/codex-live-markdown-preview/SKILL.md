---
name: codex-live-markdown-preview
description: Start a local Bun web preview that renders a Codex CLI chat as live Markdown in a CMUX browser split. Use when the user asks from Claude Code to show, open, launch, restart, or view a live Markdown preview of a Codex chat, including rendered tables and local Markdown file links.
---

# Codex Live Markdown Preview

Use this skill to start or restart the Codex live Markdown browser preview from
Claude Code. The preview tool renders Codex CLI rollout files, not Claude Code
JSONL transcripts.

## Start Preview

Run from any project:

```sh
clivp
```

The preview server:

- uses `CODEX_THREAD_ID` when present
- runs under a per-thread tmux session named `codex-live-preview-*`
- otherwise falls back to the newest Codex session for the current project, then
  the newest Codex session overall
- serves the rendered chat on the first free localhost port starting at `4777`
- registers with a per-workspace manager named `codex-live-preview-manager-*`
  that opens or reuses one CMUX split browser and switches it to the active
  registered chat
- renders GitHub-flavored Markdown tables, code blocks, links, and inline styles
- rewrites local `.md` file paths and Markdown links so clicking them opens a
  rendered document view, without rewriting code blocks

## Useful Options

Start scanning for a free port from a different base:

```sh
clivp start --port-start 4900
```

Preview a specific Codex rollout file:

```sh
clivp start --session /path/to/rollout.jsonl
```

Preview the newest Codex session for a project:

```sh
clivp start --project-dir "$PWD"
```

Force a specific port only when the user explicitly asks for one:

```sh
clivp start --port 4778
```

## Manage Running Previews

```sh
clivp list
clivp stop
clivp stop-workspace
clivp stop-all
clivp logs
clivp workspace-logs
clivp cleanup
```
