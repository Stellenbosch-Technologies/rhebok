# Contributing to rhebok

Thanks for helping. Bug reports, ideas and pull requests are all welcome.
Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting a bug

Open an
[issue](https://github.com/Stellenbosch-Technologies/rhebok/issues/new/choose)
with your `rhebok --version`, your operating system, which agent you use, and
what `rhebok status` says. `rhebok status` prints file paths from your machine,
so remove anything you would rather not share before you paste it.

Do not report a security problem in an issue. [SECURITY.md](SECURITY.md) says
how to report one privately, and [docs/security-model.md](docs/security-model.md)
describes what rhebok trusts and what can reach an agent's prompt.

## Building and testing

You need a current stable Rust toolchain (1.95 or newer), a C++20 compiler
(Xcode 16 on macOS, GCC 13 or newer on Linux), OpenSSL (`brew install
openssl@3` on macOS; `libssl-dev` and `pkg-config` on Linux), `curl` and
`python3`. See [Build from source](docs/install.md#build-from-source).

The first build needs network access. Run `scripts/fetch-lbug.sh` once
first: it downloads the LadybugDB library rhebok is pinned to and checks its
checksum. The tests need no Ollama, but the first test that opens a database
downloads LadybugDB's `vector` extension into `~/.lbdb`, so the first
`cargo test` needs network access too.

```bash
scripts/fetch-lbug.sh       # once, and again after a LadybugDB bump
cargo test                  # the full suite; no Ollama needed
cargo clippy --all-targets  # must be free of warnings
cargo fmt --all
make dev                    # a faster build without backup and OCR
make test-dev               # the tests for that build
make ci                     # every CI job locally, plus the dashboard's browser tests
cargo build --release && target/release/rhebok install   # try your build for real
```

The dashboard is a React app in `dashboard/`. Its built output in
`dashboard/dist/` is committed and embedded into the binary, so after changing
the dashboard run `npm ci && npm run build` in `dashboard/` and commit the
result.

[CLAUDE.md](CLAUDE.md) has more on the test layout and the performance guards.
It is written for coding agents, but it is just as useful for people.

`make ci` skips the audit job when `cargo-audit` is not installed, and the
plugin job when `claude` is not on your PATH. `tests/e2e/journey.sh` is an end-to-end check against a real
Ollama; it is not part of `cargo test`.

## Pull requests

- Keep each pull request to one change, as a single commit. Squash review
  fixes into it.
- Write the commit subject as a plain sentence that says what changed, with no
  prefix like `fix:`, for example `search skips files that no longer exist`.
- Add or update a test for any behaviour you change, and run `make ci` (or at
  least `cargo test` and `cargo clippy`) before you open the PR.
- Prefer clear names and small functions to explanatory comments.

## Licence

rhebok is licensed under the [AGPL-3.0-only](LICENSE). Contributions are
accepted under the same licence ("inbound = outbound"): by opening a pull
request you agree that your contribution is licensed under the AGPL-3.0-only,
and you confirm that you have the right to license it that way. You keep the
copyright in your contribution. There is no contributor licence agreement and
no sign-off requirement.
