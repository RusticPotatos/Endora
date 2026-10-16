#[test]
fn real_cli_migration_masks_code_and_removes_legacy_plaintext_session() {
    let status = std::process::Command::new("python3")
        .arg("scripts/check-migration.py")
        .arg(env!("CARGO_BIN_EXE_endora-terminal"))
        .status()
        .expect("Python developer runtime is required");
    assert!(status.success(), "masked migration contract failed");
}
