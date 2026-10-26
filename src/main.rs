mod cli;
mod cmd;
mod docker;
mod killer;
mod models;
mod output;
mod probe;
mod project;
mod scanner;
mod services;
mod theme;
mod ui;

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

/// True when an error chain ends in a closed pipe (e.g. `kav list | head`).
fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|e| {
        e.downcast_ref::<io::Error>()
            .is_some_and(|io| io.kind() == io::ErrorKind::BrokenPipe)
            || e.downcast_ref::<serde_json::Error>()
                .and_then(|j| j.io_error_kind())
                == Some(io::ErrorKind::BrokenPipe)
    })
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    theme::init(
        cli.theme,
        output::color_enabled(cli.color, io::stdout().is_terminal()),
    );
    match run(cli) {
        Ok(code) => code,
        Err(e) if is_broken_pipe(&e) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kav: error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        None => {
            if io::stdout().is_terminal() {
                ui::run(ui::TuiOptions {
                    interval: cli.interval,
                    mouse: !cli.no_mouse,
                })?;
            } else {
                // Piped or redirected: print the table instead of drawing a TUI.
                let args = cmd::list::ListArgs {
                    json: false,
                    sort: models::SortField::Port,
                    reverse: false,
                    filter: None,
                    watch: false,
                    interval: cli.interval,
                    tcp: true,
                    udp: true,
                    exposed: false,
                };
                cmd::list::run(&args, cli.color)?;
            }
        }

        Some(Command::List {
            json,
            sort,
            reverse,
            filter,
            watch,
            interval,
            tcp,
            udp,
            exposed,
        }) => {
            // Neither --tcp nor --udp means both
            let (tcp, udp) = match (tcp, udp) {
                (false, false) => (true, true),
                other => other,
            };
            let args = cmd::list::ListArgs {
                json,
                sort,
                reverse,
                filter,
                watch,
                interval,
                tcp,
                udp,
                exposed,
            };
            cmd::list::run(&args, cli.color)?;
        }

        Some(Command::Check {
            ports,
            json,
            free,
            quiet,
            probe,
        }) => {
            let args = cmd::check::CheckArgs {
                ports,
                json,
                free,
                quiet,
                probe,
            };
            return cmd::check::run(&args, cli.color);
        }

        Some(Command::Kill {
            ports,
            signal,
            force,
            yes,
            timeout,
            no_wait,
            escalate,
            parent,
            allow_protected,
            json,
        }) => {
            let args = cmd::kill::KillArgs {
                ports,
                signal: if force { killer::Signal::Kill } else { signal },
                yes,
                timeout,
                no_wait,
                escalate,
                parent,
                allow_protected,
                json,
            };
            return cmd::kill::run(&args);
        }

        Some(Command::Wait {
            ports,
            free,
            timeout,
            udp,
            quiet,
        }) => {
            let args = cmd::wait::WaitArgs {
                ports,
                free,
                timeout,
                udp,
                quiet,
            };
            return cmd::wait::run(&args);
        }

        Some(Command::Pick { range, count, udp }) => {
            return cmd::pick::run(&cmd::pick::PickArgs { range, count, udp });
        }

        Some(Command::Completions { shell }) => {
            cli::print_completions(shell);
        }

        Some(Command::Man) => {
            cli::print_man_page()?;
        }
    }

    Ok(ExitCode::SUCCESS)
}
