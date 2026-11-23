# Cutting a release


This is for maintainers.

There is no tag step. Bump `version` in `Cargo.toml` (and the `rhebok` entry
in `Cargo.lock`) and in `plugin/.claude-plugin/plugin.json` (a test fails
while they differ, because a plugin installed from a marketplace reads its
version from that file), add a `## <version> — <date>` section to
`CHANGELOG.md`, and merge to `main`. The `release` workflow builds every
target, runs `scripts/release-smoke.sh` against each packaged tarball, and
publishes `v<version>` with all of them, `install.sh` and the MCP bundle at
once. Then update the Homebrew formula, as [Homebrew](#homebrew) describes.
A push to `main` whose version is already released does nothing. The first
release needs manual steps; see
[When the repository goes public](#when-the-repository-goes-public).

The release notes are the `CHANGELOG.md` section whose heading's second word
is the version, up to the next `## ` heading. Use `###` for headings inside
a section. Older sections must not start with a bare version number that a
later release could match; br8n's is headed `## br8n 0.1.0` for that reason.

A release must carry all three targets (`aarch64-apple-darwin`,
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`); the workflow
refuses to publish one that is missing any of them. It publishes exactly nine
assets: a `rhebok-<target>.tar.gz` and a `rhebok-<target>.sha256` per target,
`install.sh`, `rhebok-mcp.mcpb` and `server.json`.

Each archive holds exactly three files, in this order: `rhebok`, `LICENSE`
and `THIRD-PARTY-NOTICES.md`. `rhebok update` takes the one file named
`rhebok` and ignores every other regular file at the archive root, so a later
release can add a file without breaking the updaters already installed. It
refuses an archive with no `rhebok` or two of them, and any entry that is not
a regular file at the root: a path with a directory, an absolute path, `..`,
a link or a directory. A `notices` job writes `THIRD-PARTY-NOTICES.md` once, and
every build job packs it with `scripts/package-release.sh`. The notices are
not a release asset of their own.

The macOS binary links OpenSSL statically and needs nothing from Homebrew. It
is signed ad hoc and not notarized. `install.sh`, `gh release download` and
`rhebok update` fetch it with tools that set no quarantine attribute, so it
runs without a warning. A tarball saved through a browser is quarantined; see
Option C, step 4, in the install guide.

The workflow takes roughly ten to twenty minutes between the merge and the
release being published, plus the few minutes the `notices` job needs before
any build starts.
`rhebok update` in that window reports the previous release as the latest.

To withdraw a bad release: `gh release delete v<version> --cleanup-tag --yes`,
then push to `main` again. Without `--cleanup-tag` the tag survives, and the
gate refuses that version on every push until the tag is gone. Once the
`registry` job has published a version, bump the version instead: the
registry keeps that version's entry, and a rebuilt bundle has a different
SHA-256 from the one the entry names.

## The MCP registry

`scripts/mcp-bundle.sh` builds `rhebok-mcp.mcpb`, an MCP bundle that holds all
three builds and a launcher, `server/rhebok-mcp`. The launcher runs the
rhebok that `rhebok install` placed, and the bundled build for the machine
only when it finds none, so a bundle installed beside a real install serves
that install's index. A bundle cannot say which chip a Linux build is for,
only which operating system, which is why one bundle carries every build.

The script also writes `server.json`, the registry entry: the bundle's
release URL and its SHA-256. The `bundle` job builds both once, and the
`publish` job releases those same files. Before that, `bundle-check` runs the
MCPB validator on the unpacked bundle and the registry's own validation on
`server.json`, so a dry run proves both. `bundle-check` holds no write
permission and uploads nothing, because the validator is fetched from npm.

The `registry` job runs after a real release (never on a dry run), downloads
that `server.json` from the release, and publishes it to
registry.modelcontextprotocol.io as `io.github.Stellenbosch-Technologies/rhebok`.
It signs in with the workflow's own OIDC token, which the registry accepts for
the organization's namespace, so no secret is stored. Anyone who can change
this workflow on `main` can therefore publish under that namespace. A failed
`registry` job leaves the release untouched. Rerun only the failed job: a
"re-run all jobs" sees the release already exists and skips it. Rerunning
helps only when the failure was transient. If the registry rejected the
entry itself, fix it and release a new version.

`mcp-publisher` is pinned in the job and verified with cosign before it runs.
To move to a newer one, change `MCP_PUBLISHER_VERSION` on purpose, to a
release at least two weeks old.

## LadybugDB

`.cargo/config.toml` pins `LBUG_VERSION`. Without the pin, lbug's build
script downloads LadybugDB's newest release, which need not match the crate.
`scripts/fetch-lbug.sh [<target>]` downloads the pinned library for the host,
checks it against a sha256 recorded in the script, and unpacks it where the
build script looks first, so the build itself downloads nothing. It refuses
when `Cargo.lock` and the pin disagree, and it can only fetch for the host.
Every CI and release job that builds rhebok runs `cargo clean -p lbug` and
then this script.

To move to a new LadybugDB, change the `lbug` version in `Cargo.lock`,
`LBUG_VERSION` in `.cargo/config.toml` and the checksum table in
`scripts/fetch-lbug.sh` together. Dependabot ignores `lbug` for that reason
(`.github/dependabot.yml`). It can change only `Cargo.toml` and `Cargo.lock`,
and `fetch-lbug.sh` fails every building CI job while the lock and the pin
disagree, which would hold back every other crate in its grouped pull
request.

`scripts/third-party-notices.sh` maps each library in LadybugDB's
`third_party` directory to its licence, or lists it as not linked. If a bump
makes it fail because LadybugDB bundles a library it neither maps nor lists,
or because a mapped licence file or header changed, update that map. Never
bypass the check.

## When the repository goes public

The first release does not happen by itself. The `release` workflow is
disabled until the repository is public, and GitHub neither queues nor
replays a push that reached `main` while a workflow was disabled, so the
merge that carried the version bump publishes nothing. Do these steps in
this order:

1. Make the repository public. While it is private, two separate requests
   answer 404. The README's one-line install downloads
   `releases/latest/download/install.sh` anonymously, and that download
   fails. `rhebok update` asks the GitHub API for `releases/latest`, and it
   reports a 404 from that call as "no rhebok release has been published
   yet". `RHEBOK_GITHUB_TOKEN` makes `rhebok update` send a token, so its
   check can work on a private repository, but the installer's download
   still fails.
2. Open a pull request and confirm that the `ci` workflow runs on it and
   passes. Add the ruleset (setting 2 below) only after that, because a
   required check that never reports blocks every merge.
3. Turn the release workflow on: `gh workflow enable release.yml`.
4. Run it as a dry run:
   `gh workflow run release.yml --ref main -f dry_run=true`. It builds and
   smoke-tests every target and publishes nothing. `dry_run` defaults to
   `true`, so a run started from the Actions page with its defaults is also
   a dry run.
5. When the dry run is green, run it for real:
   `gh workflow run release.yml --ref main -f dry_run=false`. It publishes
   only from `main`, and only if `v<version>` is not released yet. Confirm
   that the release has all nine assets.
6. Update the Homebrew formula, as [Homebrew](#homebrew) below describes.

From then on, a version bump merged to `main` publishes the release with no
manual step. Only the Homebrew formula still needs updating by hand, as
[Homebrew](#homebrew) describes.

Apply these settings once, in the repository and organization settings.
Nothing in the tree can apply them.

1. Turn on private vulnerability reporting in the repository's security
   settings. Without it, the **Report a vulnerability** button that
   `SECURITY.md` and the new-issue page link to does not exist.
2. Add a ruleset on `main`: require pull requests and the `test`, `msrv`,
   `dashboard` and `e2e` checks from `ci` and the `cargo` and `npm` checks
   from `audit`, and block force pushes and deletion.
3. Require two-factor authentication for every member of the organization.
4. Require actions to be pinned to a full commit SHA. Every workflow already
   does this.
5. Turn on Dependabot alerts, secret scanning and push protection.

## Homebrew

The tap is
[`Stellenbosch-Technologies/homebrew-tap`](https://github.com/Stellenbosch-Technologies/homebrew-tap),
and users install from it with
`brew install stellenbosch-technologies/tap/rhebok`. Update its formula after
every release. Work in the copy of the tap that Homebrew itself uses, because
`brew audit` and `brew test` take a formula name, not a file path. From the
root of this repository:

```bash
brew tap stellenbosch-technologies/tap
tap="$(brew --repository stellenbosch-technologies/tap)"
git -C "$tap" pull --ff-only
scripts/homebrew-formula.sh <version> > rhebok.rb.new && mv rhebok.rb.new "$tap/Formula/rhebok.rb"
scripts/homebrew-linux-test.sh --formula "$tap/Formula/rhebok.rb"
export HOMEBREW_NO_AUTO_UPDATE=1
brew audit --strict --online stellenbosch-technologies/tap/rhebok
brew upgrade stellenbosch-technologies/tap/rhebok
brew test stellenbosch-technologies/tap/rhebok
git -C "$tap" commit -am "The tap installs rhebok <version>"
git -C "$tap" push
```

`scripts/homebrew-linux-test.sh` tests the Linux half of the formula in a new
`ubuntu:24.04` container, as ADR-0008 requires for install tests. It installs
Homebrew, installs rhebok from the tap with the formula you pass, runs
`brew test`, indexes two notes without Ollama and searches them. It needs only
Docker, and tests the architecture of the machine it runs on. It refuses an
emulated architecture: under emulation on an Apple-silicon Mac, Homebrew
rejects the x86_64 CPU and then hangs. With no `--formula` it tests the
published formula, which is what `make brew-linux` does. Set `DOCKER` to use a
`docker` that is not on your PATH.

After the tap is pushed, run the `homebrew` workflow
(`gh workflow run homebrew.yml`). It runs the same script on GitHub's x86_64
and arm64 Linux runners, so both Linux builds are tested natively.

`HOMEBREW_NO_AUTO_UPDATE` stops `brew upgrade` from pulling the tap over the
uncommitted formula. Use `brew install` instead of `brew upgrade` if rhebok is
not installed from the tap yet. The script writes to a temporary file first,
so a failed run leaves the old formula in place.

The script reads each build's checksum from the published release, so it
fails until the release has all three builds. The formula installs `LICENSE`
and `THIRD-PARTY-NOTICES.md` beside the program, and its test checks both.
For a version with a pre-release suffix, such as `0.3.0-rc.1`, the script
adds an explicit `version` line, because Homebrew reads only `0.3.0` from the
URL.

The macOS build is the formula's top-level `url`, with
`depends_on arch: :arm64`, so an Intel Mac is refused with a clear message.
Keep it there. Inside `on_macos`, `brew audit --strict` rejects a `url`.
Inside `on_arm`, an Intel Mac has no URL at all, and Homebrew cannot load the
formula there.

Do not skip this step. `rhebok update` on a Homebrew install runs
`brew upgrade rhebok` and then checks the version it got. Until the tap
carries the new version, it fails with "its rhebok formula has not caught up
with the release yet".

## Building a release by hand

When the workflow cannot run, build one target on a machine of that target.
First write the notices file, which needs `cargo-about` 0.9.3, Node,
`python3` and the dashboard's packages:

```bash
cargo install --locked --features cli cargo-about@0.9.3
(cd dashboard && npm ci --ignore-scripts)
scripts/third-party-notices.sh THIRD-PARTY-NOTICES.md
scripts/build-release.sh THIRD-PARTY-NOTICES.md [<target>]
```

`cargo install` needs `--features cli`, or it installs no binary.
`scripts/build-release.sh <notices> [<target>]` refuses to run without the
notices file. It runs `scripts/fetch-lbug.sh`, builds, packages with
`scripts/package-release.sh`, and runs `scripts/release-smoke.sh`, writing
`dist/rhebok-<target>.tar.gz` and its `.sha256`. It links OpenSSL statically
on macOS without modifying Homebrew, remaps build paths so the binary does
not contain the builder's home directory, and packs the archive with owner
0/0 and no extended attributes so its headers do not carry the builder's user
name. Upload the files to a draft release together with `scripts/install.sh`,
then publish it.

`scripts/release-smoke.sh` makes ten checks. Among them: the binary reports
the right version, a tier-2 search opens the database (the only check that
the export-dynamic linker flag survived the release profile), the archive
holds `rhebok`, `LICENSE` and `THIRD-PARTY-NOTICES.md` in that order, and the
notices list the Rust crates and the font licence.
