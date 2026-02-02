//! Conversation viewer rendering for TUI display.
//!
//! This module renders conversation JSONL files to `Vec<RenderedLine>` for display
//! in the TUI viewer. It produces styled spans that ratatui can render directly,
//! without using ANSI escape codes.

use crate::claude::{AssistantMessage, ContentBlock, LogEntry, UserContent};
use crate::tui::app::{LineStyle, RenderedLine};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

const NAME_WIDTH: usize = 9;
const TEAL: (u8, u8, u8) = (78, 201, 176);
const DIM_TEAL: (u8, u8, u8) = (60, 160, 140);
const SEPARATOR_COLOR: (u8, u8, u8) = (80, 80, 80);

/// Options for rendering a conversation
pub struct RenderOptions {
    pub show_tools: bool,
    pub show_thinking: bool,
    pub content_width: usize,
}

/// Render a conversation file to lines for display in the TUI viewer
pub fn render_conversation(
    file_path: &Path,
    options: &RenderOptions,
) -> std::io::Result<Vec<RenderedLine>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();

    for line_result in reader.lines() {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
            render_entry(&mut lines, &entry, options);
        }
    }

    Ok(lines)
}

fn render_entry(lines: &mut Vec<RenderedLine>, entry: &LogEntry, options: &RenderOptions) {
    match entry {
        LogEntry::Summary { .. }
        | LogEntry::FileHistorySnapshot { .. }
        | LogEntry::System { .. }
        | LogEntry::Progress { .. } => {}
        LogEntry::User { message, .. } => {
            render_user_message(lines, message, options);
        }
        LogEntry::Assistant { message, .. } => {
            render_assistant_message(lines, message, options);
        }
    }
}

fn render_user_message(
    lines: &mut Vec<RenderedLine>,
    message: &crate::claude::UserMessage,
    options: &RenderOptions,
) {
    // Extract text from user message
    let text = match &message.content {
        UserContent::String(s) => process_command_message(s),
        UserContent::Blocks(blocks) => {
            let mut result = None;
            for block in blocks {
                if let ContentBlock::Text { text } = block
                    && let Some(processed) = process_command_message(text)
                {
                    result = Some(processed);
                    break;
                }
            }
            result
        }
    };

    if let Some(text) = text {
        let wrapped = wrap_text(&text, options.content_width);
        render_ledger_block(lines, "You", TEAL, true, &wrapped);
        lines.push(RenderedLine { spans: vec![] }); // Empty line after message
    }
}

fn render_assistant_message(
    lines: &mut Vec<RenderedLine>,
    message: &AssistantMessage,
    options: &RenderOptions,
) {
    let mut printed = false;

    // Text blocks
    for block in &message.content {
        if let ContentBlock::Text { text } = block {
            let wrapped = wrap_text(text, options.content_width);
            render_ledger_block(lines, "Claude", TEAL, true, &wrapped);
            printed = true;
        }
    }

    // Tool calls (if enabled)
    if options.show_tools {
        for block in &message.content {
            if let ContentBlock::ToolUse { name, input, .. } = block {
                let header = format!("<Calling: {}>", name);
                render_ledger_block(lines, "Claude", DIM_TEAL, false, &header);
                if let Ok(formatted) = serde_json::to_string_pretty(input) {
                    render_continuation(lines, &formatted);
                }
                printed = true;
            }
        }
    }

    // Thinking blocks (if enabled)
    if options.show_thinking {
        for block in &message.content {
            if let ContentBlock::Thinking { thinking, .. } = block {
                let wrapped = wrap_text(thinking, options.content_width);
                render_ledger_block(lines, "Thinking", DIM_TEAL, false, &wrapped);
                printed = true;
            }
        }
    }

    if printed {
        lines.push(RenderedLine { spans: vec![] });
    }
}

fn render_ledger_block(
    lines: &mut Vec<RenderedLine>,
    name: &str,
    color: (u8, u8, u8),
    bold: bool,
    text: &str,
) {
    for (i, line_text) in text.lines().enumerate() {
        let mut spans = Vec::new();

        // Name column (right-aligned, only on first line)
        let name_text = if i == 0 {
            format!("{:>width$}", name, width = NAME_WIDTH)
        } else {
            " ".repeat(NAME_WIDTH)
        };

        spans.push((
            name_text,
            LineStyle {
                fg: Some(color),
                bold,
                dimmed: false,
            },
        ));

        // Separator
        spans.push((
            " │ ".to_string(),
            LineStyle {
                fg: Some(SEPARATOR_COLOR),
                ..Default::default()
            },
        ));

        // Content
        spans.push((line_text.to_string(), LineStyle::default()));

        lines.push(RenderedLine { spans });
    }
}

fn render_continuation(lines: &mut Vec<RenderedLine>, text: &str) {
    for line_text in text.lines() {
        let spans = vec![
            (" ".repeat(NAME_WIDTH), LineStyle::default()),
            (
                " │ ".to_string(),
                LineStyle {
                    fg: Some(SEPARATOR_COLOR),
                    ..Default::default()
                },
            ),
            (
                line_text.to_string(),
                LineStyle {
                    dimmed: true,
                    ..Default::default()
                },
            ),
        ];

        lines.push(RenderedLine { spans });
    }
}

/// Process user message text to handle command-related XML tags.
/// Returns None if the message should be skipped entirely (e.g., empty local-command-stdout).
fn process_command_message(text: &str) -> Option<String> {
    let trimmed = text.trim();

    // Check for empty or whitespace-only local-command-stdout - skip these entirely
    if trimmed.starts_with("<local-command-stdout>") && trimmed.ends_with("</local-command-stdout>")
    {
        let tag_start = "<local-command-stdout>".len();
        let tag_end = trimmed.len() - "</local-command-stdout>".len();
        let inner = &trimmed[tag_start..tag_end];
        if inner.trim().is_empty() {
            return None;
        }
        // Non-empty local-command-stdout: show the content without the tags
        return Some(inner.trim().to_string());
    }

    // Check if this is a command message with <command-name> tag
    if let Some(start) = trimmed.find("<command-name>")
        && let Some(end) = trimmed.find("</command-name>")
    {
        let content_start = start + "<command-name>".len();
        if content_start < end {
            return Some(trimmed[content_start..end].to_string());
        }
    }

    Some(text.to_string())
}

/// Wrap text to fit within max_width, preserving existing line breaks
fn wrap_text(text: &str, max_width: usize) -> String {
    if max_width == 0 || text.is_empty() {
        return text.to_string();
    }

    let wrapped_lines: Vec<String> = text
        .lines()
        .flat_map(|line| {
            if line.is_empty() {
                vec![String::new()]
            } else {
                textwrap::wrap(line, max_width)
                    .into_iter()
                    .map(|cow| cow.into_owned())
                    .collect()
            }
        })
        .collect();

    wrapped_lines.join("\n")
}
