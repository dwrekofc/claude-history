//! Conversation export functionality.
//!
//! This module provides functions to export conversations in different formats:
//! - Ledger format (formatted text with speaker names)
//! - Plain text (simple speaker: message format)
//! - Markdown (with headers for speakers)
//! - JSONL (raw format)
//!
//! Conversations can be exported to files or copied to the clipboard.

use crate::claude::{ContentBlock, LogEntry, UserContent, UserMessage};
use arboard::Clipboard;
use chrono::Local;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Export format options
#[derive(Clone, Copy, Debug)]
pub enum ExportFormat {
    Ledger,
    Plain,
    Markdown,
    Jsonl,
}

impl ExportFormat {
    /// Get format from menu option index (0-3)
    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(ExportFormat::Ledger),
            1 => Some(ExportFormat::Plain),
            2 => Some(ExportFormat::Markdown),
            3 => Some(ExportFormat::Jsonl),
            _ => None,
        }
    }

    /// Get file extension for this format
    fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Ledger | ExportFormat::Plain => "txt",
            ExportFormat::Markdown => "md",
            ExportFormat::Jsonl => "jsonl",
        }
    }
}

/// Result of an export operation
pub struct ExportResult {
    pub success: bool,
    pub message: String,
}

/// Export conversation to file
pub fn export_to_file(source_path: &Path, format: ExportFormat) -> ExportResult {
    let timestamp = Local::now().format("%Y-%m-%d-%H%M%S");
    let ext = format.extension();
    let filename = format!("conversation-{}.{}", timestamp, ext);

    let content = match generate_content(source_path, format) {
        Ok(c) => c,
        Err(e) => {
            return ExportResult {
                success: false,
                message: format!("Failed to read: {}", e),
            };
        }
    };

    match fs::write(&filename, &content) {
        Ok(_) => ExportResult {
            success: true,
            message: format!("Exported to {}", filename),
        },
        Err(e) => ExportResult {
            success: false,
            message: format!("Failed to write: {}", e),
        },
    }
}

/// Copy conversation to clipboard
pub fn export_to_clipboard(source_path: &Path, format: ExportFormat) -> ExportResult {
    let content = match generate_content(source_path, format) {
        Ok(c) => c,
        Err(e) => {
            return ExportResult {
                success: false,
                message: format!("Failed to read: {}", e),
            };
        }
    };

    match Clipboard::new() {
        Ok(mut clipboard) => match clipboard.set_text(&content) {
            Ok(_) => ExportResult {
                success: true,
                message: "Copied to clipboard".to_string(),
            },
            Err(e) => ExportResult {
                success: false,
                message: format!("Clipboard error: {}", e),
            },
        },
        Err(e) => ExportResult {
            success: false,
            message: format!("Clipboard unavailable: {}", e),
        },
    }
}

/// Generate content in the specified format
fn generate_content(source_path: &Path, format: ExportFormat) -> std::io::Result<String> {
    match format {
        ExportFormat::Jsonl => fs::read_to_string(source_path),
        ExportFormat::Plain => generate_plain(source_path),
        ExportFormat::Markdown => generate_markdown(source_path),
        ExportFormat::Ledger => generate_ledger(source_path),
    }
}

/// Generate plain text format (simple "Speaker: message" lines)
fn generate_plain(path: &Path) -> std::io::Result<String> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut output = String::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
            match entry {
                LogEntry::User { message, .. } => {
                    if let Some(text) = extract_user_text(&message) {
                        output.push_str(&format!("You: {}\n\n", text));
                    }
                }
                LogEntry::Assistant { message, .. } => {
                    for block in &message.content {
                        if let ContentBlock::Text { text } = block {
                            output.push_str(&format!("Claude: {}\n\n", text));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(output)
}

/// Generate markdown format (with ## headers for speakers)
fn generate_markdown(path: &Path) -> std::io::Result<String> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut output = String::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
            match entry {
                LogEntry::User { message, .. } => {
                    if let Some(text) = extract_user_text(&message) {
                        output.push_str(&format!("## You\n\n{}\n\n", text));
                    }
                }
                LogEntry::Assistant { message, .. } => {
                    for block in &message.content {
                        if let ContentBlock::Text { text } = block {
                            output.push_str(&format!("## Claude\n\n{}\n\n", text));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(output)
}

/// Generate ledger-style format (formatted like the TUI viewer)
fn generate_ledger(path: &Path) -> std::io::Result<String> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut output = String::new();

    const NAME_WIDTH: usize = 9;

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
            match entry {
                LogEntry::User { message, .. } => {
                    if let Some(text) = extract_user_text(&message) {
                        append_ledger_block(&mut output, "You", &text, NAME_WIDTH);
                        output.push('\n');
                    }
                }
                LogEntry::Assistant { message, .. } => {
                    for block in &message.content {
                        if let ContentBlock::Text { text } = block {
                            append_ledger_block(&mut output, "Claude", text, NAME_WIDTH);
                            output.push('\n');
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(output)
}

/// Append a ledger-formatted block to the output
fn append_ledger_block(output: &mut String, speaker: &str, text: &str, name_width: usize) {
    for (i, line) in text.lines().enumerate() {
        if i == 0 {
            output.push_str(&format!(
                "{:>width$} │ {}\n",
                speaker,
                line,
                width = name_width
            ));
        } else {
            output.push_str(&format!("{:>width$} │ {}\n", "", line, width = name_width));
        }
    }
}

/// Extract text from a user message, handling command messages
fn extract_user_text(message: &UserMessage) -> Option<String> {
    match &message.content {
        UserContent::String(s) => process_command_text(s),
        UserContent::Blocks(blocks) => {
            for block in blocks {
                if let ContentBlock::Text { text } = block
                    && let Some(processed) = process_command_text(text) {
                        return Some(processed);
                    }
            }
            None
        }
    }
}

/// Process command message text, extracting content from XML tags
fn process_command_text(text: &str) -> Option<String> {
    let trimmed = text.trim();

    // Handle <local-command-stdout> tags
    if trimmed.starts_with("<local-command-stdout>") && trimmed.ends_with("</local-command-stdout>")
    {
        let inner = &trimmed
            ["<local-command-stdout>".len()..trimmed.len() - "</local-command-stdout>".len()];
        if inner.trim().is_empty() {
            return None;
        }
        return Some(inner.trim().to_string());
    }

    // Handle <command-name> tags
    if let Some(start) = trimmed.find("<command-name>")
        && let Some(end) = trimmed.find("</command-name>") {
            let content_start = start + "<command-name>".len();
            if content_start < end {
                return Some(trimmed[content_start..end].to_string());
            }
        }

    Some(text.to_string())
}
