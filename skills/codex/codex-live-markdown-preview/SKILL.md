---
name: codex-live-markdown-preview
description: Start a local Bun web preview that renders the active Codex CLI chat as live Markdown in a CMUX browser split. Use when the user asks to show, open, launch, restart, or view a live Markdown preview of the current Codex chat, including rendered tables and local Markdown file links.
---

# Codex Live Markdown Preview

Use this skill to start or restart the live browser preview for the active Codex
CLI conversation.

## Start Preview

Run from any project:

```sh
clivp
```

The preview server:

- finds the current Codex rollout from `CODEX_THREAD_ID`
- runs under a per-thread tmux session named `codex-live-preview-*`
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

Preview a specific rollout file:

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
