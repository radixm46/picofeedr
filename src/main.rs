//! Picofeedr CLI entrypoint.

mod commands;
mod output;

use clap::{CommandFactory, Parser};
use picofeedr::cli::{Cli, Command, OutputFormat};
use picofeedr::config;
use picofeedr::config::feeds::ConfigCheckReport;
use picofeedr::entry::{EntryDetail, EntryListResponse};
use picofeedr::error::AppError;
use picofeedr::feed::FeedListResponse;
use picofeedr::response::{Envelope, MarkResponse, TagListResponse, VersionResponse};
use picofeedr::status::StatusResponse;
use picofeedr::sync::SyncSummary;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::process::ExitCode;
use tracing::debug;

/// Runtime failure category for command execution and output rendering.
enum RunFailure {
    /// Domain/application level failure.
    App(AppError),
    /// Stdout write failure while rendering output.
    Io(io::Error),
}

/// Typed result of executing one CLI command, consumed by JSON/plain renderers.
pub(crate) enum CommandOutcome {
    /// `version` payload.
    Version(VersionResponse),
    /// `tags` payload.
    Tags(TagListResponse),
    /// `status` payload.
    Status(StatusResponse),
    /// `feeds` payload with plain-output column options.
    Feeds {
        feeds: FeedListResponse,
        include_id: bool,
    },
    /// `sync` summary.
    Sync(SyncSummary),
    /// `sync --check` validation report.
    SyncCheck(ConfigCheckReport),
    /// `list` payload with plain-output column options.
    List {
        list: EntryListResponse,
        include_id: bool,
    },
    /// `view` payload.
    View(EntryDetail),
    /// `mark` payload.
    Mark(MarkResponse),
}

/// Command outcome paired with the process exit code to report.
pub(crate) struct CommandRun {
    outcome: CommandOutcome,
    exit_code: ExitCode,
}

impl CommandRun {
    /// Wraps an outcome that exits successfully.
    pub(crate) fn success(outcome: CommandOutcome) -> Self {
        Self {
            outcome,
            exit_code: ExitCode::SUCCESS,
        }
    }

    /// Wraps an outcome with an explicit exit code (e.g. failed `sync --check`).
    pub(crate) fn with_exit_code(outcome: CommandOutcome, exit_code: ExitCode) -> Self {
        Self { outcome, exit_code }
    }
}

impl From<AppError> for RunFailure {
    /// Converts an [`AppError`] into [`RunFailure`].
    fn from(error: AppError) -> Self {
        Self::App(error)
    }
}

impl From<io::Error> for RunFailure {
    /// Converts an [`io::Error`] into [`RunFailure`].
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RunFailure {
    /// Converts a JSON serialization error into [`RunFailure`].
    fn from(error: serde_json::Error) -> Self {
        Self::App(AppError::from(error))
    }
}

/// Runs the CLI and prints JSON output or error to stdout.
fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().collect();
    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => return handle_cli_parse_error(&args, error),
    };

    if matches!(cli.command, Command::Version) {
        let output = resolve_output_format(&cli, None);
        let debug = cli.debug;
        init_logging(debug);
        debug!(?output, ?cli.command, "resolved CLI output and command");
        return handle_run_result(debug, output, run_version(output));
    }

    let config = match load_runtime_config(&cli) {
        Ok(config) => config,
        Err(error) => {
            let output = resolve_output_format(&cli, None);
            let debug = cli.debug;
            init_logging(debug);
            debug!(?output, ?cli.command, "resolved CLI output and command");
            return handle_app_failure(debug, output, error);
        }
    };
    let output = resolve_output_format(&cli, Some(&config));
    let debug = cli.debug;
    init_logging(debug);
    debug!(?output, ?cli.command, "resolved CLI output and command");
    handle_run_result(debug, output, run(&cli, output, &config))
}

/// Executes the CLI command and renders the outcome in the selected format.
fn run(
    cli: &Cli,
    output: OutputFormat,
    config: &config::AppConfig,
) -> Result<ExitCode, RunFailure> {
    let command_run = commands::run_command(&cli.command, output, config)?;
    finish_command_run(output, command_run)
}

/// Runs `version` without loading config.
fn run_version(output: OutputFormat) -> Result<ExitCode, RunFailure> {
    finish_command_run(
        output,
        CommandRun::success(CommandOutcome::Version(version_response())),
    )
}

/// Renders a command outcome and returns its exit code.
fn finish_command_run(
    output: OutputFormat,
    command_run: CommandRun,
) -> Result<ExitCode, RunFailure> {
    let CommandRun { outcome, exit_code } = command_run;
    output::write_command_output(output, outcome)?;
    Ok(exit_code)
}

/// Maps a run result to the process exit code, rendering failures.
fn handle_run_result(
    debug: bool,
    output: OutputFormat,
    result: Result<ExitCode, RunFailure>,
) -> ExitCode {
    match result {
        Ok(exit_code) => exit_code,
        Err(RunFailure::Io(error)) => handle_output_error(debug, error),
        Err(RunFailure::App(error)) => handle_app_failure(debug, output, error),
    }
}

/// Builds the version payload for this binary.
pub(crate) fn version_response() -> VersionResponse {
    VersionResponse {
        api_version: env!("CARGO_PKG_VERSION").to_string(),
        db_schema_version: picofeedr::db::migrate::current_schema_version(),
        build: "dev".to_string(),
    }
}

/// Loads config and applies CLI overrides for the current command.
fn load_runtime_config(cli: &Cli) -> Result<config::AppConfig, AppError> {
    let mut config = config::AppConfig::load(cli.config.clone())?;
    if let Some(root_dir) = cli.storage_root.clone() {
        config.override_root_dir(root_dir);
    }
    Ok(config)
}

/// Resolves effective output format (CLI > config > default).
fn resolve_output_format(cli: &Cli, config: Option<&config::AppConfig>) -> OutputFormat {
    if let Some(output) = cli.output {
        return output;
    }
    match cli.command {
        Command::Version => OutputFormat::Plain,
        _ => config
            .map(config::AppConfig::output_format)
            .unwrap_or(OutputFormat::Plain),
    }
}

/// Handles CLI parse errors and prints appropriate output.
fn handle_cli_parse_error(args: &[OsString], error: clap::Error) -> ExitCode {
    use clap::error::ErrorKind;
    let output = detect_output_from_args(args);
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            // Help/version output remains successful when stdout is closed by the consumer.
            let _ = error.print();
            ExitCode::SUCCESS
        }
        _ => {
            match output {
                OutputFormat::Json => {
                    let app_error = AppError::config(error.to_string());
                    if let Err(write_error) = write_fatal_output(output, &app_error) {
                        if is_broken_pipe_error(&write_error) {
                            return ExitCode::SUCCESS;
                        }
                        eprintln!("failed to write CLI output: {write_error}");
                    }
                }
                OutputFormat::Plain => {
                    eprintln!("{error}");
                }
            }
            ExitCode::from(1)
        }
    }
}

/// Writes a fatal error payload for the selected output format.
fn write_fatal_output(output: OutputFormat, error: &AppError) -> io::Result<()> {
    match output {
        OutputFormat::Json => output::print_json(&Envelope::<()>::fatal(error)),
        OutputFormat::Plain => {
            eprintln!("{error}");
            Ok(())
        }
    }
}

/// Handles application failures and preserves existing diagnostics and exit behavior.
fn handle_app_failure(debug: bool, output: OutputFormat, error: AppError) -> ExitCode {
    maybe_print_diagnostics(debug, &error);
    match write_fatal_output(output, &error) {
        Ok(()) => ExitCode::from(1),
        Err(write_error) => handle_output_error(debug, write_error),
    }
}

/// Handles stdout output errors and maps broken pipes to successful termination.
fn handle_output_error(debug: bool, error: io::Error) -> ExitCode {
    if is_broken_pipe_error(&error) {
        if debug {
            eprintln!("stdout closed by downstream consumer (broken pipe)");
        }
        return ExitCode::SUCCESS;
    }
    let app_error = AppError::io_with_source("failed to write CLI output", error);
    maybe_print_diagnostics(debug, &app_error);
    eprintln!("{app_error}");
    ExitCode::from(1)
}

/// Returns true when the I/O error was caused by a broken stdout pipe.
fn is_broken_pipe_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::BrokenPipe
}

/// Detects output format from raw CLI args.
fn detect_output_from_args(args: &[OsString]) -> OutputFormat {
    Cli::command()
        .ignore_errors(true)
        .try_get_matches_from(args)
        .ok()
        .and_then(|matches| matches.get_one::<OutputFormat>("output").copied())
        .unwrap_or(OutputFormat::Plain)
}

/// Prints error diagnostics to stderr when debug is enabled.
fn maybe_print_diagnostics(debug: bool, error: &AppError) {
    if !debug {
        return;
    }
    eprintln!("error: {error}");
    let mut current = error.source();
    let mut depth = 0usize;
    while let Some(source) = current {
        depth += 1;
        eprintln!("caused by[{depth}]: {source}");
        current = source.source();
    }
}

/// Initializes stderr logging for diagnostics.
fn init_logging(debug: bool) {
    if !debug {
        return;
    }
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(io::stderr)
        .with_ansi(false)
        .init();
}
