mod claude;
mod cli;
mod codex;
mod config;
mod debug;
mod debug_log;
mod display;
mod error;
mod history;
mod markdown;
mod pager;
mod syntax;
mod tool_format;
mod tui;
mod update;

use clap::Parser;
use cli::{Args, Commands};
use error::{AppError, Result};
use std::cmp::Reverse;
use std::fs::File;
use std::io::{BufRead, BufReader, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

fn main() {
    if let Err(e) = run() {
        match e {
            AppError::SelectionCancelled => {
                // User cancelled, exit silently
                std::process::exit(0);
            }
            _ => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

/// Helper function to resolve a boolean setting by merging CLI flags and config values.
///
/// Priority: enable_flag > disable_flag > config_value > default_value
fn resolve_bool_setting(
    enable_flag: bool,
    disable_flag: bool,
    config_value: Option<bool>,
    default_value: bool,
) -> bool {
    if enable_flag {
        true
    } else if disable_flag {
        false
    } else {
        config_value.unwrap_or(default_value)
    }
}

fn run() -> Result<()> {
    let args = Args::parse();

    // Handle subcommands
    if let Some(command) = args.command {
        return match command {
            Commands::Update => update::run(),
            Commands::Export { provider } => match provider {
                cli::ExportProvider::Claude(export_args) => {
                    export_project_markdown(&export_args.project_dir, &export_args.output_dir)
                }
                cli::ExportProvider::Codex(export_args) => codex::export_project_markdown(
                    &export_args.project_dir,
                    &export_args.output_dir,
                ),
            },
            Commands::ExportProjectMarkdown(export_args) => {
                export_project_markdown(&export_args.project_dir, &export_args.output_dir)
            }
        };
    }

    // Detect terminal theme before entering raw mode / alternate screen,
    // as terminal_light queries the terminal for background color
    tui::theme::detect_theme();

    let config = config::load_config()?;

    // Merge CLI arguments with config file settings. CLI takes precedence.
    let display_config = config.display.unwrap_or_default();

    // Extract resume config
    let resume_config = config.resume.unwrap_or_default();
    let default_args = resume_config.default_args.as_deref().unwrap_or(&[]);

    // Disable colors globally when --no-color is passed
    if args.no_color {
        colored::control::set_override(false);
    }

    // Resolve keybindings
    let keys = config::KeyBindings::from_config(config.keys);

    // Use positive names internally for clarity
    let show_tools = resolve_bool_setting(
        args.show_tools,
        args.no_tools,
        display_config.no_tools.map(|b| !b),
        false, // Default: hide tools
    );
    // Map CLI flag to ToolDisplayMode
    // --show-tools → Full, --no-tools → Hidden, default → Truncated
    let tool_display = if args.show_tools {
        tui::ToolDisplayMode::Full
    } else if args.no_tools {
        tui::ToolDisplayMode::Hidden
    } else {
        match display_config.no_tools {
            Some(true) => tui::ToolDisplayMode::Hidden,
            Some(false) => tui::ToolDisplayMode::Full,
            None => tui::ToolDisplayMode::Truncated,
        }
    };
    let show_last = resolve_bool_setting(args.last, args.first, display_config.last, true);
    let show_thinking = resolve_bool_setting(
        args.show_thinking,
        args.hide_thinking,
        display_config.show_thinking,
        false,
    );
    let plain_mode = resolve_bool_setting(args.plain, false, display_config.plain, false);
    let use_pager = resolve_bool_setting(
        args.pager,
        args.no_pager,
        display_config.pager,
        std::io::stdout().is_terminal(),
    );

    // Handle --delete flag: delete a session by UUID and exit
    if let Some(ref session_id) = args.delete {
        match history::delete_session_by_uuid(session_id) {
            Ok(count) => {
                if count == 1 {
                    eprintln!("Deleted session {}", session_id);
                } else {
                    eprintln!(
                        "Deleted session {} ({} copies across projects)",
                        session_id, count
                    );
                }
                return Ok(());
            }
            Err(e) => return Err(e),
        }
    }

    // Handle --debug-search flag: debug search result scoring
    if let Some(ref query) = args.debug_search {
        let mut conversations = history::load_all_conversations(show_last, args.debug)?;
        conversations.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        let searchable = tui::search::precompute_search_text(&conversations);
        let now = chrono::Local::now();

        let query_lower = tui::search::normalize_for_search(query);
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let adjacent_pairs: Vec<String> = if query_words.len() > 1 {
            query_words
                .windows(2)
                .map(|w| format!("{} {}", w[0], w[1]))
                .collect()
        } else {
            vec![]
        };

        // Optionally filter to local workspace
        let current_project_dir_name = if args.local {
            std::env::current_dir()
                .ok()
                .map(|d| history::convert_path_to_project_dir_name(&d))
        } else {
            None
        };

        let mut results: Vec<_> = searchable
            .iter()
            .filter_map(|s| {
                if let Some(ref proj) = current_project_dir_name {
                    let conv = &conversations[s.index];
                    let matches =
                        conv.path
                            .parent()
                            .and_then(|p| p.file_name())
                            .is_some_and(|name| {
                                history::is_same_project(&name.to_string_lossy(), proj)
                            });
                    if !matches {
                        return None;
                    }
                }

                let debug = tui::search::score_text_debug(
                    s,
                    &conversations[s.index].search_text_lower,
                    &query_words,
                    &adjacent_pairs,
                    conversations[s.index].timestamp,
                    now,
                )?;
                Some((s.index, debug))
            })
            .collect();

        results.sort_by(|a, b| {
            b.1.total
                .partial_cmp(&a.1.total)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for (rank, (idx, debug)) in results.iter().take(30).enumerate() {
            let conv = &conversations[*idx];
            let age = now.signed_duration_since(conv.timestamp);
            let project = conv.project_name.as_deref().unwrap_or("(none)");
            let age_str = format_debug_age(age);
            let session = conv
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("?");
            eprintln!(
                "#{:2} score={:.2} freshness={:.2} | {} | {} | {} ago",
                rank + 1,
                debug.total,
                debug.freshness,
                project,
                session,
                age_str
            );

            for field in &debug.fields {
                if field.tf_score > 0.0 || field.adjacency_score > 0.0 {
                    eprintln!(
                        "     {}: tf={:.2} adj={:.2} (w={:.1})",
                        field.name, field.tf_score, field.adjacency_score, field.weight
                    );
                    for (word, tf, ln_score) in &field.word_details {
                        if *tf > 0 {
                            eprintln!("       \"{}\" tf={} ln={:.2}", word, tf, ln_score);
                        }
                    }
                }
            }
            eprintln!();
        }

        return Ok(());
    }

    // Handle --render flag: render a JSONL file in ledger format and exit
    if let Some(ref render_path) = args.render {
        let display_options = display::DisplayOptions {
            no_tools: !show_tools,
            show_thinking,
            debug_level: args.debug,
            use_pager,
            no_color: args.no_color,
        };
        return display::render_to_terminal(render_path, &display_options);
    }

    // Handle direct file input mode
    if let Some(ref input_file) = args.input_file {
        if !input_file.exists() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", input_file.display()),
            )));
        }
        if !input_file.is_file() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Not a file: {}", input_file.display()),
            )));
        }
        tui::run_single_file(input_file.clone(), tool_display, show_thinking, keys)?;
        return Ok(());
    }

    let use_local = args.local;

    // Determine the current workspace's project directory name (for workspace filter)
    let current_dir = std::env::current_dir().ok();
    let current_project_dir_name = current_dir
        .as_ref()
        .map(|d| history::convert_path_to_project_dir_name(d));

    // Handle --show-dir flag (needs current_dir)
    if args.show_dir {
        if let Some(ref dir) = current_dir {
            let projects_dir = history::get_claude_projects_dir(dir)?;
            println!("{}", projects_dir.display());
            return Ok(());
        } else {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Failed to get current directory",
            )));
        }
    }

    // --local starts with workspace filter on; default is global (filter off)
    let workspace_filter = use_local;

    // Always use streaming global loader for all conversations
    let rx = history::load_all_conversations_streaming(show_last, args.debug);

    let (conversations, selected_path) = match tui::run_with_loader(
        rx,
        tool_display,
        show_thinking,
        keys,
        workspace_filter,
        current_project_dir_name,
    )? {
        (tui::Action::Select(path), convs) => (convs, path),
        (tui::Action::Resume(path), convs) => {
            let conv = convs.iter().find(|c| c.path == path);
            let project_path = conv.and_then(|c| c.project_path.as_ref());
            resume_with_claude(&path, project_path, default_args, false)?;
            return Ok(());
        }
        (tui::Action::ForkResume(path), convs) => {
            let conv = convs.iter().find(|c| c.path == path);
            let project_path = conv.and_then(|c| c.project_path.as_ref());
            resume_with_claude(&path, project_path, default_args, true)?;
            return Ok(());
        }
        (tui::Action::Quit, _) => return Err(AppError::SelectionCancelled),
        (tui::Action::Delete(_), _) => unreachable!("Delete is handled internally"),
    };

    if args.show_path {
        println!("{}", selected_path.display());
        return Ok(());
    }

    if args.show_id {
        let conversation_id = selected_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| {
                AppError::ClaudeExecutionError(
                    "Conversation filename is not valid Unicode".to_string(),
                )
            })?;
        println!("{}", conversation_id);
        return Ok(());
    }

    if args.resume {
        // Find the selected conversation to get its project_path
        let conv = conversations.iter().find(|c| c.path == selected_path);
        debug::debug(
            args.debug,
            &format!("Selected path: {}", selected_path.display()),
        );
        debug::debug(
            args.debug,
            &format!("Found conversation: {}", conv.is_some()),
        );
        if let Some(c) = conv {
            debug::debug(args.debug, &format!("project_path: {:?}", c.project_path));
            if let Some(p) = &c.project_path {
                debug::debug(args.debug, &format!("project_path exists: {}", p.exists()));
            }
        }
        let project_path = conv.and_then(|c| c.project_path.as_ref());
        resume_with_claude(
            &selected_path,
            project_path,
            default_args,
            args.fork_session,
        )?;
        return Ok(());
    }

    // Log parse errors to debug log if debug mode is enabled
    if args.debug.is_some()
        && let Some(conv) = conversations.iter().find(|c| c.path == selected_path)
    {
        if let Err(e) = debug_log::log_parse_errors(conv) {
            debug::warn(
                args.debug,
                &format!("Failed to write parse errors to log: {}", e),
            );
        } else if !conv.parse_errors.is_empty() {
            debug::info(
                args.debug,
                &format!(
                    "Logged {} parse error(s) to ~/.local/state/claude-history/debug.log",
                    conv.parse_errors.len()
                ),
            );
        }
    }

    // Display the selected conversation
    let display_options = display::DisplayOptions {
        no_tools: !show_tools,
        show_thinking,
        debug_level: args.debug,
        use_pager,
        no_color: args.no_color,
    };

    if plain_mode {
        display::display_conversation_plain(&selected_path, &display_options)?;
    } else {
        display::display_conversation(&selected_path, &display_options)?;
    }

    Ok(())
}

fn export_project_markdown(project_dir: &Path, output_dir: &Path) -> Result<()> {
    let project_dir = project_dir.canonicalize().map_err(AppError::Io)?;
    let claude_project_dir = history::get_claude_projects_dir(&project_dir)?;
    if !claude_project_dir.exists() {
        return Err(AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "Claude project history not found: {}",
                claude_project_dir.display()
            ),
        )));
    }

    std::fs::create_dir_all(output_dir).map_err(AppError::Io)?;

    let mut files = Vec::new();
    for entry in std::fs::read_dir(&claude_project_dir).map_err(AppError::Io)? {
        let entry = entry.map_err(AppError::Io)?;
        let path = entry.path();
        let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
            && !filename.starts_with("agent-")
        {
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            files.push((path, modified));
        }
    }
    let options = tui::export::ExportOptions {
        show_tools: false,
        show_thinking: false,
    };

    let mut candidates = Vec::new();
    for (path, modified) in files {
        let conversation = history::process_conversation_file(path.clone(), Some(modified), None)?;
        let activity_timestamp = last_claude_activity_timestamp(&path, modified)?;
        candidates.push(ClaudeExportCandidate {
            path,
            conversation,
            activity_timestamp,
        });
    }
    candidates.sort_by_key(|candidate| Reverse(candidate.activity_timestamp));

    let mut written = 0usize;
    for (idx, candidate) in candidates.iter().enumerate() {
        let content = tui::export::generate_content(
            &candidate.path,
            tui::export::ExportFormat::Markdown,
            options,
        )
        .map_err(AppError::Io)?;
        if content.trim().is_empty() {
            continue;
        }

        let stem = candidate
            .path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("session");
        let title = candidate
            .conversation
            .as_ref()
            .and_then(|conversation| {
                conversation
                    .custom_title
                    .as_deref()
                    .or(conversation.summary.as_deref())
                    .or(Some(conversation.preview.as_str()))
            })
            .unwrap_or(stem);
        let date = candidate
            .activity_timestamp
            .format("%Y-%m-%d-%H%M%S")
            .to_string();
        let filename = format!("{:03}-{}-{}.md", idx + 1, date, sanitize_filename(title));
        let output_path = output_dir.join(filename);
        std::fs::write(&output_path, content).map_err(AppError::Io)?;
        println!("{}", output_path.display());
        written += 1;
    }

    eprintln!(
        "Exported {} Markdown files from {}",
        written,
        claude_project_dir.display()
    );
    Ok(())
}

struct ClaudeExportCandidate {
    path: PathBuf,
    conversation: Option<history::Conversation>,
    activity_timestamp: chrono::DateTime<chrono::Local>,
}

fn last_claude_activity_timestamp(
    path: &Path,
    fallback_modified: SystemTime,
) -> Result<chrono::DateTime<chrono::Local>> {
    let file = File::open(path).map_err(AppError::Io)?;
    let reader = BufReader::new(file);
    let mut last_timestamp = None;

    for line in reader.lines() {
        let line = line.map_err(AppError::Io)?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<claude::LogEntry>(&line) else {
            continue;
        };
        let timestamp = match entry {
            claude::LogEntry::User { timestamp, .. }
            | claude::LogEntry::Assistant { timestamp, .. } => timestamp,
            _ => None,
        };
        if let Some(timestamp) = timestamp
            && let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&timestamp)
        {
            last_timestamp = Some(parsed.with_timezone(&chrono::Local));
        }
    }

    Ok(
        last_timestamp
            .unwrap_or_else(|| chrono::DateTime::<chrono::Local>::from(fallback_modified)),
    )
}

pub(crate) fn sanitize_filename(input: &str) -> String {
    let mut output = String::new();
    let mut previous_dash = false;
    for ch in input.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            output.push(lower);
            previous_dash = false;
        } else if !previous_dash {
            output.push('-');
            previous_dash = true;
        }
        if output.len() >= 80 {
            break;
        }
    }
    let trimmed = output.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "conversation".to_string()
    } else {
        trimmed
    }
}

fn resume_with_claude(
    selected_path: &Path,
    project_path: Option<&PathBuf>,
    default_args: &[String],
    fork_session: bool,
) -> Result<()> {
    let conversation_id = selected_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| {
            AppError::ClaudeExecutionError("Conversation filename is not valid Unicode".to_string())
        })?
        .to_owned();

    let project_dir = project_path.filter(|p| p.exists() && p.is_dir());

    let cwd = std::env::current_dir().map_err(|e| {
        AppError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Failed to get current directory: {}", e),
        ))
    })?;

    let conv_projects_dir = selected_path.parent().ok_or_else(|| {
        AppError::ClaudeExecutionError(
            "Cannot determine conversation's project directory".to_string(),
        )
    })?;

    // When the original project directory is gone (e.g. deleted worktree) or when
    // forking cross-project, copy session files to CWD's project directory and
    // resume from there.
    let needs_copy = if project_dir.is_none() {
        true
    } else if fork_session {
        let cwd_projects_dir = history::get_claude_projects_dir(&cwd)?;
        cwd_projects_dir != conv_projects_dir
    } else {
        false
    };

    if needs_copy {
        let cwd_projects_dir = history::get_claude_projects_dir(&cwd)?;
        std::fs::create_dir_all(&cwd_projects_dir).map_err(AppError::Io)?;
        copy_session_files(
            selected_path,
            &conversation_id,
            conv_projects_dir,
            &cwd_projects_dir,
        )?;

        let mut command = Command::new("claude");
        command.args(["--resume", &conversation_id]);
        command.args(default_args);
        command.current_dir(&cwd);
        return run_claude_command(command);
    }

    let mut command = Command::new("claude");
    command.args(["--resume", &conversation_id]);
    if fork_session {
        command.arg("--fork-session");
    }
    command.args(default_args);
    command.current_dir(project_dir.unwrap());

    run_claude_command(command)
}

/// Copy session files from one project directory to another for cross-project forking.
///
/// Copies:
/// 1. The .jsonl transcript file
/// 2. The session subdirectory (tool-results/, subagents/) if it exists
/// 3. The file-history directory for undo support if it exists
fn copy_session_files(
    jsonl_path: &Path,
    session_id: &str,
    source_projects_dir: &Path,
    target_projects_dir: &Path,
) -> Result<()> {
    // 1. Copy the .jsonl file
    let target_jsonl = target_projects_dir.join(jsonl_path.file_name().unwrap());
    std::fs::copy(jsonl_path, &target_jsonl).map_err(AppError::Io)?;

    // 2. Copy the session subdirectory (tool-results/, subagents/)
    let session_dir = source_projects_dir.join(session_id);
    if session_dir.is_dir() {
        let target_session_dir = target_projects_dir.join(session_id);
        copy_dir_recursive(&session_dir, &target_session_dir)?;
    }

    // Note: file-history (~/.claude/file-history/<uuid>/) is global, not per-project.
    // Claude Code finds it by session ID, so no copy needed.

    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(AppError::Io)?;
    for entry in std::fs::read_dir(src).map_err(AppError::Io)? {
        let entry = entry.map_err(AppError::Io)?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(AppError::Io)?;
        }
    }
    Ok(())
}

fn format_debug_age(age: chrono::Duration) -> String {
    let hours = age.num_hours();
    if hours < 24 {
        format!("{}h", hours)
    } else {
        format!("{}d", hours / 24)
    }
}

#[cfg(unix)]
fn run_claude_command(mut command: Command) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let err = command.exec();
    Err(AppError::ClaudeExecutionError(err.to_string()))
}

#[cfg(not(unix))]
fn run_claude_command(mut command: Command) -> Result<()> {
    let status = command
        .status()
        .map_err(|e| AppError::ClaudeExecutionError(e.to_string()))?;

    if !status.success() {
        return Err(AppError::ClaudeExecutionError(format!(
            "claude CLI exited with status {}",
            status
        )));
    }

    Ok(())
}
