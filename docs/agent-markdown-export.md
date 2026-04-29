# Agent Markdown Export

This fork adds a non-interactive export path for agents that need Claude Code
chat history from a specific project.

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

Run:

```sh
claude-history export-project-markdown \
  --project-dir /absolute/path/to/project \
  --output-dir /absolute/path/to/export/folder
```

Example:

```sh
claude-history export-project-markdown \
  --project-dir /Volumes/CORE-02/resources/sap-website-download/us.sitesucker.mac.sitesucker \
  --output-dir /Volumes/CORE-02/resources/sap-website-download/chat-history
```

The command:

- maps `--project-dir` to the matching Claude Code project history folder
  under `~/.claude/projects`
- exports one `.md` file per top-level conversation
- sorts sessions chronologically
- prefixes output filenames with a stable sequence number and timestamp
- omits `agent-*` sidecar logs
- omits tools, tool results, thinking blocks, subagent internals, and usage
  metadata

## Agent instructions

When asked to export chat history for a project:

1. Identify the target project directory. Prefer the current working directory
   if the user says "this project".
2. Choose or create an output directory.
3. Run `claude-history export-project-markdown --project-dir ... --output-dir ...`.
4. Verify the output:

```sh
find "$OUTPUT_DIR" -maxdepth 1 -type f -name '*.md' | sort
rg -n 'tool_use|tool_result|parent_tool_use_id|<usage>|</usage>|<thinking>|thinking_delta|toolUseResult' "$OUTPUT_DIR" || true
```

The `rg` command should print no matches unless those strings appear in normal
user or assistant prose.

Do not use the interactive TUI for batch export.
