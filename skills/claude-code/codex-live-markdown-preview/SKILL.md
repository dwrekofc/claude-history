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
bun --cwd /Volumes/CORE-02/projects/claude-history run live:cmux
```

The preview server:

- uses `CODEX_THREAD_ID` when present
- otherwise falls back to the newest Codex session for the current project, then
  the newest Codex session overall
- serves the rendered chat at `http://127.0.0.1:4777/` by default
- opens or reuses the CMUX split browser via `browser.open_split`
- renders GitHub-flavored Markdown tables, code blocks, links, and inline styles
- rewrites local `.md` file paths and Markdown links so clicking them opens a
  rendered document view in the browser split

## Useful Options

Use a different port:

```sh
bun --cwd /Volumes/CORE-02/projects/claude-history run live:cmux -- --port 4778
```

Preview a specific Codex rollout file:

```sh
bun --cwd /Volumes/CORE-02/projects/claude-history run live:cmux -- --session /path/to/rollout.jsonl
```

Preview the newest Codex session for a project:

```sh
bun --cwd /Volumes/CORE-02/projects/claude-history run live:cmux -- --project-dir "$PWD"
```

## If The Server Is Already Running

If the port is occupied, either use another port or stop the old listener:

```sh
lsof -tiTCP:4777 -sTCP:LISTEN | xargs kill
```

Then start the preview again.
