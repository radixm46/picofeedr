use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

#[test]
fn schemas_binary_writes_all_artifacts_under_root_schemas() {
    let temp = TempDir::new().expect("create temporary directory");

    cargo_bin_cmd!("schemas")
        .current_dir(temp.path())
        .assert()
        .success();

    let schema_dir = temp.path().join("schemas");
    for name in [
        "fatal-error.response.schema.json",
        "feeds.response.schema.json",
        "list.response.schema.json",
        "mark.response.schema.json",
        "status.response.schema.json",
        "sync-check.response.schema.json",
        "sync.response.schema.json",
        "tags.response.schema.json",
        "version.response.schema.json",
        "view.response.schema.json",
    ] {
        assert!(schema_dir.join(name).is_file(), "missing {name}");
    }

    assert!(!temp.path().join("doc").join("spec").join("schema").exists());
}
