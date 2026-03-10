//! Picofeedr CLI entrypoint.

mod command_exec;
mod output;

use clap::Parser;
use picofeedr::cli::{Cli, Command, OutputFormat};
use picofeedr::config;
use picofeedr::entry::{EntryDetail, EntryListResponse};
use picofeedr::error::AppError;
use picofeedr::feed::FeedListResponse;
use picofeedr::response::{Envelope, ResponseStatus};
use picofeedr::status::StatusResponse;
use picofeedr::sync::SyncSummary;
use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::process::ExitCode;
use tracing::debug;

/// Execution results for CLI commands.
enum CommandOutput {
    Ping,
    Version {
        api_version: &'static str,
        db_schema_version: i64,
        build: &'static str,
    },
    Tags {
        tags: Vec<String>,
    },
    Status {
        status: StatusResponse,
    },
    FeedsList {
        feeds: FeedListResponse,
    },
    Sync {
        summary: SyncSummary,
    },
    List {
        list: EntryListResponse,
        include_id: bool,
    },
    View {
        detail: EntryDetail,
    },
    Mark {
        updated: usize,
    },
}

/// Runtime failure category for command execution and output rendering.
enum RunFailure {
    /// Domain/application level failure.
    App(AppError),
    /// Stdout write failure while rendering output.
    Io(io::Error),
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
    let preloaded_config = match preload_runtime_config(&cli) {
        Ok(config) => config,
        Err(error) => {
            let output = resolve_effective_output(&cli, None);
            let log_level = resolve_effective_log_level(&cli, None);
            init_logging(log_level);
            debug!(?output, ?cli.command, "resolved CLI output and command");
            return handle_app_failure(log_level, output, error);
        }
    };
    let output = resolve_effective_output(&cli, preloaded_config.as_ref());
    let log_level = resolve_effective_log_level(&cli, preloaded_config.as_ref());
    init_logging(log_level);
    debug!(?output, ?cli.command, "resolved CLI output and command");
    if matches!(cli.command, Command::Feeds { config_check: true }) {
        let config = preloaded_config
            .as_ref()
            .expect("config-backed commands should preload config");
        match run_config_check(&cli, output, config) {
            Ok(exit_code) => return exit_code,
            Err(RunFailure::Io(error)) => return handle_output_error(log_level, error),
            Err(RunFailure::App(error)) => return handle_app_failure(log_level, output, error),
        }
    }
    if let Err(error) = run(&cli, output, preloaded_config.as_ref()) {
        match error {
            RunFailure::Io(error) => return handle_output_error(log_level, error),
            RunFailure::App(error) => return handle_app_failure(log_level, output, error),
        }
    }
    ExitCode::SUCCESS
}

/// Executes the CLI command and prints a JSON response.
fn run(
    cli: &Cli,
    output: OutputFormat,
    config: Option<&config::AppConfig>,
) -> Result<(), RunFailure> {
    let result = if matches!((&cli.command, output), (Command::Sync, OutputFormat::Plain)) {
        command_exec::execute_sync_command_plain(config.expect("sync requires config"))?
    } else {
        command_exec::execute_command(cli, config.expect("config-backed commands require config"))?
    };
    match output {
        OutputFormat::Json => output::render_json(&result)?,
        OutputFormat::Plain => output::render_plain(&result)?,
    }
    Ok(())
}

/// Runs static feeds config validation without touching the database.
fn run_config_check(
    _cli: &Cli,
    output: OutputFormat,
    config: &config::AppConfig,
) -> Result<ExitCode, RunFailure> {
    let feeds_config = config::feeds::FeedsConfig::load(&config.feeds.source)?;
    let report = feeds_config.validate();
    let is_valid = report.valid;
    match output {
        OutputFormat::Json => {
            let status = if is_valid {
                ResponseStatus::Ok
            } else {
                ResponseStatus::Warning
            };
            output::print_json_or_fallback(&Envelope::ok_with_status(report, status))?;
        }
        OutputFormat::Plain => output::render_config_check_plain(&report)?,
    }
    Ok(if is_valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

/// Loads config once for config-backed commands.
fn preload_runtime_config(cli: &Cli) -> Result<Option<config::AppConfig>, AppError> {
    if matches!(cli.command, Command::Ping | Command::Version) {
        return Ok(None);
    }
    load_runtime_config(cli).map(Some)
}

/// Loads config and applies CLI overrides for the current command.
fn load_runtime_config(cli: &Cli) -> Result<config::AppConfig, AppError> {
    let mut config = config::AppConfig::load(cli.config.clone())?;
    if let Some(root_dir) = cli.storage_root.clone() {
        config.override_root_dir(root_dir)?;
    }
    Ok(config)
}

/// Resolves effective output format (CLI > config > default).
fn resolve_output(cli: &Cli, config: Option<&config::AppConfig>) -> OutputFormat {
    if let Some(output) = cli.output {
        return output;
    }
    if let Some(config) = config {
        return config.cli.output;
    }
    OutputFormat::Plain
}

/// Resolves the effective output format using CLI or config when available.
fn resolve_effective_output(cli: &Cli, config: Option<&config::AppConfig>) -> OutputFormat {
    if let Some(output) = cli.output {
        return output;
    }
    match cli.command {
        Command::Ping | Command::Version => OutputFormat::Plain,
        _ => resolve_output(cli, config),
    }
}

/// Handles CLI parse errors and prints appropriate output.
fn handle_cli_parse_error(args: &[OsString], error: clap::Error) -> ExitCode {
    use clap::error::ErrorKind;
    let output = detect_output_from_args(args);
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
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
                OutputFormat::Plain => eprintln!("{error}"),
            }
            ExitCode::from(1)
        }
    }
}

/// Writes a fatal error payload for the selected output format.
fn write_fatal_output(output: OutputFormat, error: &AppError) -> io::Result<()> {
    match output {
        OutputFormat::Json => output::print_json_or_fallback(&Envelope::<()>::fatal(error)),
        OutputFormat::Plain => {
            eprintln!("{error}");
            Ok(())
        }
    }
}

/// Handles application failures and preserves existing diagnostics and exit behavior.
fn handle_app_failure(
    log_level: config::LogLevel,
    output: OutputFormat,
    error: AppError,
) -> ExitCode {
    maybe_print_diagnostics(log_level, &error);
    match write_fatal_output(output, &error) {
        Ok(()) => ExitCode::from(1),
        Err(write_error) => handle_output_error(log_level, write_error),
    }
}

/// Handles stdout output errors and maps broken pipes to successful termination.
fn handle_output_error(log_level: config::LogLevel, error: io::Error) -> ExitCode {
    if is_broken_pipe_error(&error) {
        if should_emit_diagnostics(log_level) {
            eprintln!("stdout closed by downstream consumer (broken pipe)");
        }
        return ExitCode::SUCCESS;
    }
    let app_error = AppError::io_with_source("failed to write CLI output", error);
    maybe_print_diagnostics(log_level, &app_error);
    eprintln!("{app_error}");
    ExitCode::from(1)
}

/// Returns true when the I/O error was caused by a broken stdout pipe.
fn is_broken_pipe_error(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::BrokenPipe
}

/// Detects output format from raw CLI args.
fn detect_output_from_args(args: &[OsString]) -> OutputFormat {
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        let arg_value = arg.to_string_lossy();
        if arg_value == "--output" {
            if let Some(value) = iter.peek() {
                return if value.to_string_lossy() == "plain" {
                    OutputFormat::Plain
                } else {
                    OutputFormat::Json
                };
            }
            return OutputFormat::Json;
        }
        if let Some(value) = arg_value.strip_prefix("--output=") {
            return if value == "plain" {
                OutputFormat::Plain
            } else {
                OutputFormat::Json
            };
        }
    }
    OutputFormat::Plain
}

/// Prints error diagnostics to stderr when debug/trace is enabled.
fn maybe_print_diagnostics(level: config::LogLevel, error: &AppError) {
    if !should_emit_diagnostics(level) {
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

/// Resolves effective log level (CLI > config > default).
fn resolve_effective_log_level(cli: &Cli, config: Option<&config::AppConfig>) -> config::LogLevel {
    if cli.trace {
        return config::LogLevel::Trace;
    }
    if cli.debug {
        return config::LogLevel::Debug;
    }
    config
        .map(|config| config.log.level)
        .unwrap_or(config::LogLevel::Info)
}

/// Returns true if diagnostics should be emitted for the log level.
fn should_emit_diagnostics(level: config::LogLevel) -> bool {
    matches!(level, config::LogLevel::Debug | config::LogLevel::Trace)
}

/// Initializes stderr logging for diagnostics.
fn init_logging(level: config::LogLevel) {
    if !should_emit_diagnostics(level) {
        return;
    }
    let level = match level {
        config::LogLevel::Error => tracing::Level::ERROR,
        config::LogLevel::Warn => tracing::Level::WARN,
        config::LogLevel::Info => tracing::Level::INFO,
        config::LogLevel::Debug => tracing::Level::DEBUG,
        config::LogLevel::Trace => tracing::Level::TRACE,
    };
    let _ = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(io::stderr)
        .with_ansi(false)
        .try_init();
}
