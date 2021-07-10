# Changelog

The format is based on [Keep a Changelog].

[Keep a Changelog]: http://keepachangelog.com/en/1.0.0/

## Unreleased

- New [version command now takes new `bump-to-dev`](https://github.com/paritytech/cargo-unleash/pull/47) that bumps to the next breaking version and appends a `-dev` pre-release value
- Use saved credentials from `cargo login`
- Do not fail if package selections turns up 0 (unless `--empty-is-failure` is given)

## 1.0.0-alpha.12 - 2021-05-18
- New [version command  now takes new `bump-breaking`](https://github.com/paritytech/cargo-unleash/pull/37) that bumps to the next breaking version
- Fix: [dependency injection now uses localised packages exclusively](https://github.com/paritytech/cargo-unleash/pull/39), fixing the problem of leaking the local workspace path's into the build when only releasing a subset of crates
- Fix: Updated to latest dependencies, cargo `0.53.0`.

## 1.0.0-alpha.11 - 2021-01-05

- New: [Support for automatic readme generation](https://github.com/paritytech/cargo-unleash/pull/9) behind non-default `gen-readme`-feature-flag
- Fix: [Adhere to crates.io crawler policy](https://github.com/paritytech/cargo-unleash/pull/23)
- Fix: Updated to latest dependencies, cargo `0.50.0`.
- Various smaller fixes and improvements
- Meta: Started a changelog