use crate::claude::{AssistantMessage, ContentBlock, LogEntry, UserContent};
use crate::cli::DebugLevel;
use crate::debug;
use crate::debug_log;
use crate::error::Result;
use colored::*;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Process user message text to handle command-related XML tags
/// Returns None if the message should be skipped entirely (e.g., empty local-command-stdout)
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
            // Extract just the command name (e.g., "/clear")
            return Some(trimmed[content_start..end].to_string());
        }
    }

    // Return original text for non-command messages
    Some(text.to_string())
}

/// Display a conversation from a file
pub fn display_conversation(
    file_path: &Path,
    no_tools: bool,
    show_thinking: bool,
    debug_level: Option<DebugLevel>,
) -> Result<()> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    for (line_number, line_result) in reader.lines().enumerate() {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<LogEntry>(&line) {
            Ok(entry) => {
                display_entry(&entry, no_tools, show_thinking);
            }
            Err(e) => {
                debug::error(
                    debug_level,
                    &format!("Failed to parse line {}: {}", line_number + 1, e),
                );
                if debug_level.is_some() {
                    let _ = debug_log::log_display_error(
                        file_path,
                        line_number + 1,
                        &e.to_string(),
                        &line,
                    );
                }
            }
        }
    }

    Ok(())
}

fn display_entry(entry: &LogEntry, no_tools: bool, show_thinking: bool) {
    match entry {
        LogEntry::Summary { .. }
        | LogEntry::FileHistorySnapshot { .. }
        | LogEntry::System { .. }
        | LogEntry::Progress { .. } => {
            // Skip metadata entries (summary, file history snapshot, system, progress)
        }
        LogEntry::User { message, .. } => match &message.content {
            UserContent::String(text) => {
                if let Some(processed) = process_command_message(text) {
                    println!("{} {}", "User:".blue().bold(), processed);
                }
            }
            UserContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => {
                            if let Some(processed) = process_command_message(text) {
                                println!("{} {}", "User:".blue().bold(), processed);
                            }
                        }
                        ContentBlock::ToolResult { content, .. } => {
                            if !no_tools {
                                println!("{}", "<Tool Result>".cyan().bold());
                                if let Some(content_value) = content {
                                    // Tool result content can be a string or an array of content blocks
                                    if let Some(result_str) = content_value.as_str() {
                                        // If it's a simple string, display it directly
                                        println!("{}", result_str.dimmed());
                                    } else if let Ok(formatted_result) =
                                        serde_json::to_string_pretty(content_value)
                                    {
                                        // Otherwise, pretty-print the JSON
                                        println!("{}", formatted_result.dimmed());
                                    }
                                } else {
                                    println!("{}", "<no tool result content>".dimmed());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        },
        LogEntry::Assistant { message, .. } => {
            display_assistant_message(message, no_tools, show_thinking);
        }
    }
}

/// Helper struct to categorize assistant message content
struct FormattedMessage<'a> {
    text_blocks: Vec<&'a str>,
    tool_calls: Vec<(&'a str, &'a serde_json::Value)>,
    thinking_steps: Vec<&'a str>,
}

impl<'a> From<&'a AssistantMessage> for FormattedMessage<'a> {
    fn from(msg: &'a AssistantMessage) -> Self {
        let mut text_blocks = Vec::new();
        let mut tool_calls = Vec::new();
        let mut thinking_steps = Vec::new();

        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => text_blocks.push(text.as_str()),
                ContentBlock::ToolUse { name, input, .. } => {
                    tool_calls.push((name.as_str(), input))
                }
                ContentBlock::Thinking { thinking, .. } => thinking_steps.push(thinking.as_str()),
                _ => {}
            }
        }

        Self {
            text_blocks,
            tool_calls,
            thinking_steps,
        }
    }
}

fn display_assistant_message(message: &AssistantMessage, no_tools: bool, show_thinking: bool) {
    let formatted = FormattedMessage::from(message);

    for text in formatted.text_blocks {
        println!("{} {}", "Assistant:".green().bold(), text);
    }

    if !no_tools {
        for (tool_name, tool_input) in formatted.tool_calls {
            println!(
                "{} <Calling Tool: {}>",
                "Assistant:".green().bold(),
                tool_name
            );
            if let Ok(formatted_input) = serde_json::to_string_pretty(tool_input) {
                println!("{}", formatted_input.dimmed());
            }
        }
    }

    if show_thinking {
        for thought in formatted.thinking_steps {
            println!("{} {}", "Thinking:".yellow().bold(), thought);
        }
    }
}
