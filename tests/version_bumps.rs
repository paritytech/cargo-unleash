use assert_cmd::prelude::*;
use assert_fs::prelude::*;
use cargo::{core::source::SourceId, ops::read_package, util::config::Config as CargoConfig};
use semver::Version;
use std::process::Command;

#[test]
fn set_pre() -> Result<(), Box<dyn std::error::Error>> {
	let cfg = CargoConfig::default()?;
	let temp = assert_fs::TempDir::new()?;
	temp.copy_from("tests/fixtures/simple-base", &["*.toml", "*.rs"])?;

	let mut cmd = Command::cargo_bin("cargo-unleash")?;

	cmd.arg("--manifest-path")
		.arg(temp.path())
		.arg("version")
		.arg("set-pre")
		.arg("dev")
		.arg("--packages")
		.arg("crateA")
		.arg("crateB");
	cmd.assert().success();

	let temp_path = temp.path().to_path_buf();
	let source = SourceId::for_path(temp.path())?;

	let (crate_a, _) = read_package(&temp_path.join("crateA").join("Cargo.toml"), source, &cfg)?;
	let (crate_b, _) = read_package(&temp_path.join("crateB").join("Cargo.toml"), source, &cfg)?;
	let (crate_c, _) = read_package(&temp_path.join("crateC").join("Cargo.toml"), source, &cfg)?;
	assert_eq!(crate_a.version(), &Version::parse("0.1.0-dev")?);
	assert_eq!(crate_b.version(), &Version::parse("2.0.0-dev")?);
	assert_eq!(crate_c.version(), &Version::parse("3.1.0")?); // wasn't selected

	temp.close()?;
	Ok(())
}

#[test]
fn bump_to_dev() -> Result<(), Box<dyn std::error::Error>> {
	let cfg = CargoConfig::default()?;
	let temp = assert_fs::TempDir::new()?;
	temp.copy_from("tests/fixtures/simple-base", &["*.toml", "*.rs"])?;

	let mut cmd = Command::cargo_bin("cargo-unleash")?;

	cmd.arg("--manifest-path")
		.arg(temp.path())
		.arg("version")
		.arg("bump-to-dev")
		.arg("--packages")
		.arg("crateA")
		.arg("crateB")
		.arg("crateC");
	cmd.assert().success();

	let temp_path = temp.path().to_path_buf();
	let source = SourceId::for_path(temp.path())?;

	let (crate_a, _) = read_package(&temp_path.join("crateA").join("Cargo.toml"), source, &cfg)?;
	let (crate_b, _) = read_package(&temp_path.join("crateB").join("Cargo.toml"), source, &cfg)?;
	let (crate_c, _) = read_package(&temp_path.join("crateC").join("Cargo.toml"), source, &cfg)?;
	assert_eq!(crate_a.version(), &Version::parse("0.2.0-dev")?);
	assert_eq!(crate_b.version(), &Version::parse("3.0.0-dev")?);
	assert_eq!(crate_c.version(), &Version::parse("4.0.0-dev")?);

	temp.close()?;
	Ok(())
}
