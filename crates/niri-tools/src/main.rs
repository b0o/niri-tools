use std::os::unix::net::UnixStream;
use std::time::Duration;

use clap::{Parser, Subcommand};
use niri_tools_common::protocol::{Command, Response};

#[derive(Parser, Debug)]
#[command(name = "niri-tools", about = "Niri window manager tools")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    /// Manage the daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Manage scratchpad windows
    Scratchpad {
        #[command(subcommand)]
        command: ScratchpadCommand,
    },
}

#[derive(Subcommand, Debug, PartialEq)]
enum DaemonCommand {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Restart the daemon
    Restart,
    /// Show daemon status
    Status,
}

#[derive(Subcommand, Debug, PartialEq)]
enum ScratchpadCommand {
    /// Toggle a scratchpad (smart toggle if no name given)
    Toggle {
        /// Scratchpad name (smart toggle if omitted)
        name: Option<String>,
    },
    /// Hide the focused scratchpad
    Hide,
    /// Toggle floating/tiled state
    ToggleFloat {
        /// Scratchpad name
        name: Option<String>,
    },
    /// Float a scratchpad
    Float {
        /// Scratchpad name
        name: Option<String>,
    },
    /// Tile a scratchpad
    Tile {
        /// Scratchpad name
        name: Option<String>,
    },
}

/// Convert CLI arguments to a protocol [`Command`].
///
/// Returns `None` for `DaemonCommand::Start`, which is handled separately
/// (it spawns the daemon rather than sending a socket message).
fn build_command(cli: &Cli) -> Option<Command> {
    match &cli.command {
        Commands::Daemon { command } => match command {
            DaemonCommand::Stop => Some(Command::DaemonStop),
            DaemonCommand::Restart => Some(Command::DaemonRestart),
            DaemonCommand::Status => Some(Command::DaemonStatus),
            DaemonCommand::Start => None,
        },
        Commands::Scratchpad { command } => match command {
            ScratchpadCommand::Toggle { name } => Some(Command::Toggle { name: name.clone() }),
            ScratchpadCommand::Hide => Some(Command::Hide),
            ScratchpadCommand::ToggleFloat { name } => {
                Some(Command::ToggleFloat { name: name.clone() })
            }
            ScratchpadCommand::Float { name } => Some(Command::Float { name: name.clone() }),
            ScratchpadCommand::Tile { name } => Some(Command::Tile { name: name.clone() }),
        },
    }
}

/// Send a command to the daemon over the Unix socket and return the response.
fn send_command(command: &Command) -> anyhow::Result<Response> {
    let socket_path = niri_tools_common::paths::socket_path();
    let mut stream = UnixStream::connect(&socket_path)?;
    niri_tools_common::protocol::write_message(&mut stream, command)?;
    let response: Response = niri_tools_common::protocol::read_message(&mut stream)?;
    Ok(response)
}

/// Maximum number of connection attempts after spawning the daemon.
const MAX_CONNECT_ATTEMPTS: usize = 10;

/// Delay between connection retry attempts.
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(100);

/// Send a command to the daemon, auto-starting it if not running.
fn send_command_with_autostart(command: &Command) -> anyhow::Result<Response> {
    match send_command(command) {
        Ok(response) => Ok(response),
        Err(_) => {
            eprintln!("Daemon not running, starting...");
            spawn_daemon()?;

            for _ in 0..MAX_CONNECT_ATTEMPTS {
                std::thread::sleep(CONNECT_RETRY_DELAY);
                if let Ok(response) = send_command(command) {
                    return Ok(response);
                }
            }
            anyhow::bail!(
                "Daemon failed to start after {:.1}s",
                MAX_CONNECT_ATTEMPTS as f64 * CONNECT_RETRY_DELAY.as_secs_f64()
            );
        }
    }
}

/// Spawn the daemon process via `niri msg action spawn`.
fn spawn_daemon() -> anyhow::Result<()> {
    let status = std::process::Command::new("niri")
        .args(["msg", "action", "spawn", "--", "niri-tools-daemon"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .status()?;
    if !status.success() {
        anyhow::bail!("Failed to spawn daemon via niri");
    }
    Ok(())
}

/// Handle the `daemon start` command.
fn start_daemon() -> anyhow::Result<()> {
    // Check if already running by attempting a status query
    if send_command(&Command::DaemonStatus).is_ok() {
        eprintln!("Daemon is already running");
        return Ok(());
    }
    spawn_daemon()?;

    // Verify the daemon actually started
    for _ in 0..MAX_CONNECT_ATTEMPTS {
        std::thread::sleep(CONNECT_RETRY_DELAY);
        if send_command(&Command::DaemonStatus).is_ok() {
            eprintln!("Daemon started");
            return Ok(());
        }
    }
    anyhow::bail!(
        "Daemon failed to start after {:.1}s",
        MAX_CONNECT_ATTEMPTS as f64 * CONNECT_RETRY_DELAY.as_secs_f64()
    );
}

/// Print a daemon status response.
fn print_status(pid: u32, cmdline: &str, ppid: u32, parent_cmdline: &str, socket: &str) {
    println!("Daemon is running");
    println!("  PID:    {pid} ({cmdline})");
    println!("  Parent: {ppid} ({parent_cmdline})");
    println!("  Socket: {socket}");
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Handle daemon start separately — it spawns the daemon rather than
    // sending a command over the socket.
    if let Commands::Daemon {
        command: DaemonCommand::Start,
    } = &cli.command
    {
        return start_daemon();
    }

    let command = build_command(&cli).expect("unhandled command variant");
    let response = send_command_with_autostart(&command)?;

    match response {
        Response::Ok => {}
        Response::Status {
            pid,
            cmdline,
            ppid,
            parent_cmdline,
            socket,
        } => {
            print_status(pid, &cmdline, ppid, &parent_cmdline, &socket);
        }
        Response::Error(msg) => {
            eprintln!("Error: {msg}");
            std::process::exit(1);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CLI argument parsing tests ──────────────────────────────────────

    #[test]
    fn parse_daemon_start() {
        let cli = Cli::try_parse_from(["niri-tools", "daemon", "start"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Daemon {
                command: DaemonCommand::Start
            }
        );
    }

    #[test]
    fn parse_daemon_stop() {
        let cli = Cli::try_parse_from(["niri-tools", "daemon", "stop"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Daemon {
                command: DaemonCommand::Stop
            }
        );
    }

    #[test]
    fn parse_daemon_restart() {
        let cli = Cli::try_parse_from(["niri-tools", "daemon", "restart"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Daemon {
                command: DaemonCommand::Restart
            }
        );
    }

    #[test]
    fn parse_daemon_status() {
        let cli = Cli::try_parse_from(["niri-tools", "daemon", "status"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Daemon {
                command: DaemonCommand::Status
            }
        );
    }

    #[test]
    fn parse_scratchpad_toggle_with_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "toggle", "term"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Scratchpad {
                command: ScratchpadCommand::Toggle {
                    name: Some("term".to_string())
                }
            }
        );
    }

    #[test]
    fn parse_scratchpad_toggle_without_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "toggle"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Scratchpad {
                command: ScratchpadCommand::Toggle { name: None }
            }
        );
    }

    #[test]
    fn parse_scratchpad_hide() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "hide"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Scratchpad {
                command: ScratchpadCommand::Hide
            }
        );
    }

    #[test]
    fn parse_scratchpad_toggle_float_with_name() {
        let cli =
            Cli::try_parse_from(["niri-tools", "scratchpad", "toggle-float", "browser"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Scratchpad {
                command: ScratchpadCommand::ToggleFloat {
                    name: Some("browser".to_string())
                }
            }
        );
    }

    #[test]
    fn parse_scratchpad_toggle_float_without_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "toggle-float"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Scratchpad {
                command: ScratchpadCommand::ToggleFloat { name: None }
            }
        );
    }

    #[test]
    fn parse_scratchpad_float_with_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "float", "editor"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Scratchpad {
                command: ScratchpadCommand::Float {
                    name: Some("editor".to_string())
                }
            }
        );
    }

    #[test]
    fn parse_scratchpad_float_without_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "float"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Scratchpad {
                command: ScratchpadCommand::Float { name: None }
            }
        );
    }

    #[test]
    fn parse_scratchpad_tile_with_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "tile", "music"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Scratchpad {
                command: ScratchpadCommand::Tile {
                    name: Some("music".to_string())
                }
            }
        );
    }

    #[test]
    fn parse_scratchpad_tile_without_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "tile"]).unwrap();
        assert_eq!(
            cli.command,
            Commands::Scratchpad {
                command: ScratchpadCommand::Tile { name: None }
            }
        );
    }

    #[test]
    fn parse_no_args_is_error() {
        assert!(Cli::try_parse_from(["niri-tools"]).is_err());
    }

    #[test]
    fn parse_invalid_subcommand_is_error() {
        assert!(Cli::try_parse_from(["niri-tools", "bogus"]).is_err());
    }

    #[test]
    fn parse_daemon_missing_subcommand_is_error() {
        assert!(Cli::try_parse_from(["niri-tools", "daemon"]).is_err());
    }

    #[test]
    fn parse_scratchpad_missing_subcommand_is_error() {
        assert!(Cli::try_parse_from(["niri-tools", "scratchpad"]).is_err());
    }

    // ── Command construction tests ──────────────────────────────────────

    #[test]
    fn build_command_daemon_start_returns_none() {
        let cli = Cli::try_parse_from(["niri-tools", "daemon", "start"]).unwrap();
        assert!(build_command(&cli).is_none());
    }

    #[test]
    fn build_command_daemon_stop() {
        let cli = Cli::try_parse_from(["niri-tools", "daemon", "stop"]).unwrap();
        assert_eq!(build_command(&cli), Some(Command::DaemonStop));
    }

    #[test]
    fn build_command_daemon_restart() {
        let cli = Cli::try_parse_from(["niri-tools", "daemon", "restart"]).unwrap();
        assert_eq!(build_command(&cli), Some(Command::DaemonRestart));
    }

    #[test]
    fn build_command_daemon_status() {
        let cli = Cli::try_parse_from(["niri-tools", "daemon", "status"]).unwrap();
        assert_eq!(build_command(&cli), Some(Command::DaemonStatus));
    }

    #[test]
    fn build_command_scratchpad_toggle_with_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "toggle", "term"]).unwrap();
        assert_eq!(
            build_command(&cli),
            Some(Command::Toggle {
                name: Some("term".to_string())
            })
        );
    }

    #[test]
    fn build_command_scratchpad_toggle_without_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "toggle"]).unwrap();
        assert_eq!(build_command(&cli), Some(Command::Toggle { name: None }));
    }

    #[test]
    fn build_command_scratchpad_hide() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "hide"]).unwrap();
        assert_eq!(build_command(&cli), Some(Command::Hide));
    }

    #[test]
    fn build_command_scratchpad_toggle_float_with_name() {
        let cli =
            Cli::try_parse_from(["niri-tools", "scratchpad", "toggle-float", "browser"]).unwrap();
        assert_eq!(
            build_command(&cli),
            Some(Command::ToggleFloat {
                name: Some("browser".to_string())
            })
        );
    }

    #[test]
    fn build_command_scratchpad_toggle_float_without_name() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "toggle-float"]).unwrap();
        assert_eq!(
            build_command(&cli),
            Some(Command::ToggleFloat { name: None })
        );
    }

    #[test]
    fn build_command_scratchpad_float() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "float", "x"]).unwrap();
        assert_eq!(
            build_command(&cli),
            Some(Command::Float {
                name: Some("x".to_string())
            })
        );
    }

    #[test]
    fn build_command_scratchpad_tile() {
        let cli = Cli::try_parse_from(["niri-tools", "scratchpad", "tile", "y"]).unwrap();
        assert_eq!(
            build_command(&cli),
            Some(Command::Tile {
                name: Some("y".to_string())
            })
        );
    }
}
