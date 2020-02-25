# [Unleash the dragons](https://github.com/gnunicorn/cargo-unleash)

`cargo` release automation tooling for _massiv mono-repo_. Developed primarily for [Parity Substrate](https://github.com/paritytech/substrate).

## Installation

Use `cargo install` to install:
```bash
cargo install https://github.com/gnunicorn/cargo-unleash
```

## Usage

Try and have it report what it would do on your mono repo with

```bash

cargo unleash em --dry=run
```

There are more options available on the CLI, just run with `--help`

## License & Credits

This Software is released under the [GNU General Public License (GPL) 3.0](https://www.gnu.org/licenses/gpl-3.0.en.html).

This, as any other software, is build on the shoulders of giants. In particular, this uses `cargo` internally and draws heavily on the knowledge established by [cargo publish-all](https://torkleyy.gitlab.io/cargo-publish-all/) and [cargo hack](https://github.com/taiki-e/cargo-hack).
