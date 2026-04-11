use super::*;

#[test]
fn plain_output_succeeds_when_stdout_is_closed() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let storage_root = db_root(&paths.db_path);

    let output = run_with_closed_stdout(vec![
        "--output".to_string(),
        "plain".to_string(),
        "--config".to_string(),
        paths.config_path.clone(),
        "--storage-root".to_string(),
        storage_root,
        "feeds".to_string(),
    ]);

    assert!(output.status.success());
}

#[test]
fn json_output_succeeds_when_stdout_is_closed() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let storage_root = db_root(&paths.db_path);

    let output = run_with_closed_stdout(vec![
        "--output".to_string(),
        "json".to_string(),
        "--config".to_string(),
        paths.config_path.clone(),
        "--storage-root".to_string(),
        storage_root,
        "feeds".to_string(),
    ]);

    assert!(output.status.success());
}

#[test]
fn broken_pipe_emits_debug_diagnostic() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let storage_root = db_root(&paths.db_path);

    let output = run_with_closed_stdout(vec![
        "--output".to_string(),
        "plain".to_string(),
        "--debug".to_string(),
        "--config".to_string(),
        paths.config_path.clone(),
        "--storage-root".to_string(),
        storage_root,
        "feeds".to_string(),
    ]);

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("broken pipe"));
}

#[test]
fn parse_error_with_output_equals_json_is_enveloped() {
    let output = cargo_bin_cmd!("picofeedr")
        .arg("--output=json")
        .arg("unknown")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_error_envelope(&output, "CONFIG_ERROR", false);
}

#[test]
fn root_help_command_matches_long_help() {
    let long_help = cargo_bin_cmd!("picofeedr")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help_subcommand = cargo_bin_cmd!("picofeedr")
        .arg("help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(help_subcommand, long_help);
}

#[test]
fn root_short_help_stays_distinct_from_long_help() {
    let long_help = String::from_utf8(
        cargo_bin_cmd!("picofeedr")
            .arg("--help")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .expect("utf8");
    let short_help = String::from_utf8(
        cargo_bin_cmd!("picofeedr")
            .arg("-h")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .expect("utf8");

    assert_ne!(short_help, long_help);
    assert!(short_help.contains("Print help (see more with '--help')"));
    assert!(long_help.contains("Print help (see a summary with '-h')"));
}

#[test]
fn missing_subcommand_error_stays_compact_and_points_to_help() {
    let short_help = String::from_utf8(
        cargo_bin_cmd!("picofeedr")
            .arg("-h")
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .expect("utf8");
    let stderr = String::from_utf8(
        cargo_bin_cmd!("picofeedr")
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone(),
    )
    .expect("utf8");

    assert_eq!(stderr.trim_end(), short_help.trim_end());
}

#[test]
fn unknown_subcommand_error_stays_compact_and_points_to_help() {
    let stderr = String::from_utf8(
        cargo_bin_cmd!("picofeedr")
            .arg("unknown")
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone(),
    )
    .expect("utf8");

    assert!(stderr.starts_with("error: unrecognized subcommand 'unknown'"));
    assert!(stderr.contains("Usage: picofeedr [OPTIONS] <COMMAND>"));
    assert!(stderr.contains("For more information, try '--help'."));
}

#[test]
fn version_plain_renders_one_kv_per_line() {
    let output = picofeedr_cmd_plain()
        .arg("version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let output = String::from_utf8(output).expect("utf8");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("api_version: "));
    assert!(lines[1].starts_with("db_schema_version: "));
    assert!(lines[2].starts_with("build: "));
}

#[test]
fn db_locked_returns_retry_true() {
    let temp = TempDir::new().expect("tempdir");
    let paths = write_fixture_files(&temp);
    let _lock = acquire_exclusive_db_lock(&paths.db_path);

    let output = picofeedr_cmd_json()
        .arg("--config")
        .arg(&paths.config_path)
        .arg("--storage-root")
        .arg(db_root(&paths.db_path))
        .arg("tags")
        .assert()
        .failure()
        .get_output()
        .stdout
        .clone();
    assert_error_envelope(&output, "DB_LOCKED", true);
    let details = extract_error_details(&output);
    assert_eq!(details["retry_after_ms"], 200);
    assert!(details["sqlite_code"].is_string() || details["sqlite_code"].is_null());
}
