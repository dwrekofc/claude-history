# Agent Markdown Export

This fork adds non-interactive export paths for agents that need Claude Code or
Codex CLI chat history from a specific project.

## Build and install

Install directly from the fork:

```sh
cargo install --git https://github.com/dwrekofc/claude-history.git
```

Or build from a local checkout:

```sh
git clone https://github.com/dwrekofc/claude-history.git
cd claude-history
cargo build --release
install -m 755 target/release/claude-history ~/.local/bin/claude-history
```

Make sure `~/.local/bin` is on `PATH`, or install the binary somewhere already
on `PATH`.

## Export a specific project

Export Claude Code history:

```sh
claude-history export claude \
  --project-dir /absolute/path/to/project \
  --output-dir /absolute/path/to/export/folder
```

Export Codex CLI history:

```sh
claude-history export codex \
  --project-dir /absolute/path/to/project \
  --output-dir /absolute/path/to/export/folder
```

The old Claude-only command is still supported:

```sh
claude-history export-project-markdown \
  --project-dir /absolute/path/to/project \
  --output-dir /absolute/path/to/export/folder
```

Example:

```sh
claude-history export codex \
  --project-dir /Volumes/CORE-02/resources/sap-website-download/us.sitesucker.mac.sitesucker \
  --output-dir /Volumes/CORE-02/resources/sap-website-download/chat-history
```

The Claude command:

- maps `--project-dir` to the matching Claude Code project history folder
  under `~/.claude/projects`
- exports one `.md` file per top-level conversation
- sorts sessions by newest last-message activity
- prefixes output filenames with a stable sequence number and last-message timestamp
- omits `agent-*` sidecar logs
- omits tools, tool results, thinking blocks, subagent internals, and usage
  metadata

The Codex command:

- scans `~/.codex/sessions` for `rollout-*.jsonl`
- matches sessions whose recorded `cwd` canonicalizes exactly to `--project-dir`
- exports one `.md` file per conversation
- sorts sessions by newest last-message activity
- prefixes output filenames with a stable sequence number and last-message timestamp
- omits tool calls, tool results, reasoning blocks, turn context, and session
  metadata

## Agent instructions

When asked to export chat history for a project:

1. Identify the target project directory. Prefer the current working directory
   if the user says "this project".
2. Choose or create an output directory.
3. Run `claude-history export claude --project-dir ... --output-dir ...` for
   Claude Code, or `claude-history export codex --project-dir ... --output-dir ...`
   for Codex CLI.
4. Verify the output:

```sh
find "$OUTPUT_DIR" -maxdepth 1 -type f -name '*.md' | sort
rg -n 'tool_use|tool_result|parent_tool_use_id|<usage>|</usage>|<thinking>|thinking_delta|toolUseResult|function_call|function_call_output|encrypted_content|turn_context' "$OUTPUT_DIR" || true
```

The `rg` command should print no matches unless those strings appear in normal
user or assistant prose.

Do not use the interactive TUI for batch export.

## Live Codex Markdown Preview

For an active Codex CLI session, run the Bun preview server from this checkout:

```sh
bun --cwd /Volumes/CORE-02/projects/claude-history run live:cmux
```

The server:

- finds the current rollout from `CODEX_THREAD_ID`, falling back to the newest
  Codex session for the current project
- serves a live Markdown-rendered chat preview on localhost
- opens the preview in the CMUX split browser via `browser.open_split` when CMUX
  is available
- rewrites local `.md` file paths and Markdown links in the rendered chat so
  clicking them opens a rendered document view in the browser split

Useful options:

```sh
bun --cwd /Volumes/CORE-02/projects/claude-history run live -- --port 4778
bun --cwd /Volumes/CORE-02/projects/claude-history run live:cmux -- --session /path/to/rollout.jsonl
bun --cwd /Volumes/CORE-02/projects/claude-history run live:cmux -- --project-dir "$PWD"
```
