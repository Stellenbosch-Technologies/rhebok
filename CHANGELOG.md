# Changelog

## 0.2.3 — 2026-11-26

### Scanned PDFs

- **OCR reads every scanned PDF.** rhebok now reads PDFs with
  [pdf_oxide](https://crates.io/crates/pdf_oxide) in place of pdf-inspector.
  pdf_oxide decides page by page whether the text layer can be trusted, and
  draws the pages that need OCR, so PDFium is no longer needed.
- **Apple Vision is the OCR engine on macOS.** It is part of macOS, so it needs
  no library and no model download. Elsewhere the engine is oar-ocr with the
  PP-OCRv6 models. These need ONNX Runtime, and are downloaded (about 31 MB,
  checked by SHA-256) the first time a scanned page is read. On 27 labelled
  scanned pages, word F1 was 0.943 with Vision and 0.903 with oar-ocr, against
  0.875 with the old engine. Vision took 0.09 s per page against 1.18 s, and
  peak memory fell from 1.8 GB to 550 MB.
- **A page with text and pictures of text keeps its text** and gains the OCR
  lines that its text does not already contain.
- **One page that fails costs only that page.** Before, it turned OCR off for
  the rest of the run.
- **Any OCR program or service can read the scanned pages.** With
  `ocr_engine = "command"`, rhebok runs `ocr_command` on a PNG of each page.
  With `ocr_engine = "http"`, it POSTs the PNG to `ocr_url`, with
  `RHEBOK_OCR_TOKEN` as a bearer token when it is set. Either one can answer in
  plain text or in JSON lines with boxes and confidences, and
  `ocr_timeout_secs` (default 120) limits each page. The dashboard cannot change
  `ocr_command`, because it runs a program. See "Your own OCR engine" in
  `docs/configuration.md`.
- **`pdfium_lib_path` is gone.** `rhebok config check` reports it as an
  unknown key; remove it from `config.toml`.
- **PDFs that are already indexed keep the text that the old reader got from
  them** until they change. Run `rhebok index --reindex` to read them again
  with the new reader and engines.

### Restart

- **`rhebok restart` restarts a running dashboard and index on the installed
  binary.** After an update, both keep running the old binary until they are
  restarted. A dashboard comes back on the same port, so an open browser tab
  keeps working. An index starts again with the same arguments; what it had not
  yet published is lost, and the live index is unchanged. Restart leaves any
  other holder of the index lock, such as a backup, running, and refuses while
  an update runs.
- **The Health tab's version card has a Restart button**, in the web dashboard
  and in Rhebok.app. When an index is running, the button first says what the
  restart costs.

### Search

- **Graph expansion reads only the chunks it returns.** The quality tiers that
  use the graph (2 to 4) used to load the text of every chunk in the index for
  each search. Peak memory for one of those searches fell from 735 MB to
  231 MB at 100,000 chunks. Results are unchanged.
- **The MCP server searches at the tier the config names now**, also after
  that config changes while the server runs. Before, moving the hook tier from
  0-1 to 2-4 made every prompt fail with `requires graph expansion, which
  needs an open store` until the server restarted.
- **An Ollama that fails to embed is no longer reported as a missing model.**
  Only a 400 or 404 says to run `ollama pull`. A 500 or 503 says that Ollama
  is up but failed, and to check its log.

### Native dashboard (macOS)

- **Rhebok.app ships with each release.** `rhebok install` and `rhebok update`
  place it in `~/Applications`, so you no longer build it from source.
  Homebrew, `install.sh` and the dashboard's Update button place it too.
  The app is signed ad hoc and not notarized.
- **`rhebok app` starts `~/Applications/Rhebok.app`.** If it is missing, the
  message names the fix: `rhebok install` in a release build, `make install`
  in a build from source. On Linux it says the app is macOS only.
- **A build from source never downloads the app.** `rhebok install` warns and
  points to `make install`. The app finds a Homebrew rhebok on its own.
- **`rhebok uninstall` removes `~/Applications/Rhebok.app`.** It removes only
  a bundle that rhebok placed.
- **The app's version message names the fix.** When the app is older than
  rhebok, it says to run `rhebok install`.

## 0.2.2 — 2026-11-25

### MCP

- **Every tool returns structured output.** All seven tools declare an
  output schema and return their answer as `structuredContent` beside the
  text reply. An agent that reads only the text sees what it saw before.
- **New `rhebok_memories` tool.** It lists saved memories by kind and scope,
  newest first, with their ids. It reads only the memory pack, so it works
  while the embedding server is down.
- **`rhebok_search` says why it could not search.** A missing index, an empty
  index, an embedding server that is not reachable and a timeout each get
  their own reply and next step. Before, every one of them read "No matching
  notes found", which told the agent the user had no notes on the subject.
  `rhebok_related` is fixed the same way.
- **Tool descriptions say when to use each tool and when not to**, and
  `rhebok_context` no longer opens with "Call this FIRST".

### Native dashboard (macOS, build from source)

- **`rhebok-app` is a native macOS dashboard** with every tab of the web
  dashboard: Health, Search, Memories, Integrations, Settings, Graph and
  first-run setup. It looks and works like the web one, and the web
  dashboard is unchanged. Both now run on one shared core.
- **The release archives do not include it yet.** Build it from `app/` with
  `cargo build --release` (Rust 1.98 or newer) and run
  `app/target/release/rhebok-app`.
  `rhebok app` starts it only when `rhebok-app` sits beside the `rhebok`
  binary or in rhebok's `bin/` folder.

### Distribution

- **Each release ships an MCP bundle**, `rhebok-mcp.mcpb`, and publishes
  rhebok to the official MCP registry as
  `io.github.Stellenbosch-Technologies/rhebok`. The bundle runs an installed
  rhebok when there is one.
- **The plugin has an icon** (512 pixels square) and links to the docs and
  the issues.
- **`rhebok install` copies binary plugin files unchanged.** It used to read
  every plugin file as text, which corrupted the icon.

### Security

- **`url` is 2.5.9**, which fixes RUSTSEC-2026-0592, a stack overflow on
  nested `blob:` URLs.
- Dependency updates for cargo, the dashboard, the e2e suite and the
  release workflow's cosign installer.

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
