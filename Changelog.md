# Changelog

The format is based on [Keep a Changelog].

[Keep a Changelog]: http://keepachangelog.com/en/1.0.0/

## 1.0.0-alpha.14
- 2022 refresh, add color to the help and upgrade most deps
- Migrated to 2021 edition, enforcing MSRV of `1.56.1`. [#58](https://github.com/paritytech/cargo-unleash/pull/58)
- New cli-option for `graphviz`/`dot`-file dependency graph generation [#59](https://github.com/paritytech/cargo-unleash/pull/59)
- Fix: Handle diamond shaped dependency trees within a workspace [#59](https://github.com/paritytech/cargo-unleash/pull/59)

## 1.0.0-alpha.13 - 2021-10-11
- Update to cargo 0.57 and semver 1.0 – support for `edition = "2021"`
- Breaking (UX): Not finding any package with the selections given is not considered an error anymore, but means the process ends successfully. If you want the old behaviour back where no package matching the criteria gives you a non-zero exit code add the `--empty-is-failure` cli switch to the call.
- New: [`version` subcommand `bump-to-dev`](https://github.com/paritytech/cargo-unleash/pull/47) bumps to the next breaking version and appends a `-dev` pre-release value
- New: `--changed-since=GIT_REF`-package selection param allows you to specify only crates that have been touched between the current git head and the given `$GIT_REF` (e.g. your current branch and `master`) - very useful to check only crates changed in a PR. See `--help` for more information.
- Fix: Use saved credentials from `cargo login`, fixes #35
- Fix: A new end-to-end test suite checks that the params work as expected, still needs more tests but it's a start.

## 1.0.0-alpha.12 - 2021-05-18
- New [version command now takes new `bump-breaking`](https://github.com/paritytech/cargo-unleash/pull/37) that bumps to the next breaking version
- Fix: [dependency injection now uses localised packages exclusively](https://github.com/paritytech/cargo-unleash/pull/39), fixing the problem of leaking the local workspace path's into the build when only releasing a subset of crates
- Fix: Updated to latest dependencies, cargo `0.53.0`.

## 1.0.0-alpha.11 - 2021-01-05

- New: [Support for automatic readme generation](https://github.com/paritytech/cargo-unleash/pull/9) behind non-default `gen-readme`-feature-flag
- Fix: [Adhere to crates.io crawler policy](https://github.com/paritytech/cargo-unleash/pull/23)
- Fix: Updated to latest dependencies, cargo `0.50.0`.
- Various smaller fixes and improvements
- Meta: Started a changelog
