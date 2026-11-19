# Installing rhebok

This takes about ten minutes, most of it the one-time model download.

`rhebok` is a single program that installs itself. Its program, index,
memories and logs live in one folder: `~/Library/Application Support/rhebok`
on macOS, or `~/.local/share/rhebok` on Linux. Its config file, `config.toml`,
sits in the same folder on macOS and in `~/.config/rhebok` on Linux.
`rhebok status` prints both paths.

rhebok also touches a few things outside those folders: a `rhebok` link on
your PATH, the Claude Code plugin registration and its cache under
`~/.claude/plugins`, the config file of each agent you connect (it keeps the
original beside it as `<file>.rhebok-bak`), and the vector-search extension
that the database library keeps under `~/.lbdb`. See [Uninstall](#uninstall)
for what each command removes.

## Before you start

You need:

- **A supported computer.** A Mac with Apple silicon (M1 or newer), or Linux
  on x86_64 or arm64 with glibc 2.39 or newer (Ubuntu 24.04 or newer). On
  Windows, use the Linux build inside
  [WSL](https://learn.microsoft.com/windows/wsl/install); native Windows
  support is planned. On an older Linux, [build from source](#build-from-source).
  Intel Macs are not supported yet, not even from source; see
  [Build from source](#build-from-source) for why.
- **Ollama, installed and running.** rhebok uses it to understand your notes
  on your own machine.
  - On macOS, install the app from [ollama.com](https://ollama.com), or run
    `brew install ollama`, then open it.
  - On Linux, Ollama's installer needs `curl` and `zstd`, and a minimal
    system such as a container image may not have them. On Ubuntu,
    install both with `sudo apt install curl zstd`. Then run
    `curl -fsSL https://ollama.com/install.sh | sh`.

  Run `ollama list` to check that it is running.
- **At least one coding agent.** Claude Code is the main one, and its
  `claude` command must be on your PATH. rhebok can also connect to Codex,
  Gemini CLI, Cursor and Claude Desktop; see
  [Connect other agents](usage.md#connect-other-agents).

## Step 1: Download and install rhebok

Pick one option.

### Option A: one-line installer

```bash
curl -fsSL https://github.com/Stellenbosch-Technologies/rhebok/releases/latest/download/install.sh | sh
```

This downloads the build for your machine, checks it against its checksum and
runs `rhebok install`. For an unattended install, which answers yes to every
question except the optional distillation model, add `-s -- --yes` after `sh`.

### Option B: with the GitHub CLI

You need the [GitHub CLI](https://cli.github.com), logged in with `gh auth login`.

```bash
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64)  target=aarch64-apple-darwin ;;
  Linux-x86_64)  target=x86_64-unknown-linux-gnu ;;
  Linux-aarch64) target=aarch64-unknown-linux-gnu ;;
  *)             target= ;;
esac
echo "target: ${target:-none, so build from source}"
```

If it prints `none`, stop here and [build from source](#build-from-source).
Otherwise:

```bash
mkdir -p ~/rhebok-download && cd ~/rhebok-download
gh release download --repo Stellenbosch-Technologies/rhebok --pattern "rhebok-$target.*"
shasum -a 256 -c "rhebok-$target.sha256"
tar -xzf "rhebok-$target.tar.gz"
./rhebok install
```

This downloads the build for your machine, checks it against its checksum,
unpacks it and runs the installer. On a Mac this route shows no security
warning. `gh` doesn't mark files as downloaded from the internet, so
Gatekeeper never checks the program. The `shasum` line must print `OK`; if it
doesn't, stop and download again.

You can delete `~/rhebok-download` afterwards, because `rhebok install` copies
itself into its own folder.

### Option C: download in your browser

1. Open the [releases page](https://github.com/Stellenbosch-Technologies/rhebok/releases) and,
   from the newest release, download the two files for your machine:
   `rhebok-<target>.tar.gz` and `rhebok-<target>.sha256`. `<target>` is:
   - `aarch64-apple-darwin` on a Mac;
   - `x86_64-unknown-linux-gnu` or `aarch64-unknown-linux-gnu` on Linux.
2. In a terminal, go to the folder you downloaded them to and check the
   download. It must print `OK`:

   ```bash
   shasum -a 256 -c rhebok-<target>.sha256
   ```

3. Unpack it. You get `rhebok`, its `LICENSE` and `THIRD-PARTY-NOTICES.md`:

   ```bash
   tar -xzf rhebok-<target>.tar.gz
   ```

4. **On a Mac only:** remove the "downloaded from the internet" mark your
   browser added. rhebok is not signed by an Apple developer account, so
   without this step macOS refuses to run it and says it cannot verify it:

   ```bash
   xattr -d com.apple.quarantine rhebok
   ```

   If it says "No such xattr", the file wasn't marked, and that's fine.

5. Install:

   ```bash
   ./rhebok install
   ```

### Option D: with Homebrew

On a Mac with Apple silicon, or on Linux with [Homebrew](https://brew.sh):

```bash
brew install stellenbosch-technologies/tap/rhebok
rhebok install
```

Homebrew downloads the build for your machine and checks it against its
checksum. It does not mark the program as downloaded from the internet, so a
Mac runs it without a warning. `rhebok install` then sets rhebok up as
[Step 2](#step-2-answer-the-installers-questions) describes, except that the
program stays where Homebrew put it and Homebrew puts it on your PATH.

### Found rhebok in Claude Code's plugin menu?

The plugin you install from `/plugin` (or with
`/plugin marketplace add Stellenbosch-Technologies/rhebok`) needs the `rhebok` program, and tells
you so at the start of each session until the program is installed. Install
it with one of the options above. `rhebok install` then replaces that plugin
with its own copy, which it keeps in step with the program, so rhebok's hooks
never run twice.

That plugin carries the `rhebok-prehook` mod too, and the mod finds the
program the same way the hooks do, from the plugin's own folder. Until the
program is installed the mod injects nothing and the status line reads
`unavailable` after the plugin name.

## Step 2: Answer the installer's questions

`rhebok install` does the following:

1. Copies itself into rhebok's folder and puts `rhebok` on your PATH.
2. Writes a starter config file. On a first install it asks which embedding
   model to use: Qwen3-Embedding 0.6B (the default) or EmbeddingGemma 2. See
   [Choosing the embedding model](#choosing-the-embedding-model).
3. Registers the rhebok plugin with Claude Code.
4. Asks before downloading the embedding model (`qwen3-embedding:0.6b`,
   about 640 MB, or `embeddinggemma-2:270m-bf16-text`, about 575 MB). Say
   yes, because rhebok can't work without it.
5. Asks, in a separate question, before downloading the session distillation
   model (`qwen3:4b-instruct`, about 2.5 GB). This one is optional; see
   [About the models](#about-the-models).
6. Lists the other coding agents it found (Codex, Gemini CLI, Cursor,
   Claude Desktop) and asks before connecting each one.

It prints what it did. Read the last lines:

- **An `export PATH=...` line:** your shell can't find `rhebok` yet. Add that
  line to your shell profile (`~/.zshrc` on a Mac, usually `~/.bashrc` on
  Linux) and open a new terminal.
- **A warning:** run `rhebok install` again. It repairs whatever it can and
  names what it can't.

`rhebok install --yes` answers yes to every question, including connecting
every agent it found, except the optional distillation model: `--yes` never
downloads it, and prints how to get it instead. So an unattended install
downloads about 640 MB. It does not ask about the embedding model; it keeps
the default unless you add `--embed-model gemma`.

### Choosing the embedding model

| | Qwen3-Embedding 0.6B (`qwen3`) | EmbeddingGemma 2 (`gemma`) |
|---|---|---|
| Ollama model | `qwen3-embedding:0.6b` | `embeddinggemma-2:270m-bf16-text` |
| Download | about 640 MB | about 575 MB |
| Ollama | any recent version | 0.40 or newer |
| Hook gate / MCP gate | 0.66 / 0.55 | 0.76 / 0.68 |

Both find the same notes: on a 27,000-chunk test corpus their recall at every
quality tier was within one question of each other. EmbeddingGemma 2 was
about 10% faster per query and indexed about 28% faster. The two models score
on different scales, which is why each comes with its own relevance gates.

Choose with `rhebok install --embed-model qwen3` or `--embed-model gemma`, or
with the Model picker in the dashboard's Settings, under Embedding. Both set
the model, its dimensions and both gates together. Switching an existing
install makes its index unreadable, so run `rhebok index --reindex`
afterwards; on a large index that takes hours. If your Ollama is older than
0.40, install refuses to download EmbeddingGemma 2 and says how to upgrade.

### Using another embedding model

At the same question, or with `--embed-model`, you can also give rhebok a
model of your own:

| You give | rhebok uses |
|---|---|
| an Ollama model name, such as `nomic-embed-text` | that model, pulled through Ollama |
| an Ollama model page, such as `https://ollama.com/library/nomic-embed-text` | the model the page names, tag included |
| a Hugging Face model page, such as `https://huggingface.co/<organisation>/<model>` | `hf.co/<organisation>/<model>`, pulled through Ollama. This works only for repositories that hold GGUF files |
| the URL of an OpenAI-compatible embedding server, such as `http://localhost:1234` | that server, for every embedding |

For an Ollama model, install pulls it, embeds one test sentence to learn how
many dimensions it returns, and writes the model and its dimensions to
`config.toml`. If the model cannot embed, which is what happens with a chat
model, install says so and leaves the config on the model it had.

For a server, install asks for a token (press Enter if it needs none), lists
the models the server offers, and asks which one to use. It then embeds one
test sentence and writes the server's URL, the model and the token to the
`env` file beside `config.toml` (see
[Embedding somewhere other than local Ollama](configuration.md#embedding-somewhere-other-than-local-ollama)).
Unattended, with `--yes`, it reads the model from `RHEBOK_EMBED_MODEL` and
the token from `RHEBOK_EMBED_TOKEN`, or uses the server's only model if it
offers just one.

The relevance gates were measured for Qwen3 and EmbeddingGemma 2 only. Every
model scores on its own scale: `nomic-embed-text` rates unrelated text around
0.73, so Qwen3's 0.66 hook gate would let unrelated notes into every prompt.
For any other model, install therefore estimates the gates. It embeds 24
built-in pairs of a question and a short note, and measures what the model
gives a question and a note about something else. It then puts each gate at
the same point in those scores where Qwen3's gates sit in Qwen3's own. For
`nomic-embed-text` that gives 0.77 for the hook and 0.70 for MCP. The
estimate can only make a gate stricter: when it comes out below Qwen3's gate,
as it does for `all-minilm`, install keeps 0.66 or 0.55 and says so. Install
prints the estimate. It is a coarse estimate: run `rhebok bench` with a golden
set to see where the gates belong for your notes, then set `threshold` under
`[hook]` and `[mcp]`. Install estimates again on a later run only while the
gates are still Qwen3's, so it never overwrites gates that you set. If the
model cannot embed the pairs, install keeps Qwen3's gates and says so.
After your first `rhebok index`, run `rhebok doctor`: it checks the server,
the model and the dimensions end to end, and whether they agree with the
vectors in your index. Before that first index, its agreement check fails,
because there are no vectors yet to compare against.

## Step 3: Tell rhebok what to search

rhebok searches the folders you choose. It also searches your past Claude Code
and Codex sessions, but a folder of notes is where it shines. Add yours in
whichever way you prefer:

- **In the dashboard (easiest):** run `rhebok dashboard`. If nothing is
  indexed yet, it opens a three-step setup: pick your folders, connect your
  agents, then index. It ends on the Search page with a first search taken
  from your own notes. If sessions were already indexed, the setup doesn't
  appear; open **Settings › Sources**, add your folders, and click
  **Re-index now**.
- **From the terminal:**

  ```bash
  rhebok config set sources '["~/notes", "~/Documents/papers"]'
  rhebok index
  ```

  `rhebok config check` confirms the folders exist.

The first index takes a few minutes for a few hundred notes. After that,
rhebok re-reads only what changed, and every new Claude Code session starts a
quick background index on its own. So you rarely need to run `rhebok index`
yourself.

## Step 4: Restart your agents

- **Claude Code:** quit and start it again. The plugin loads when a session
  starts.
- **Codex:** run `/hooks` once and trust rhebok's hook.
- **Claude Desktop:** restart it.
- **Gemini CLI and Cursor:** they pick rhebok up in their next session.

## Step 5: Check it works

```bash
rhebok status
```

The first lines show the version and the install checks, and every one should
pass. The `ollama:` line must not name a missing required model. It reads
`reachable at <url>, model(s) present`, or, if you skipped the optional
distillation model, only the optional-model line described below.
`documents:` should be more than zero.

Two lines are only information:

- **`ocr:`** says the OCR libraries are optional and not installed. Text PDFs
  are indexed as usual; only scanned pages are skipped. See
  [Scanned PDFs](configuration.md#scanned-pdfs) to add them.
- **A missing distillation model** is listed as optional. Without it, finished
  sessions are not distilled into episode memories. Run
  `ollama pull qwen3:4b-instruct`, or turn distillation off with
  `rhebok config set memory.distill_episodes false`.

Then try it:

- `rhebok search "something you know is in your notes"` should list the
  matching notes.
- In Claude Code, ask about something from your notes. rhebok adds the most
  relevant passages to your prompt automatically, and Claude can also search
  explicitly with the `rhebok_search` tool.
- `rhebok dashboard` shows the same things visually. The **Health** tab lists
  any install problem with its fix, and the **Search** tab shows exactly what
  rhebok found and what it passed to Claude.

If something is off, see [Install problems](#install-problems).

## Update

```bash
rhebok update
```

It downloads the newest release, checks it and installs it. If Homebrew
installed rhebok, it runs `brew update` and `brew upgrade rhebok` instead,
then `rhebok install`. The dashboard's Health tab has the same button. Restart your agents afterwards. Once a day, a
new Claude Code session checks whether a newer release exists and tells you
when one does. Nothing installs until you run `rhebok update`. An update
never downloads the optional distillation model.

`rhebok update --check` reports the installed and latest versions without
installing. If no release has been published yet, both commands say so and
exit successfully, and `rhebok status` and the dashboard show "no release
published yet" beside the version; there is nothing to fix.

The update check asks GitHub's API without logging in, which GitHub limits to
60 requests an hour per network. If you hit that limit, wait, or set
`RHEBOK_GITHUB_TOKEN` to any GitHub token, in your shell or in the `env` file
beside `config.toml` (mode 0600). The token is optional, needs no scopes, and
only raises the limit.

A rhebok built before 0.2.0 cannot unpack the release archive, which now
holds `LICENSE` and `THIRD-PARTY-NOTICES.md` beside the program. Its update
fails with "must contain exactly one file named rhebok". Install once with
Option A; later updates work as usual. From 0.2.0 on, `rhebok update` takes
the program and ignores any other regular file at the archive root, so a
release that adds a file there does not break it.

To switch the daily check off:

```toml
[update]
check = false
```

## Uninstall

```bash
rhebok uninstall            # removes rhebok, its plugin, the PATH link, and its entries in every connected agent
rhebok uninstall --purge    # also deletes everything rhebok stored: index, memories, config, golden set, backup key
```

Both refuse while an index is running. Your notes are never touched. If
Homebrew installed rhebok, finish with `brew uninstall rhebok`, which removes
the program itself.

Plain `rhebok uninstall` removes the program, the Claude Code plugin and its
cache, the `rhebok` links on your PATH, and rhebok's entries in every
connected agent. It keeps everything else, so a reinstall picks up where you
left off: the config, the index, your memories, the golden set, the backup key
and the Drive token. It prints where the config and the index are.

`rhebok uninstall --purge` asks first (`--yes` skips the question), then
deletes rhebok's whole data folder and its config. That is:

- the index and every memory, which nothing can rebuild;
- `backup.key`, the default backup encryption key. Without it, your encrypted
  backups cannot be restored;
- `config.toml`, `golden.toml` (your hand-written golden set) and the `env`
  file with an embedding endpoint token;
- the Drive token, the project trust records, the logs and the usage records.

Before you purge, keep what you want:

```bash
rhebok memory export --out memories.jsonl
```

and copy `backup.key` and `golden.toml` somewhere safe. `rhebok status` shows
where they are.

Three things stay behind either way:

- **The models.** Ollama keeps them, because other tools may use them.
  Remove them with `ollama rm qwen3-embedding:0.6b` and, if you pulled it,
  `ollama rm qwen3:4b-instruct`.
- **The backup schedule, if you set one** with `rhebok backup schedule`.
  Remove it first with `rhebok backup schedule --uninstall`.
- **The vector-search extension** that LadybugDB, rhebok's database library,
  downloaded to `~/.lbdb`. Delete that folder if no other program uses
  LadybugDB.

## Build from source

Use this on a Linux with glibc older than 2.39, or when no release build
exists for your machine.

An Intel Mac is not supported yet, from source either. The build downloads
LadybugDB's prebuilt library, and its Intel Mac version references
`___cpu_model`, which Apple's linker cannot resolve. Setting
`LBUG_BUILD_FROM_SOURCE=1` makes the build compile LadybugDB itself instead;
that route is untested.

1. Install a current stable Rust toolchain with [rustup](https://rustup.rs).
2. Install a compiler:
   - On macOS, Xcode 16 or newer, and OpenSSL from Homebrew:
     `brew install openssl@3`.
   - On Linux, GCC 13 or newer plus `libssl-dev` and `pkg-config`
     (`sudo apt install build-essential libssl-dev pkg-config` on Ubuntu).
3. Build and install:

   ```bash
   git clone https://github.com/Stellenbosch-Technologies/rhebok.git && cd rhebok
   scripts/fetch-lbug.sh
   cargo build --release && target/release/rhebok install
   ```

   `scripts/fetch-lbug.sh` downloads the LadybugDB library that rhebok is
   pinned to and checks its checksum. It needs `curl` and `python3`. Without
   it, the build downloads the same library itself, unchecked.

Then continue at [Step 2](#step-2-answer-the-installers-questions).

The linker flags rhebok needs are in `.cargo/config.toml`. If you set
`RUSTFLAGS`, cargo ignores that file and the database extensions silently
fail to load.

A source build has the same version number as the release, so `rhebok update`
reports it as current. Run `rhebok update` only when you want the release
build back.

## About the models

rhebok turns your notes into searchable vectors with one small model, run
locally through [Ollama](https://ollama.com):

```sh
qwen3-embedding:0.6b               # about 640 MB, required (the default)
embeddinggemma-2:270m-bf16-text    # about 575 MB, if you chose it instead
```

`rhebok install` downloads the one you chose after asking. With `contextual = true` in
`[embed]`, rhebok also needs the enrichment model named in
`[embed] enrich_model`, and `rhebok install` downloads that too.

A second model is optional. While sessions are indexed, memory is on and
`[memory] distill_episodes = true` (all three are the default), rhebok
summarizes each finished session into a short episode memory with:

```sh
qwen3:4b-instruct         # about 2.5 GB, optional; [memory] distill_model
```

An interactive `rhebok install` asks about it separately. `--yes`, `--quiet`
and `rhebok update` never download it. Running a 4B model also needs a few GB
of free memory while it works. To use it, run `ollama pull qwen3:4b-instruct`.
To stop distilling, run `rhebok config set memory.distill_episodes false`.
Until one of the two is done, `rhebok status` lists the model as optional,
and the background index writes a line to `db.log` naming both fixes.

To embed on another machine, such as a GPU box or LM Studio, see
[Embedding somewhere other than local Ollama](configuration.md#embedding-somewhere-other-than-local-ollama).

## Install problems

Run `rhebok status` first. It names the problem.

- **`install:` shows a failed check:** run `rhebok install` again. It repairs
  every check it can and prints the ones it cannot.
- **`command not found: rhebok`:** add the `export PATH=...` line the
  installer printed to your shell profile, then open a new terminal.
- **macOS says it can't verify rhebok, or stops it the moment it starts:**
  the file came from a browser and still has the "downloaded" mark. Run
  `xattr -d com.apple.quarantine rhebok` on the unpacked file, or use Option A,
  B or D, which never add the mark.
- **`gh release download` says "no assets match":** the release has no build
  for your machine. [Build from source](#build-from-source).
- **`curl` returns 404:** no release has been published yet;
  [build from source](#build-from-source).
- **`ollama: unreachable`:** start Ollama (open the app, or run
  `ollama serve`). With a remote embedding endpoint and `contextual` off,
  Ollama is needed only for the optional session distillation, and the
  message says so.
- **"the downloaded binary reports ... expected ...":** the release file
  doesn't match its version. Nothing was installed. Please report it.
- **"will not execute on this machine":** the release build doesn't match
  your computer. [Build from source](#build-from-source).
- **"update check failed":** most often there is no network. `rhebok update`
  works once the network is back.
- **"no rhebok release has been published yet":** there is nothing to update
  to, and nothing on your machine needs fixing.
- **"GitHub refused the request":** most often this network has used up
  GitHub's limit of 60 anonymous requests an hour. Try again later, or set
  the optional `RHEBOK_GITHUB_TOKEN`; see [Update](#update).
- **"must contain exactly one file named rhebok":** your rhebok predates
  0.2.0. Install once with Option A.
- **Moving from br8n:** rhebok used to be called br8n. Install rhebok with
  Option A, which runs `rhebok install`. It moves the br8n index, memories,
  config and env file, renames `BR8N_*` keys in the env file to `RHEBOK_*`,
  replaces the br8n Claude Code plugin, and moves each connected agent to
  rhebok. Rename any `BR8N_*` variables in your
  shell profile yourself; rhebok warns about the ones it sees. Then restart
  Claude Code and Claude Desktop. If Claude Desktop was running, its br8n
  entry is left in place and rhebok says so: quit Claude Desktop, run
  `rhebok connect claude-desktop` from a terminal, then open it again.
- **`claude-desktop` shows as stale, or a change to it does not last:** Claude
  Desktop saves its config file from memory while it runs, which undoes any
  change made meanwhile. Quit Claude Desktop (⌘Q), run
  `rhebok connect claude-desktop` from a terminal, then open it again.
- **Nothing is added to Claude's prompts after an update:** restart Claude
  Code. The running session still has the old version.
