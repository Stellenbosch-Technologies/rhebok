# Changelog

## 0.2.1 — 2026-11-20

### Install

- **Homebrew.** `brew install stellenbosch-technologies/tap/rhebok`, then
  `rhebok install`. Under Homebrew, `rhebok update` runs `brew upgrade` and
  `rhebok uninstall` leaves the keg alone.
- **Pick the embedding model.** A first interactive install asks which model
  to use: Qwen3-Embedding 0.6B (the default) or EmbeddingGemma 2.
  `--embed-model qwen3|gemma` chooses without asking, and the dashboard's
  Settings tab has the same picker. Each model carries its own relevance
  gates. EmbeddingGemma 2 needs Ollama 0.40 or newer, and install and
  `rhebok status` say so on an older Ollama.
- **Or paste a link.** The same question, and `--embed-model`, take an Ollama
  library link (`https://ollama.com/library/nomic-embed-text`), a Hugging
  Face link (pulled as `hf.co/<org>/<repo>`, GGUF repositories only), a bare
  Ollama model name, or the URL of an OpenAI-compatible embedding server such
  as LM Studio. For a server, install lists the models it serves and asks for
  a token, which is now optional. The vector length is read from the model
  itself.
- **Gates for a model with no preset.** Install embeds 24 built-in sentence
  pairs with the chosen model and sets the hook and MCP gates from how it
  scores unrelated text. They are never set looser than Qwen3's 0.66 and
  0.55. Install says the estimate is coarse and that `rhebok bench` with a
  golden set is the real calibration. A model that barely tells related text
  from unrelated text gets a warning.
- **EmbeddingGemma 2's hook gate is 0.82**, up from 0.76, which let unrelated
  notes into the prompt.
- **The required model download defaults to yes** (`[Y/n]`). The optional
  model, connecting an agent and purging data still default to no.
- **Claude Code is found where its installer puts it**, in `~/.local/bin`,
  when that folder is not on `PATH` yet.
- **The summary points at the warnings.** An install that printed warnings
  ends with "Installed, but read the N lines marked ! above". A declined
  optional model is advice, not a warning.
- **Re-installing reports the gates `config.toml` holds**, not the preset's.
- **The install guide names `curl` and `zstd`**, which Ollama's Linux
  installer needs and a minimal Linux does not have.

### Retrieval

- **Tier 4 keeps its answers in the top five.** `exhaustive`, the tier MCP
  and `rhebok search` use, diversifies at `mmr_lambda` 0.7 instead of 0.5. At
  0.5 it pushed the right answer from the top five to positions six to nine
  in 7 of 179 measured questions.

### Dashboard

- **The Rhebok brand.** New palette, type and icon.
- **The Health tab** shows what the index holds, recall and search time per
  tier, and why each skipped file was skipped. **Index now** starts an index
  and shows its progress.
- **The status line** no longer repeats "rhebok". `[prehook] status` picks
  `counts` (the default), `glyph` or `words`.

### Fixes

- **Claude Desktop's config is left alone while Claude Desktop runs.** The
  running app saves its config back over any change, so connect, disconnect
  and uninstall now refuse and say to quit it first.
- **Two backups in the same millisecond** no longer share a name, so the
  second no longer overwrites the first.
- **`rhebok status` says Ollama is reachable** when only the optional model
  is missing.

### Known issue

- EmbeddingGemma 2 (`embeddinggemma-2:270m-bf16-text`) cannot be pulled on
  Linux, where Ollama says it needs MLX. Use Qwen3 on Linux.

### Community

- Questions and ideas go to
  [Discussions](https://github.com/Stellenbosch-Technologies/rhebok/discussions),
  bugs to issues.

## 0.2.0 — 2026-11-12

br8n is now **rhebok**, and lives at `Stellenbosch-Technologies/rhebok`. This
is the first release under the new name. It covers everything since br8n
0.1.0.

### Moving from br8n

- **One command moves an existing install.** `rhebok install` moves the br8n
  data and config directories (index, memories, config, env file) to their
  rhebok names, renames `BR8N_*` keys in the env file to `RHEBOK_*`, removes
  the `br8n@br8n` Claude Code plugin and marketplace, removes the old `br8n`
  link on PATH, moves every connected agent's `br8n` MCP server and Codex
  guidance block to `rhebok`, and rewrites a scheduled br8n backup. It refuses
  while a br8n index is running, and it never overwrites a rhebok directory
  that already exists.
- **Old files still read.** Backups written by br8n restore, a repo's
  `.br8n/project.toml` is still found and stays trusted, and `rhebok
  audit-injections` still counts `<br8n-context>` blocks.
- **Renamed for you:** the binary, the `RHEBOK_*` environment variables, the
  `rhebok_*` MCP tools, the `/rhebok-*` commands and skills, and the
  `<rhebok-context>` and `<rhebok-lessons>` tags.
- A br8n binary cannot update itself to rhebok. Install rhebok once with the
  one-line installer, then run `rhebok install`.

### Projects, the prehook mod and Claude Desktop Chat

- **Projects.** A project in `config.toml`, or a trusted
  `.rhebok/project.toml` in a repository, limits the prompts written in its
  directories to its own part of the index. Memories belong to the project
  they were saved in; `rhebok memory promote` makes one global. A project or
  slice can be exported as a read-only pack and mounted on another machine.
  `rhebok project show`, `explain`, `rescope` and `bench --project` explain
  and measure it.
- **The prehook mod.** The Claude Code plugin carries a `rhebok-prehook` mod
  that injects context through the running MCP server, so a prompt skips the
  process start the command hook pays. `[prehook] injector` (or
  `/rhebok-prehook`) picks the mod, the command hook, or neither.
- **Context in Claude Desktop Chat.** `rhebok context` and the new
  `rhebok_context` MCP tool return exactly what the prompt hook would inject.
  `rhebok project instructions <name>` prints text for a Chat Project that
  asks Claude to call it before each answer.

### Install and update

- **The plugin installs from a marketplace.**
  `/plugin marketplace add Stellenbosch-Technologies/rhebok` offers rhebok
  inside Claude Code. Until the program is installed, the plugin says so and
  does nothing else.
- **Models.** `rhebok install` asks once to pull the embedding model
  (`qwen3-embedding:0.6b`, about 640 MB). The session distillation model
  (`qwen3:4b-instruct`, about 2.5 GB) is optional and gets its own question,
  only in an interactive install. `--yes`, `--quiet` and `rhebok update` never
  pull it; they print how to pull it, or how to turn distillation off.
  `rhebok status` reports a missing distillation model as optional.
- **OCR is optional and says so.** OCR needs PDFium and ONNX Runtime, which
  rhebok does not bundle. On Linux, rhebok now looks for `.so` files in the
  usual library directories, including versioned ones. When the libraries are
  missing, `rhebok status` says text PDFs are indexed as usual and only
  scanned pages are skipped.
- **A missing vector extension is named.** If LadybugDB's `vector` extension
  was never downloaded (it needs network access on the first `rhebok index`),
  `rhebok status`, the dashboard and search say so and how to fix it, instead
  of reporting a busy index.
- **Update messages.** When no release has been published, `rhebok update`
  and `rhebok update --check` say so and exit 0. When GitHub refuses the
  request, the message names its rate limit for anonymous requests.
  `RHEBOK_GITHUB_TOKEN` is optional and only raises that limit.
- **Licence notices ship with the binary.** Every release archive holds
  `rhebok`, `LICENSE` and `THIRD-PARTY-NOTICES.md`, the licences of the Rust
  crates, LadybugDB's bundled libraries, OpenSSL, the dashboard's packages and
  its fonts. A rhebok binary built before this change cannot unpack such an
  archive, so `rhebok update` fails with "must contain exactly one file named
  rhebok". Reinstall once with the one-line installer. From 0.2.0 on, the
  updater takes the file named `rhebok` and ignores any other file at the
  archive root, so a later release can add a file without that break.
- **Homebrew-aware.** A rhebok that runs from a Homebrew keg leaves the
  program to Homebrew: `rhebok update` runs `brew upgrade`, and `rhebok
  uninstall` leaves the keg alone. No Homebrew tap is published yet.

### Security

- **The dashboard refuses a foreign `Host`.** Every request must name
  `127.0.0.1`, `localhost` or `[::1]` with the dashboard's own port, or it
  gets `421 Misdirected Request`. This blocks DNS rebinding, which could
  otherwise read the dashboard's search, memories and settings from a web
  page. A reverse proxy in front of the dashboard must keep `Host` as
  `127.0.0.1:<port>`.
- **Retrieved text stays inside its block.** The `<rhebok-context>` block,
  and `rhebok_search` and `rhebok_related` results, start with a fixed line
  saying the excerpts are data, not instructions, and may be outdated. Every
  line of excerpt text starts with `> `. A tag in retrieved text whose name
  starts with `rhebok-`, `br8n-` or `system-reminder` has its `<` escaped as
  `&lt;`, so a note cannot close the block early, even with invisible
  characters such as a zero-width space inside the tag. The same applies to
  the `hits` list of `rhebok_context` and to `rhebok search` output, which the
  `/rhebok-search` command reads.
- **Lessons no longer claim your authority.** The `<rhebok-lessons>` block
  says lessons are guidance and that your current instructions take
  precedence. Each lesson, and each memory line in the context block, says
  who saved it: you, Claude, or session distillation.
- **The confidence floor applies when memories are read.** A memory Claude
  saved below `[memory] min_confidence` is no longer shown or retrieved, even
  if it was saved before the floor was raised. `rhebok memory list` still
  shows it.
- **Mounted packs cannot pose as memories.** Hits from a mounted pack never
  carry a memory line.
- **[Security model](docs/security-model.md)** describes what can reach your
  prompts and how to limit it, and [SECURITY.md](SECURITY.md) says how to
  report a vulnerability.
- **Release builds** use a pinned LadybugDB library checked against a
  recorded checksum, and every workflow pins its actions to a commit.
  `cargo audit` and `npm audit` run on every pull request and weekly.

### Backups

- **Memories are backed up.** Every backup now includes the memory store.
  `rhebok restore` restores it when there is no local one; `--memories`
  replaces an existing one, which is kept beside it as
  `memory/db.before-restore-<time>`. `--force` still only overwrites
  `config.toml` and `golden.toml`. If the backup's embedding model or
  dimensions differ from your config, the memories are skipped with a warning
  that says how to restore them.
- **Manifests are authenticated.** With a backup key, each manifest carries
  an HMAC-SHA256 tag, and a restore refuses a manifest that was altered,
  renamed, has no tag, or claims to be unencrypted. Every restore prints the
  date and host the backup was written on. `--allow-unencrypted-manifest`
  accepts a manifest that says it is unencrypted while a backup key is set;
  use it only for a target nobody else can write to.
- **Upgrade note:** a restore of an encrypted backup made by an earlier
  version needs `--allow-unauthenticated-manifest` once, or run one new
  `rhebok backup` first. The manifest version is now 2, so an older rhebok
  refuses to restore backups this version writes.
- Restore writes only `config.toml` and `golden.toml` from a backup's config
  files, never any other file a manifest names.
- **`rhebok backup schedule` writes a quoted command** that runs the
  installed program, so a path with a space (the default on macOS) works.
  A schedule that is not five cron fields or an `@` macro is refused.
  `rhebok install`, which `rhebok update` runs, quotes a schedule an earlier
  version wrote unquoted, keeping its time, program and config. A
  commented-out line is left alone. A live line it cannot read is left alone
  with a warning to run `rhebok backup schedule` again.

### Search, indexing and memory

- **Claude's own searches can find past sessions.** `rhebok_search` now
  weighs transcripts at 0.85 by default, so a strong match from a past
  session can clear its gate. The prompt hook keeps transcripts out by
  default, because it fires without being asked.
- **An edit to a note's frontmatter alone is re-indexed.** Tags, `title` and
  `status` changes used to be skipped until a full `--reindex`.
- **Keyword-only search ranks by keyword score.** On an index built with
  `--no-embed`, results came back in chunk-id order; they now follow the
  BM25 ranking.
- **Faster, lighter indexing.** Each document is written in one transaction,
  and every write path commits in batches, so a full index and `--compact`
  spend far less time waiting on the disk. A pack build reads each document
  once, which cuts its memory peak by about 40%.
- **Readers no longer write.** Search, status and the dashboard open the
  index read-only, so a prompt hook killed mid-search can no longer leave the
  index unopenable.
- **The index summary counts removed documents.**
- **Distillation** defaults to `qwen3:4b-instruct` and refuses a summary the
  model did not finish. If the model is missing, the background run stops
  with a message that names the fix.
- **The MCP server** reports its own name and version and annotates each
  tool, so a client can tell the read-only searches from `rhebok_forget`.
- **The dashboard bundles its fonts** (contributed by PandaHUN777). It no
  longer loads them from Google Fonts, so opening it makes no request off your
  machine.

Release builds are for macOS on Apple silicon and Linux on x86_64 or arm64
(glibc 2.39 or newer). There is no Intel Mac build. On Windows, use the Linux
build inside WSL; native Windows support is planned.

## br8n 0.1.0 — 2026-09-27

The first public release of br8n: local, semantic search over your notes,
papers and past agent sessions, injected into your coding agent's prompts.

- **Search your own knowledge base.** Indexes Markdown, PDFs (with OCR for
  scanned pages), web pages and your past Claude Code and Codex sessions into
  a local vector, keyword and graph store. Re-indexing is incremental, so
  only changed files are read again.
- **Automatic context and on-demand search.** A prompt hook injects the most
  relevant passages when they clear a confidence gate, and an MCP tool lets
  the agent search whenever it decides to. Five quality tiers trade latency
  for recall.
- **Works with your agents.** `br8n connect` sets up Claude Code, Codex,
  Gemini CLI, Cursor and Claude Desktop, and `br8n disconnect` removes only
  what br8n added.
- **Memory.** Lessons and facts that carry across sessions, added by you or
  distilled from finished sessions, shown at the start of each session.
- **Dashboard.** `br8n dashboard` gives a knowledge graph, a search view that
  shows exactly what was injected and why, a health view with fixes for any
  problem, first-run setup, and settings for everything in the config file.
- **Local by default.** Embeddings run through Ollama on your machine, or
  through any OpenAI-compatible endpoint you point it at.
- **Encrypted backups** of your config and index to your own S3 bucket or
  Google Drive, on a schedule if you like.
- **Installs and updates itself.** `br8n install`, `br8n update` and
  `br8n uninstall`, plus a one-line installer.

Release builds are for macOS on Apple silicon and Linux on x86_64 (glibc 2.39
or newer). On Windows, use the Linux build inside WSL; native Windows support
is planned. Other platforms can build from source.
