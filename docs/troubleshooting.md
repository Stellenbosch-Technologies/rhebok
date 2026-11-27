# Troubleshooting


Start with `rhebok status`; it names most problems and their fixes.
`rhebok doctor` goes deeper: it checks the embedding endpoint, the machine's
memory, and which plugins inject context. The background indexer and the daily update check
write their output to `db.log`, beside the index
(`~/Library/Application Support/rhebok/db.log` on macOS,
`~/.local/share/rhebok/db.log` on Linux).

**No results.** The index may not exist yet, or may be empty. Run
`/rhebok-index` (or `rhebok index`) and check the reported document count.
`rhebok status` also shows document and chunk counts directly.

**A file never turns up in search.** `rhebok status` lists what the last index
skipped and why. A scanned PDF is skipped when OCR could not run; the
message names the missing library. OCR is optional and its libraries are not
bundled; see [Scanned PDFs](configuration.md#scanned-pdfs).

**Results look stale, or `rhebok status` reports a model mismatch.** The index
was built with a different embedding model than the one currently configured.
Embeddings from different models are not comparable, so `rhebok` refuses to
mix them. Rebuild with `rhebok index --reindex`.

**`store: UNAVAILABLE`, and the vector extension "is not installed".** rhebok's
database library, LadybugDB, downloads its `vector` extension from
`https://extension.ladybugdb.com/` into `~/.lbdb/extension` the first time
`rhebok index` runs. On a machine that was offline then, the index is written
but nothing can open it: `rhebok status`, search tiers 2 to 4 and the
dashboard show this message. The prompt hook still answers at its default
tier. Run `rhebok index` once with network access, or copy `~/.lbdb/extension`
from a machine with the same rhebok version and platform. If the message
instead says the extension "could not be loaded", the binary was built
without the linker flags in `.cargo/config.toml`, often because `RUSTFLAGS`
was set; rebuild without it.

**Past sessions never appear in your prompts.** That is the default. The
prompt hook weighs transcripts at 0.45, below its gate, so it injects your
notes and memories instead. `rhebok_search_knowledge` and `rhebok search` do find
sessions. See [Ranking weights](configuration.md#ranking-weights) to change
it.

**`db.log` says "distillation stopped for this run".** The session
distillation model is not in Ollama. Run `ollama pull qwen3:4b-instruct`, or
set `distill_episodes = false` under `[memory]`.

**Claude Code says rhebok is not installed.** The plugin is installed but the
`rhebok` program is not, or the plugin can't find it. Install rhebok with the
[install guide](install.md). The plugin looks for `rhebok` on the PATH Claude
Code started with, then in rhebok's own folder.

**rhebok's context arrives twice, or `rhebok status` says a second rhebok plugin is
installed.** Claude Code has rhebok's plugin from two places, for example from a
plugin directory and from `rhebok install`, and both copies run the hooks. Run
`rhebok install`: it keeps its own copy and removes the other.

**The dashboard answers `421 Misdirected Request`.** It serves only requests
addressed to `127.0.0.1`, `localhost` or `[::1]` with its own port. Open the
address `rhebok dashboard` prints. Through an SSH tunnel, the tunnel's local
port must be the dashboard's port: start it with `rhebok dashboard --port 7331`
and forward with `ssh -L 7331:127.0.0.1:7331 <host>`. Behind a reverse proxy,
make the proxy rewrite `Host` to `127.0.0.1:<dashboard port>`; see
[Security model](security-model.md#the-dashboard), which also covers `Origin`.

**Update messages.** "No rhebok release has been published yet" means there is
nothing to update to. "GitHub refused the request" usually means GitHub's
limit of 60 anonymous requests an hour; see
[Update](install.md#update).

**Everything returns empty and nothing seems to work.** Confirm Ollama is
running: `ollama list`. If it's not, `rhebok`'s embed calls fail
silently by design — the hook is built to degrade to no injection rather than
error out mid-prompt — so this failure mode looks identical to "nothing
matched." Start Ollama and try again.
