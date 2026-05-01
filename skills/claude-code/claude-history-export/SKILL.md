---
name: claude-history-export
description: Export Claude Code chat histories for a specific project to Markdown using the non-interactive claude-history export claude command. Use when a user asks to export, archive, migrate, or inspect Claude Code project chat history without tool calls or thinking.
---

# Claude History Export

Use this skill to export Claude Code conversations for one project into
Markdown files that contain only user messages and agent messages.

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
claude-history export claude \
  --project-dir "$PROJECT_DIR" \
  --output-dir "$OUTPUT_DIR"
```

4. Verify the export:

```sh
find "$OUTPUT_DIR" -maxdepth 1 -type f -name '*.md' | sort
rg -n 'tool_use|tool_result|parent_tool_use_id|<usage>|</usage>|<thinking>|thinking_delta|toolUseResult' "$OUTPUT_DIR" || true
```

The export command is intentionally non-interactive. Do not open the TUI for
batch exports.

## Output Semantics

The Markdown files include top-level user messages and top-level Claude/agent
messages. Tool calls, tool results, thinking blocks, subagent internals, and
usage metadata are omitted.
