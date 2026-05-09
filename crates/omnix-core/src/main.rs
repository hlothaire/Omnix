use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use colored::Colorize;
use omnix_protocol::{CoreCommand, CoreEvent, PermissionMode};
use tokio::sync::mpsc;

use omnix_core::agent::AgentCore;
use omnix_core::config::AppConfig;
use omnix_core::permissions::PermissionEnforcer;
use omnix_core::prompt::SystemPromptBuilder;
use omnix_core::provider::{AnyProvider, LlamaCppProvider};
use omnix_core::provider::ollama::OllamaProvider;
use omnix_core::tools::{
    ToolRegistry, bash::Bash, file_edit::EditFile, file_read::ReadFile, file_write::WriteFile,
    glob::Glob, grep::Grep, list_dir::ListDir, memory::MemoryTool,
};

#[derive(Parser, Debug)]
#[command(name = "omnix")]
#[command(about = "Terminal agent harness powered by local LLMs")]
struct Cli {
    #[arg(short, long)]
    provider: Option<String>,

    #[arg(short, long)]
    model: Option<String>,

    #[arg(short = 'H', long)]
    host: Option<String>,

    #[arg(short = 'M', long)]
    mode: Option<String>,

    #[arg(long)]
    session_dir: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load config and merge with CLI args (CLI overrides config)
    let config = AppConfig::load().unwrap_or_default();

    let provider_kind = cli.provider.as_ref()
        .unwrap_or(&config.provider.kind)
        .clone();

    let model = cli.model.as_ref()
        .unwrap_or(&config.provider.model)
        .clone();

    if model.is_empty() {
        eprintln!("{} No model specified. Either pass --model <MODEL> or set it in ~/.omnix/settings.toml", "✗".red());
        std::process::exit(1);
    }

    let host = cli.host.as_ref()
        .unwrap_or(&config.provider.host)
        .clone();

    let permission_mode = parse_mode(
        cli.mode.as_ref()
            .unwrap_or(&config.permissions.mode)
    );

    let session_dir = AppConfig::expand_home(
        cli.session_dir.as_ref()
            .unwrap_or(&config.session.directory)
    )?;

    std::fs::create_dir_all(&session_dir)?;

    let memory_path = dirs::home_dir()
        .map(|h| h.join(".omnix").join("memory.md"))
        .unwrap_or_else(|| PathBuf::from(".omnix/memory.md"));
    if let Some(parent) = memory_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<CoreEvent>();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<CoreCommand>();

    let (provider, final_host) = match provider_kind.as_str() {
        "ollama" => {
            let ollama_host = if host == "http://localhost:8080" {
                "http://localhost:11434".to_string()
            } else {
                host.clone()
            };
            (
                AnyProvider::Ollama(OllamaProvider::new(&ollama_host, &model)),
                ollama_host,
            )
        }
        _ => (
            AnyProvider::LlamaCpp(LlamaCppProvider::new(&host, &model)),
            host.clone(),
        ),
    };

    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(Bash));
    tools.register(Arc::new(ReadFile));
    tools.register(Arc::new(WriteFile));
    tools.register(Arc::new(EditFile));
    tools.register(Arc::new(Glob));
    tools.register(Arc::new(Grep));
    tools.register(Arc::new(ListDir));
    tools.register(Arc::new(MemoryTool::new(&memory_path)));

    let permissions = PermissionEnforcer::new(permission_mode);

    let prompt_builder =
        SystemPromptBuilder::new(permission_mode).with_tools(tools.tool_definitions());

    // Spawn agent core
    let mut core = AgentCore::new(
        provider,
        model.clone(),
        tools,
        permissions,
        prompt_builder,
        memory_path.clone(),
        event_tx.clone(),
    );

    let core_handle = tokio::spawn(async move {
        core.run(cmd_rx).await;
    });

    println!(
        "{} {} — {}",
        "Omnix".bold().cyan(),
        env!("CARGO_PKG_VERSION").dimmed(),
        "Terminal Agent Harness".dimmed()
    );
    println!(
        "Provider: {} | Model: {} | Host: {} | Mode: {}\n",
        provider_kind.bright_cyan(),
        model.bright_green(),
        final_host.bright_blue(),
        format!("{:?}", permission_mode).bright_yellow()
    );

    let event_handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            render_event(&event);
        }
    });

    let cmd_tx_clone = cmd_tx.clone();
    let repl_handle = tokio::task::spawn_blocking(move || {
        run_repl(cmd_tx_clone);
    });

    let _ = repl_handle.await;

    let _ = cmd_tx.send(CoreCommand::Shutdown);
    let _ = core_handle.await;
    let _ = event_handle.await;

    println!("\n{} Goodbye.", "✓".green());
    Ok(())
}

fn parse_mode(s: &str) -> PermissionMode {
    match s.to_lowercase().as_str() {
        "readonly" | "read-only" | "ro" => PermissionMode::ReadOnly,
        "workspace-write" | "workspace" | "ww" => PermissionMode::WorkspaceWrite,
        "danger" | "danger-full-access" | "full" => PermissionMode::DangerFullAccess,
        "prompt" | "ask" => PermissionMode::Prompt,
        "allow" | "auto" => PermissionMode::Allow,
        _ => PermissionMode::WorkspaceWrite,
    }
}

fn run_repl(cmd_tx: mpsc::UnboundedSender<CoreCommand>) {
    let mut rl = rustyline::DefaultEditor::new().expect("Failed to create editor");

    loop {
        let readline = rl.readline("omnix> ");
        match readline {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(line);

                if line.starts_with('/') {
                    if !handle_slash_command(line, &cmd_tx) {
                        break;
                    }
                    continue;
                }

                if cmd_tx
                    .send(CoreCommand::SendPrompt { text: line.into() })
                    .is_err()
                {
                    eprintln!("{} Agent disconnected", "✗".red());
                    break;
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                println!("^C (use /quit to exit)");
                continue;
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                println!("EOF");
                break;
            }
            Err(err) => {
                eprintln!("{} Error: {}", "✗".red(), err);
                break;
            }
        }
    }
}

fn handle_slash_command(line: &str, cmd_tx: &mpsc::UnboundedSender<CoreCommand>) -> bool {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let cmd = parts.first().copied().unwrap_or("");
    let args = &parts[1..];

    match cmd {
        "/quit" | "/q" | "/exit" => {
            println!("Shutting down...");
            return false;
        }
        "/new" => {
            let _ = cmd_tx.send(CoreCommand::NewSession);
            println!("{} New session started", "✓".green());
        }
        "/save" => {
            let _ = cmd_tx.send(CoreCommand::SaveSession);
            println!("{} Session saved", "✓".green());
        }
        "/load" => {
            if let Some(id) = args.first() {
                let _ = cmd_tx.send(CoreCommand::LoadSession { id: id.to_string() });
                println!("{} Loading session {}", "⏳".yellow(), id);
            } else {
                eprintln!("{} Usage: /load <session-id>", "✗".red());
            }
        }
        "/mode" => {
            if let Some(mode_str) = args.first() {
                let mode = parse_mode(mode_str);
                let _ = cmd_tx.send(CoreCommand::SetPermissionMode { mode });
                println!("{} Mode set to {:?}", "✓".green(), mode);
            } else {
                eprintln!(
                    "{} Usage: /mode <readonly|workspace-write|danger|prompt|allow>",
                    "✗".red()
                );
            }
        }
        "/help" | "/h" | "/?" => {
            print_help();
        }
        _ => {
            eprintln!(
                "{} Unknown command: {}. Type /help for available commands.",
                "✗".red(),
                cmd
            );
        }
    }

    true
}

fn print_help() {
    println!("{}", "Available commands:".bold());
    println!("  /new          Start a new session");
    println!("  /save         Save current session");
    println!("  /load <id>    Load a saved session");
    println!(
        "  /mode <mode>  Set permission mode (readonly, workspace-write, danger, prompt, allow)"
    );
    println!("  /quit         Exit Omnix");
    println!("  /help         Show this help message");
    println!();
    println!("{}", "Permission modes:".bold());
    println!("  readonly        Only read operations (read_file, glob, list_dir)");
    println!("  workspace-write Read + write within workspace (default)");
    println!("  danger          Auto-approve everything (use with caution)");
    println!("  prompt          Ask for approval on every tool call");
    println!("  allow           Auto-approve all safe operations");
}

fn render_event(event: &CoreEvent) {
    match event {
        CoreEvent::TokenDelta { text } => {
            print!("{}", text);
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        CoreEvent::ToolCallStarted { id, name, input } => {
            println!();
            println!(
                "{} {} {}",
                "▶".bright_cyan(),
                name.bold(),
                format!("[{}]", id).dimmed()
            );
            let json = serde_json::to_string_pretty(input).unwrap_or_default();
            for line in json.lines() {
                println!("  {}", line.dimmed());
            }
        }
        CoreEvent::ApprovalRequested {
            call_id,
            tool_name,
            risk_level,
            description,
            ..
        } => {
            println!();
            let risk_color = match risk_level {
                omnix_protocol::RiskLevel::Informational => "ℹ".bright_blue(),
                omnix_protocol::RiskLevel::WorkspaceWrite => "⚠".bright_yellow(),
                omnix_protocol::RiskLevel::Destructive => "⚠".bright_red(),
            };
            println!(
                "{} {} approval required for {} [{}]",
                risk_color,
                "●".bright_yellow(),
                tool_name.bold(),
                call_id.dimmed()
            );
            println!("  {}", description.dimmed());
        }
        CoreEvent::ToolCallCompleted { id, output, .. } => {
            let prefix = if output.is_error {
                "✗".red()
            } else {
                "✓".green()
            };
            let content = if output.output.lines().count() > 5 {
                let lines: Vec<&str> = output.output.lines().take(5).collect();
                format!(
                    "{}\n  ... ({} more lines)",
                    lines.join("\n  "),
                    output.output.lines().count() - 5
                )
            } else {
                output.output.replace('\n', "\n  ")
            };
            println!(
                "{} {} {}",
                prefix,
                "Completed".dimmed(),
                format!("[{}]", id).dimmed()
            );
            println!("  {}", content);
        }
        CoreEvent::ToolError { call_id, message } => {
            println!(
                "{} Tool error [{}]: {}",
                "✗".red(),
                call_id.dimmed(),
                message
            );
        }
        CoreEvent::TurnEnded { usage, .. } => {
            println!();
            println!(
                "{} {} tokens in / {} tokens out",
                "⏹".dimmed(),
                usage.input_tokens.to_string().dimmed(),
                usage.output_tokens.to_string().dimmed()
            );
            println!();
        }
        CoreEvent::ApiError { message, retryable } => {
            let retry_str = if *retryable { " (retryable)" } else { "" };
            eprintln!("{} API error{}: {}", "✗".red(), retry_str, message);
        }
        CoreEvent::FatalError { message } => {
            eprintln!("{} Fatal: {}", "✗".red(), message);
        }
        CoreEvent::SessionCreated { id } => {
            println!("{} Session created: {}", "✓".green(), id.dimmed());
        }
        CoreEvent::SessionSaved { id } => {
            println!("{} Session saved: {}", "✓".green(), id.dimmed());
        }
        CoreEvent::SessionLoaded { id, .. } => {
            println!("{} Session loaded: {}", "✓".green(), id.dimmed());
        }
        _ => {}
    }
}
