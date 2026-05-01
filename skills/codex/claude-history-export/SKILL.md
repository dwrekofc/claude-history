---
name: codex-history-export
description: Export Codex CLI chat histories for a specific project to Markdown using the non-interactive claude-history export codex command. Use when a user asks to export, archive, migrate, or inspect Codex CLI project chat history without tool calls or reasoning.
---

# Codex History Export

Use this skill to export Codex CLI conversations for one project into Markdown
files that contain only user messages and agent messages.

## Requirements

`claude-history` must be installed from this fork:

```sh
cargo install --git https://github.com/dwrekofc/claude-history.git
```

## Workflow

1. Determine the target project directory. If the user says "this project", use
   the current working directory.
2. Determine the output directory. Create it if needed.
3. Run:

```sh
claude-history export codex \
  --project-dir "$PROJECT_DIR" \
  --output-dir "$OUTPUT_DIR"
```

4. Verify the export:

```sh
find "$OUTPUT_DIR" -maxdepth 1 -type f -name '*.md' | sort
rg -n 'function_call|function_call_output|encrypted_content|turn_context|<environment_context>|AGENTS.md instructions' "$OUTPUT_DIR" || true
```

The export command is intentionally non-interactive. Do not open the TUI for
batch exports.

## Output Semantics

The Markdown files include top-level user messages and top-level Codex/agent
messages. Tool calls, tool results, reasoning blocks, turn context, session
metadata, and automatic environment/instruction context are omitted.
