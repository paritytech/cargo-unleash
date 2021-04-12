use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use cargo::{core::source::SourceId, ops::read_package, util::config::Config as CargoConfig};
use predicates::prelude::*;
use semver::Version;
use std::path;
use std::process::Command;

#[test]
fn check_include_pre() -> Result<(), Box<dyn std::error::Error>> {
    let temp = assert_fs::TempDir::new()?;
    temp.copy_from("tests/fixtures/include-pre", &["*.toml", "*.rs"])?;

    let mut cmd = Command::cargo_bin("cargo-unleash")?;

    cmd.arg("--manifest-path")
        .arg(temp.path())
        .arg("check")
        .arg("--packages")
        .arg("crate_a")
        .arg("--include-pre-deps");
    cmd.assert().success().code(0);
    temp.close()?;
    Ok(())
}
