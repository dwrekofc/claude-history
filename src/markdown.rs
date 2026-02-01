//! Markdown to ANSI text rendering
//!
//! Converts markdown text to ANSI-styled strings suitable for terminal output.
//! Handles text wrapping internally to preserve ledger alignment.

use colored::{ColoredString, Colorize};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Render markdown text to ANSI-styled string with line wrapping
pub fn render_markdown(input: &str, max_width: usize) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(input, options);
    let mut renderer = MarkdownRenderer::new(max_width);

    for event in parser {
        renderer.handle_event(event);
    }

    renderer.finish()
}

struct MarkdownRenderer {
    output: String,
    max_width: usize,
    style_stack: Vec<TextStyle>,
    list_stack: Vec<ListContext>,
    in_code_block: bool,
    pending_text: String,
    at_line_start: bool,
}

#[derive(Clone)]
struct ListContext {
    index: Option<u64>,
    depth: usize,
}

#[derive(Clone)]
enum TextStyle {
    Bold,
    Italic,
    Strikethrough,
    Quote,
    Link(String),
}

impl MarkdownRenderer {
    fn new(max_width: usize) -> Self {
        Self {
            output: String::new(),
            max_width,
            style_stack: vec![],
            list_stack: vec![],
            in_code_block: false,
            pending_text: String::new(),
            at_line_start: true,
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => self.inline_code(&code),
            Event::SoftBreak => self.soft_break(),
            Event::HardBreak => self.hard_break(),
            Event::Rule => self.rule(),
            _ => {}
        }
    }

    fn start_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                self.flush_pending();
                if !self.output.is_empty() && !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
            }
            Tag::Heading { level, .. } => {
                self.flush_pending();
                if !self.output.is_empty() && !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                let hashes = heading_level_to_usize(level);
                let prefix = "#".repeat(hashes);
                self.output
                    .push_str(&format!("{} ", prefix).cyan().bold().to_string());
            }
            Tag::CodeBlock(kind) => {
                self.flush_pending();
                self.in_code_block = true;
                if !self.output.is_empty() && !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                if !lang.is_empty() {
                    self.output
                        .push_str(&format!("```{}", lang).dimmed().to_string());
                } else {
                    self.output.push_str(&"```".dimmed().to_string());
                }
                self.output.push('\n');
            }
            Tag::List(start) => {
                self.flush_pending();
                let depth = self.list_stack.len();
                self.list_stack.push(ListContext {
                    index: start,
                    depth,
                });
            }
            Tag::Item => {
                self.flush_pending();
                if !self.output.is_empty() && !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                let indent = if let Some(ctx) = self.list_stack.last() {
                    "  ".repeat(ctx.depth)
                } else {
                    String::new()
                };
                if let Some(ctx) = self.list_stack.last_mut() {
                    match &mut ctx.index {
                        None => self.output.push_str(&format!("{}- ", indent)),
                        Some(n) => {
                            self.output
                                .push_str(&format!("{}{}. ", indent, n).dimmed().to_string());
                            *n += 1;
                        }
                    }
                }
                self.at_line_start = false;
            }
            Tag::Emphasis => self.style_stack.push(TextStyle::Italic),
            Tag::Strong => self.style_stack.push(TextStyle::Bold),
            Tag::Strikethrough => self.style_stack.push(TextStyle::Strikethrough),
            Tag::BlockQuote(_) => {
                self.flush_pending();
                if !self.output.is_empty() && !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.output.push_str(&"> ".green().to_string());
                self.style_stack.push(TextStyle::Quote);
            }
            Tag::Link { dest_url, .. } => {
                self.style_stack.push(TextStyle::Link(dest_url.to_string()));
            }
            _ => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush_pending();
                self.output.push('\n');
                self.at_line_start = true;
            }
            TagEnd::Heading(_) => {
                self.flush_pending();
                self.output.push('\n');
                self.at_line_start = true;
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                if !self.output.ends_with('\n') {
                    self.output.push('\n');
                }
                self.output.push_str(&"```".dimmed().to_string());
                self.output.push('\n');
                self.at_line_start = true;
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Item => {
                self.flush_pending();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.style_stack.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_pending();
                self.style_stack.pop();
            }
            TagEnd::Link => {
                if let Some(TextStyle::Link(url)) = self.style_stack.pop() {
                    self.pending_text
                        .push_str(&format!(" ({})", url).blue().underline().to_string());
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if self.in_code_block {
            // Code blocks: preserve formatting, apply code style per line
            for line in text.lines() {
                self.output.push_str(&line.on_bright_black().to_string());
                self.output.push('\n');
            }
            // Handle case where text doesn't end with newline
            if !text.ends_with('\n') && !text.is_empty() {
                // Remove the extra newline we added
                if self.output.ends_with('\n') {
                    self.output.pop();
                }
            }
        } else {
            // Regular text: accumulate for wrapping
            self.pending_text.push_str(text);
        }
    }

    fn inline_code(&mut self, code: &str) {
        // Inline code with background styling (no backticks - background distinguishes it)
        let styled = code.on_bright_black().to_string();
        self.pending_text.push_str(&styled);
    }

    fn soft_break(&mut self) {
        self.pending_text.push(' ');
    }

    fn hard_break(&mut self) {
        self.flush_pending();
        self.output.push('\n');
        self.at_line_start = true;
    }

    fn rule(&mut self) {
        self.flush_pending();
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.output
            .push_str(&"─".repeat(self.max_width.min(40)).dimmed().to_string());
        self.output.push('\n');
        self.at_line_start = true;
    }

    fn flush_pending(&mut self) {
        if self.pending_text.is_empty() {
            return;
        }

        let text = std::mem::take(&mut self.pending_text);

        // Wrap the plain text (stripping ANSI that may have been added by inline code)
        // Then apply styles to each line
        let wrapped = wrap_text_preserve_ansi(&text, self.max_width);

        for (i, line) in wrapped.iter().enumerate() {
            if i > 0 {
                self.output.push('\n');
                // Add list indent for continuation lines
                if let Some(ctx) = self.list_stack.last() {
                    let indent = "  ".repeat(ctx.depth);
                    self.output.push_str(&format!("{}  ", indent));
                }
            }
            let styled = apply_styles(line, &self.style_stack);
            self.output.push_str(&styled);
        }

        self.at_line_start = false;
    }

    fn finish(mut self) -> String {
        self.flush_pending();
        // Trim trailing whitespace but preserve structure
        while self.output.ends_with("\n\n") {
            self.output.pop();
        }
        self.output
    }
}

fn heading_level_to_usize(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn apply_styles(text: &str, styles: &[TextStyle]) -> String {
    if styles.is_empty() {
        return text.to_string();
    }

    let mut result: ColoredString = text.normal();

    for style in styles {
        result = match style {
            TextStyle::Bold => result.bold(),
            TextStyle::Italic => result.italic(),
            TextStyle::Strikethrough => result.strikethrough(),
            TextStyle::Quote => result.green(),
            TextStyle::Link(_) => result.blue().underline(),
        };
    }

    result.to_string()
}

/// Wrap text while preserving ANSI escape codes
/// This is a simplified approach: we strip ANSI for width calculation
fn wrap_text_preserve_ansi(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 || text.is_empty() {
        return vec![text.to_string()];
    }

    // Simple wrapping that respects word boundaries
    // ANSI codes are preserved but may cause slight width miscalculation
    textwrap::wrap(text, max_width)
        .into_iter()
        .map(|cow| cow.into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text() {
        let result = render_markdown("Hello world", 80);
        assert_eq!(result.trim(), "Hello world");
    }

    #[test]
    fn test_inline_code() {
        let result = render_markdown("Use `foo()` here", 80);
        assert!(result.contains("foo()"));
    }

    #[test]
    fn test_bold() {
        let result = render_markdown("This is **bold** text", 80);
        assert!(result.contains("bold"));
    }

    #[test]
    fn test_code_block() {
        let result = render_markdown("```rust\nlet x = 1;\n```", 80);
        assert!(result.contains("let x = 1;"));
        assert!(result.contains("```"));
    }

    #[test]
    fn test_list() {
        let result = render_markdown("- item 1\n- item 2", 80);
        assert!(result.contains("- item 1"));
        assert!(result.contains("- item 2"));
    }

    #[test]
    fn test_heading() {
        let result = render_markdown("# Heading", 80);
        assert!(result.contains("#"));
        assert!(result.contains("Heading"));
    }
}
