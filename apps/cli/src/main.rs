use anyhow::{Context, bail};
use clap::{Parser, Subcommand, ValueEnum};
use savescummer_core::*;
use savescummer_ipc::*;
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(about = "Command-line client for the Save Scummer background runtime")]
struct Cli {
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Background host executable override for development and tests.
    #[arg(long, global = true, value_name = "EXECUTABLE")]
    host: Option<PathBuf>,
    /// Reuse after a transport error to retrieve the same accepted operation.
    #[arg(long, global = true)]
    request_id: Option<String>,
    /// Connect only; do not start a host if one is absent.
    #[arg(long, global = true)]
    no_start: bool,
    #[command(subcommand)]
    command: CliCommand,
}
#[derive(Subcommand)]
enum CliCommand {
    State,
    Watch,
    /// Print the per-user service endpoint for integration setup.
    Endpoint,
    Startup {
        #[arg(value_enum)]
        setting: SoundSetting,
    },
    SaveActive,
    LoadActive,
    /// Resolve a folder to the exact actions allowed in its Explorer menu.
    ExplorerTargets {
        path: PathBuf,
    },
    Explore {
        game: String,
    },
    /// Reset paths to the sole detected catalog location.
    Reset {
        game: String,
    },
    /// Select an installation/data location from the latest state's candidate list.
    SelectLocation {
        game: String,
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        exe: Vec<PathBuf>,
    },
    /// Enable or disable the app-wide Play sounds preference.
    Sounds {
        #[arg(value_enum)]
        setting: SoundSetting,
    },
    Configure {
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        exe: Vec<PathBuf>,
    },
    AddCustom {
        #[arg(long)]
        name: String,
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        exe: PathBuf,
    },
    History {
        game: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, conflicts_with = "all")]
        cursor: Option<String>,
        #[arg(long)]
        all: bool,
    },
    Save {
        game: String,
    },
    Load {
        game: String,
        /// Saved checkpoint ID from state.snapshots or a history row's snapshot_id.
        #[arg(long, value_name = "CHECKPOINT_ID")]
        target: Option<Id>,
    },
    Revert {
        game: String,
        /// Recovery checkpoint ID from a Loaded/Reverted row's recovery_id.
        #[arg(value_name = "RECOVERY_CHECKPOINT_ID")]
        target: Id,
    },
    Recover {
        game: String,
        operation: Id,
        #[arg(value_enum)]
        choice: Choice,
    },
    Operation {
        id: Id,
    },
    FlushPreview {
        game: String,
    },
    FlushDetails {
        game: String,
        #[arg(long)]
        cursor: String,
    },
    Flush {
        game: String,
        #[arg(long)]
        confirmed_revision: u64,
    },
    Forget {
        game: String,
        #[arg(long)]
        confirmed_revision: u64,
    },
    Rescan,
    Shutdown,
}
#[derive(Clone, ValueEnum)]
enum SoundSetting {
    On,
    Off,
}
#[derive(Clone, ValueEnum)]
enum Choice {
    Retry,
    KeepCurrent,
    RestoreBefore,
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let data_dir = cli
        .data_dir
        .map(Ok)
        .unwrap_or_else(savescummer_platform::app_data_dir)?;
    std::fs::create_dir_all(&data_dir)?;
    let data_dir = std::fs::canonicalize(data_dir)?;
    let address = endpoint(&data_dir)?;
    if matches!(cli.command, CliCommand::Endpoint) {
        println!("{}", address.display());
        return Ok(());
    }
    if !cli.no_start
        && connect(&address).await.is_err()
        && !matches!(cli.command, CliCommand::Shutdown)
    {
        let executable = cli
            .host
            .clone()
            .unwrap_or(std::env::current_exe()?.with_file_name(if cfg!(windows) {
                "SaveScummer.Host.exe"
            } else {
                "SaveScummer.Host"
            }));
        let mut host = std::process::Command::new(executable);
        host.arg("--data-dir")
            .arg(&data_dir)
            .arg("--minimized")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            host.creation_flags(0x08000000);
        }
        let mut child = host.spawn().context("cannot start background host")?;
        // Another client may win the instance race; probe the endpoint either way.
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if connect(&address).await.is_ok() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                bail!("host did not become ready within 15 seconds");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    let all_history = matches!(&cli.command, CliCommand::History { all: true, .. });
    let command = match cli.command {
        CliCommand::State => Command::State,
        CliCommand::Watch => Command::Watch,
        CliCommand::Endpoint => unreachable!(),
        CliCommand::Startup { setting } => Command::SetLaunchOnStartup {
            enabled: matches!(setting, SoundSetting::On),
        },
        CliCommand::SaveActive => Command::ExecuteActive {
            action: ShortcutAction::Save,
        },
        CliCommand::LoadActive => Command::ExecuteActive {
            action: ShortcutAction::Load,
        },
        CliCommand::ExplorerTargets { path } => Command::ExplorerTargets { path },
        CliCommand::Explore { game } => Command::Explore { game_id: game },
        CliCommand::Reset { game } => Command::SelectDetectedLocation {
            game_id: game,
            location: None,
        },
        CliCommand::SelectLocation { game, dir, exe } => Command::SelectDetectedLocation {
            game_id: game,
            location: Some(GameLocation {
                data_dir: dir,
                executables: exe,
            }),
        },
        CliCommand::Sounds { setting } => Command::SetPlaySounds {
            enabled: matches!(setting, SoundSetting::On),
        },
        CliCommand::Configure { id, name, dir, exe } => Command::Configure {
            id,
            name,
            data_dir: dir,
            executables: exe,
        },
        CliCommand::AddCustom { name, dir, exe } => Command::AddCustomGame {
            name,
            executable: exe,
            data_dir: dir,
        },
        CliCommand::History {
            game,
            cursor,
            limit,
            ..
        } => Command::History {
            game_id: game,
            anchor_id: None,
            cursor,
            limit,
        },
        CliCommand::Save { game } => Command::Execute {
            game_id: game,
            action: Action::Save,
        },
        CliCommand::Load { game, target } => Command::Execute {
            game_id: game,
            action: Action::Load { target },
        },
        CliCommand::Revert { game, target } => Command::Execute {
            game_id: game,
            action: Action::Revert { target },
        },
        CliCommand::Recover {
            game,
            operation,
            choice,
        } => Command::Execute {
            game_id: game,
            action: Action::Recover {
                operation,
                choice: match choice {
                    Choice::Retry => RecoveryChoice::Retry,
                    Choice::KeepCurrent => RecoveryChoice::KeepCurrent,
                    Choice::RestoreBefore => RecoveryChoice::RestoreBefore,
                },
            },
        },
        CliCommand::Operation { id } => Command::Operation { operation_id: id },
        CliCommand::FlushPreview { game } => Command::FlushPreview { game_id: game },
        CliCommand::FlushDetails { game, cursor } => Command::FlushDetails {
            game_id: game,
            cursor,
        },
        CliCommand::Flush {
            game,
            confirmed_revision,
        } => Command::Execute {
            game_id: game,
            action: Action::Flush { confirmed_revision },
        },
        CliCommand::Forget {
            game,
            confirmed_revision,
        } => Command::Execute {
            game_id: game,
            action: Action::Forget { confirmed_revision },
        },
        CliCommand::Rescan => Command::Rescan,
        CliCommand::Shutdown => Command::Shutdown,
    };
    let request = Request {
        version: VERSION,
        request_id: cli.request_id.unwrap_or_else(new_id),
        command,
    };
    // Print before transmission so an uncertain reply can be queried/retried safely.
    eprintln!("request_id={}", request.request_id);
    if all_history {
        let mut request = request;
        loop {
            let response = savescummer_ipc::request(&address, &request).await?;
            match response.result {
                Reply::HistoryPage { page } => {
                    for row in page.rows {
                        println!("{}", serde_json::to_string(&row)?);
                    }
                    let Some(next) = page.next_cursor else {
                        return Ok(());
                    };
                    if let Command::History { cursor, .. } = &mut request.command {
                        *cursor = Some(next);
                    }
                    request.request_id = new_id();
                }
                Reply::Error { error } => bail!(error),
                _ => bail!("unexpected history response"),
            }
        }
    }
    if matches!(request.command, Command::Watch) {
        let mut stream = connect(&address).await?;
        write_frame(&mut stream, &request).await?;
        loop {
            let response: Response = read_frame(&mut stream).await?;
            println!("{}", serde_json::to_string(&response)?);
            if let Reply::Error { error } = response.result {
                bail!(error);
            }
        }
    }
    let response = savescummer_ipc::request(&address, &request)
        .await
        .context("request failed; this does not establish whether an operation was accepted")?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    match response.result {
        Reply::Accepted { operation_id } => loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let response = savescummer_ipc::request(
                &address,
                &Request {
                    version: VERSION,
                    request_id: new_id(),
                    command: Command::Operation {
                        operation_id: operation_id.clone(),
                    },
                },
            )
            .await
            .context("connection lost; query the accepted operation ID after reconnecting")?;
            if let Reply::Operation { operation } = &response.result {
                if operation.status != OperationStatus::Pending {
                    println!("{}", serde_json::to_string_pretty(&response)?);
                    if matches!(
                        operation.status,
                        OperationStatus::Failed | OperationStatus::RecoveryNeeded
                    ) {
                        bail!("operation {} ended as {:?}", operation.id, operation.status);
                    }
                    break;
                }
            } else {
                bail!("unexpected operation response: {:?}", response.result);
            }
        },
        Reply::Error { error } => bail!(error),
        _ => (),
    }
    Ok(())
}
