use clap::{ArgAction, Parser};

#[derive(Parser, Debug)]
#[command(name = "claude-history")]
#[command(about = "View Claude conversation history with fuzzy search")]
pub struct Args {
    /// Hide tool calls from the output
    #[arg(
        long,
        short = 't',
        help = "Hide tool calls from the conversation output. Overrides config.",
        action = ArgAction::Count,
    )]
    pub no_tools: u8,

    /// Show the conversation directory and exit
    #[arg(
        long,
        short = 'd',
        help = "Print the conversation directory path and exit"
    )]
    pub show_dir: bool,

    /// Show last messages in preview instead of first messages
    #[arg(
        long,
        short = 'l',
        help = "Show the last messages in the fuzzy finder preview. Overrides config.",
        action = ArgAction::Count,
    )]
    pub last: u8,

    /// Show relative time (e.g. \"10 minutes ago\") instead of timestamp
    #[arg(
        long,
        short = 'r',
        help = "Display relative time instead of absolute timestamp. Overrides config.",
        action = ArgAction::Count,
    )]
    pub relative_time: u8,

    /// Resume the selected conversation in the Claude CLI
    #[arg(
        long,
        short = 'c',
        help = "Resume the selected conversation in Claude Code"
    )]
    pub resume: bool,
}
