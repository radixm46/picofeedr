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
fn ping_plain_uses_kv_status_line() {
    let output = picofeedr_cmd_plain()
        .arg("ping")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(String::from_utf8(output).expect("utf8"), "status: ok\n");
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
