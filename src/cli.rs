use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "claude-history")]
#[command(about = "View Claude conversation history with fuzzy search")]
pub struct Args {
    /// Hide tool calls from the output
    #[arg(long, help = "Hide tool calls from the conversation output")]
    pub no_tools: bool,

    /// Show the conversation directory and exit
    #[arg(long, help = "Print the conversation directory path and exit")]
    pub show_dir: bool,

    /// Show last messages in preview instead of first messages
    #[arg(long, help = "Show the last messages in the fuzzy finder preview")]
    pub last: bool,
}
