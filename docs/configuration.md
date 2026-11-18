# Configuration and tuning

## Configure

Run `rhebok status` once before writing a config — it prints the exact paths
this build of `rhebok` resolves for its config file and database. They come
from the `directories` crate and are platform-specific: on macOS the config
lives at `~/Library/Application Support/rhebok/config.toml`, not
`~/.config/rhebok/`. Don't guess the path; ask `rhebok status`.

The hook never fails over a bad config: a file it cannot parse means every
setting falls back to its default, silently. `rhebok config check` is the
strict reader. It reports syntax errors, unknown keys (with the key you
probably meant), values of the wrong type, values outside the range rhebok
accepts, and `sources` that do not exist, and exits 1 if it finds any.
`rhebok status` prints the same problems.

```bash
rhebok config path                      # where config.toml is
rhebok config check                     # every problem, with its line
rhebok config get hook.threshold        # the value in effect, default or not
rhebok config set hook.threshold 0.7    # value is read as TOML, else as a string
rhebok config set sources '["~/notes"]'
rhebok config unset hook.threshold      # back to the default
```

`set` and `unset` edit the file in place and keep your comments and ordering.
They refuse a change that would introduce a problem, write through a temporary
file and a rename, and leave the previous version beside it as
`config.toml.bak`. The dashboard's settings use the same code.

A minimal config:

```toml
sources = [
    "~/notes",
    "~/Documents/papers",
]
```

`sources` is the one setting with no default — without it there is nothing to
index but your Claude Code session transcripts (see below). Paths can be
files or directories; directories are walked recursively, honoring
`.gitignore`.

Session history from `~/.claude/projects` is indexed automatically —
`index_transcripts` defaults to `true` — because it's one of the four sources
this tool exists to search, and typically the largest one: hundreds of files
and tens of MB of transcripts on a machine used daily. Add this to decline it:

```toml
index_transcripts = false
```

Transcripts otherwise stay in the index until Claude Code's own 30-day sweep
deletes the file, so a long-lived history grows the index without limit. Set
`index_transcripts_max_age_days` to leave out anything older than that many
days, judged by the transcript file's own modification time; it defaults to
unset, which keeps every transcript regardless of age. Setting it to `0`
excludes every transcript, no matter how recent, since none can be zero days
old at the moment it is discovered. A transcript that crosses the limit is
left out of the next index run rather than deleted — turning the limit back
off, or raising it, brings it back.

```toml
index_transcripts_max_age_days = 60
```

Codex CLI sessions are indexed the same way, from `$CODEX_HOME/sessions`
(`~/.codex/sessions` when `CODEX_HOME` is unset). They get the same
ten-minute settle rule, the same `index_transcripts_max_age_days` limit, the
same ranking weight and decay as Claude Code transcripts, and finished ones are
distilled into episodes too. Each one keeps what was said by you and by Codex,
plus a one-line marker for each tool call and the first 600 characters of its
output; the model's reasoning, and the instructions and environment context
Codex injects into every session, are left out. The graph and the Health tab
say which agent a session came from. `index_transcripts = false` turns off
both agents' sessions; to decline only Codex's:

```toml
index_codex_sessions = false
```

Codex's session file format is internal to Codex and undocumented, so rhebok
reads it defensively: a line it does not recognise is skipped rather than
failing the file. Two Codex features take sessions out of that directory, and
rhebok then drops them from the index at the next run: archiving a session
moves it to `archived_sessions`, and the experimental
`local_thread_store_compression` feature rewrites sessions older than seven
days as `.jsonl.zst`, which rhebok does not read.

Some directories are skipped by default. `_templates` is the only one, and it
exists because an empty Obsidian template scores like a real note: a blank ADR
form measured 0.802 on one live vault, above the 0.66 hook gate, so a blank
form could be injected into a prompt as though it were a decision. The index
run prints how many files it skipped, so the exclusion is visible rather than
inferred from an absence.

```toml
ignore = ["_templates"]   # default; set to [] to index everything
```

The list applies to notes and to agent session files. An entry takes one of
three forms:

- **A name**, such as `_templates`, skips every file or folder with exactly
  that name, case-sensitively. `_templates2` is a different directory, and
  `Templates` is not excluded, since a vault may keep real notes under that
  name.
- **An absolute path**, such as `~/notes/drafts`, skips that file or folder
  and everything under it.
- **A glob**, any entry with `*`, `?`, `[` or `{`, is matched against the
  whole path. `*` stays inside one folder and `**` crosses folders. A glob
  with no `/`, such as `*.tmp`, is matched against each name in the path, and
  a relative glob with a `/`, such as `subagents/workflows/*/journal.jsonl`,
  matches at any depth.

A relative path such as `notes/drafts` is refused, because it is neither a
name nor a location. **If you keep notes you want to search inside a matching
directory, take the entry out**, and note that a previously indexed document
that an entry matches is removed from the index on the next run.

The dashboard's Health tab lists the files the last index run skipped, with
the reason and any operating-system error code, and can add an entry for a
file, a folder or a whole group. It shows how many listed files an entry
would hide before it is saved.

## Quality tiers

Retrieval runs at one of five tiers, each trading latency for accuracy. The
prompt hook defaults to `fast` (it fires on every turn and must stay quick);
the MCP tool defaults to `thorough` (Claude called it on purpose and can
afford to wait).

| Tier | Name | Candidates | BM25 | Graph expansion | Budget |
|---|---|---|---|---|---|
| 0 | instant | 5 | no | no | 150ms |
| 1 | fast | 20 | yes | no | 220ms |
| 2 | balanced | 30 | yes | 1 hop | 320ms |
| 3 | thorough | 40 | yes | 1 hop | 700ms |
| 4 | exhaustive | 80 | yes | 2 hops | 1600ms |

Higher tiers widen the candidate pool. They do not change how candidates are
ordered: every tier ranks by the same cosine similarity.

There is no LLM reranking stage. Ollama has no rerank endpoint, so a true
cross-encoder cannot be served, and the generative substitutes measured as
noise — `qwen3:0.6b` answers "yes" to every query/passage pair, `qwen3:4b`
rejects most of them. On the corpus it was measured on, ordering by the
cosine took `thorough` from 0.80 to 1.00 recall and 706ms to 137ms.

Budgets are ceilings, not targets — on that corpus every tier returned in
under 160ms. Each sits at least twice the measured warm floor, because a ceiling that trips on a
healthy machine degrades silently instead of catching a genuinely slow query.

Set a surface's tier explicitly in config:

```toml
[hook]
quality = 2

[mcp]
quality = 4
```

### Who injects context into prompts

rhebok ships two ways to add context to a Claude Code prompt: the
`rhebok-prehook` mod inside the plugin, and the classic `UserPromptSubmit`
command hook. One setting picks which one does it:

```toml
[prehook]
injector = "mod"      # "mod" (the default), "command" or "off"
status = "counts"     # "counts" (the default), "glyph" or "words"
```

- `mod`: the mod injects. It asks the running `rhebok mcp` server, which keeps
  the index open, so a prompt skips the process start and index open the
  command hook pays every time. The status line shows what it found after
  the plugin name, for example `2 hits · 94 ms`. In any session where the
  mod is not running (an older Claude Code, or the mod failed to load), the
  command hook injects instead, so you never silently get nothing.
- `command`: the classic command hook injects, as before the mod existed.
  This is how to switch the mod off.
- `off`: nothing is injected. `rhebok search` and the MCP tools still work.

Switch it with `rhebok config set prehook.injector command`, from the
dashboard's Settings tab, or with `/rhebok-prehook command` inside Claude
Code. It applies to every open session from the next prompt; there is no
restart. `rhebok status` prints the current choice and `rhebok doctor` warns
when more than one plugin injects context, including an older plugin from
rhebok's former names (br8n, brain), which searches every prompt twice.

`status` picks how the mod's status line reads. Claude Code already shows
the plugin name in front of it, so none of the three repeat it.

| Style    | Notes added           | Nothing relevant         | rhebok cannot answer |
| -------- | --------------------- | ------------------------ | -------------------- |
| `counts` | `4 hits · 181 ms`     | `nothing above the gate` | `unavailable`        |
| `glyph`  | `● 4 · 181 ms`        | `○ no match`             | `▲ unavailable`      |
| `words`  | `added 4 notes`       | `nothing relevant`       | `not available`      |

`words` adds the time only when a prompt took a second or more, as in
`added 4 notes · slow, 1.3 s`. In a project, `counts` and `glyph` put the
project's name first (`api · 4 hits · 181 ms`) and `words` puts it last
(`added 4 notes from api`). Change it with `rhebok config set prehook.status
words` or in the dashboard under Settings, Retrieval, Claude Code. Like
`injector`, it applies from the next prompt.

`rhebok context --json` prints what would be injected for a prompt on
stdin, which is the quickest way to see why something was or was not
added.

### Projects

A project is a name for the part of your knowledge base that belongs to
one piece of work. rhebok picks the project from the directory a prompt is
written in: the project whose `roots` entry is the deepest parent of that
directory wins.

```toml
[projects.api]
roots   = ["~/code/api"]
include = ["file://~/notes/api/**", "tag:api", "agent:claude"]
exclude = ["tag:draft"]

[projects.api.slices.adr]
include = ["tag:adr"]
```

A rule is a URI glob (`file:///notes/**`, `claude-session://**`),
`tag:<tag>`, `type:<source type>` (`markdown`, `pdf`, `web`, `transcript`)
or `agent:claude|codex`. `rhebok config check` names any rule it cannot read.

A repository can describe its own project in `.rhebok/project.toml`
(`name`, `include`, `exclude`, `slices`). rhebok ignores that file until you
review it and run `rhebok project trust <repo>`, and any later edit untrusts
it again, so a cloned repository can never choose what rhebok injects. A
project in `config.toml` wins over a repository file at the same depth.

`rhebok project show` says which project the current directory belongs to
and why, `rhebok project list` lists them, and `rhebok status` prints both.

Inside a project, every prompt searches only that project's part of the
index: vector search, keyword search and graph expansion are all limited
to it, so a note outside the project can never be injected there. A slice
is the part of a project its own rules select. The index records which
chunks each project and slice holds; `rhebok index` keeps that current, and
after you edit a project's rules `rhebok project rescope` rebuilds it in
seconds without reindexing. Until then that project's prompts get no
context and say why, rather than searching everything.

Memories follow the project too. A lesson or fact saved anywhere inside a
project's roots applies across that project, and in no other. The
project's `memories` key picks what else it sees: `both` (the default)
adds global memories, `project` keeps them out, and `global` shows only
global ones. `rhebok memory promote <id>` makes a project memory global
without changing its id, and the dashboard's Memories tab shows each
memory's project by name.

`rhebok project explain <file or uri>` lists every project and slice a
document is in or out of and the rule that decided it. `rhebok bench
--project <name>` measures recall inside one project, over the golden
cases that set `project = "<name>"`; a project with no such cases is
reported as unmeasured, never as passing.

### Claude Desktop Chat

Chat has no prompt hooks, so rhebok cannot add context there by itself. What
it can do is give Claude a tool and ask Claude to use it. With rhebok
connected to Claude Desktop (`rhebok connect claude-desktop`), run

```
rhebok project instructions api
```

and paste what it prints into a Chat Project's instructions. It asks Claude
to call `rhebok_context` before answering each message, with the project's
root as `cwd`, which keeps the answer inside that project's scope and
memories. This is best effort: Claude decides whether to make the call,
unlike the Claude Code mod, which always runs. Plugin hooks do not help
here: a plugin uploaded to Claude Desktop installs with its hooks, but
Chat never runs them. Called as a tool, `rhebok_context` keeps the hook's
gate, weights and token budget but searches with the MCP surface's time
budget, since nothing kills a tool call at the hook's five seconds; the
mod, which passes `prehook`, keeps the hook's. For the phone or the web,
driving a Claude Code session through Remote Control gets the real prehook
instead.

### Sharing a slice with someone else

`rhebok project export team:adr --out ~/share/team-adr` writes that slice as
a standalone pack another rhebok can mount: no database, nothing to re-embed.
`--dry-run` lists every document that would leave first. Session
transcripts are left out unless you pass `--include-transcripts`, because
they hold whatever was said in a session.

On the other machine, `rhebok project mount ~/share/team-adr` registers it
under `[mounts]` and a project imports it by name:

```toml
[projects.api]
roots   = ["~/code/api"]
imports = ["team:adr"]
```

A mounted slice is searched read-only beside your own index, only inside
projects that import it. It must come from a rhebok with the same embedding
model, analyzer and pack format, or `mount` refuses it. Those are
compatibility checks, not trust checks: a mounted pack's text reaches your
prompts like your own notes do, so mount only packs from people you trust.
Its hits never carry a memory line, so a pack cannot pose as your memories.
See [Security model](security-model.md). Its documents
never get usage records on your machine. `imports` also takes a local
`<project>:<slice>`. `rhebok project unmount <name>` forgets a mount and
leaves its files alone.

Imports do not chain: if `api` imports `team:adr` and `team` imports
something else, `api` sees only `team:adr`. What a project can reach is
always readable from its own `imports`. `rhebok project show` lists each
import and what it resolves to, and `rhebok doctor` warns when a project
imports more than 5 local slices or more than 2 mounts, since every mount
is another pack opened on each prompt.

### Ranking weights

Not all relevance is equal authority. A session transcript that discusses a
note is genuinely similar to a query about that note — and still the wrong
answer, because the note is the source of truth. Retrieval therefore
multiplies each hit's relevance by a per-source-type weight before gating and
ordering:

```toml
[weights]
transcript = 0.45   # default for ordering and for the prompt hook
authority  = 0.0    # opt-in link authority, see below

[mcp.weights]       # per-surface override, field by field
transcript = 0.85   # default for rhebok_search: Claude's own searches
```

Relevance is a score from 0 to 1, and the weight multiplies it before the
gate, so a transcript can score at most its weight. At 0.45, transcripts
cannot clear the hook's 0.66 gate at all — deliberate, since the hook fires
unasked. `rhebok_context` and the dashboard's search use the hook's weights
too. `rhebok_search` ships with 0.85, so a transcript with an unweighted
relevance of about 0.65 or more clears its 0.55 gate. `rhebok search` is not
gated and shows transcripts either way.

How the defaults combine:

- The MCP surface's 0.85 applies while `[weights] transcript` is the
  shipped 0.45, even if you have an `[mcp]` table without weights.
- A global `[weights] transcript` set to any other value reaches both
  surfaces.
- `[hook.weights] transcript` and `[mcp.weights] transcript` always win for
  their surface. Writing `[weights] transcript = 0.45` does not opt the MCP
  surface out; set `[mcp.weights] transcript = 0.45` for that.
- To let the hook inject strong transcripts, raise `[hook.weights]
  transcript`.

The dashboard's Settings tab shows these results. An empty hook or MCP cell
shows the weight that surface uses with the saved config, and "shipped
default" marks the MCP transcript weight while it applies.

The 0.45 default was chosen on a small golden set over a corpus that was
mostly transcripts, where it ranked notes above the transcripts that discuss
them.

`authority` (default off) lifts well-linked notes by their inbound wikilink
count. It is a bounded lift, never a penalty: an unlinked note keeps exactly
1.0, the most-linked reaches `1.0 + authority`. Measured on a 100-case golden
set, `authority = 0.3` gains 0.02-0.03 recall at the middle tiers.

### Superseded decision records are demoted — ON by default

If a note's frontmatter says `status: superseded`, its chunks are multiplied by
`superseded` before the gate. This is the one weight here that does NOT ship at
1.0:

```toml
[weights]
superseded    = 0.88   # ON by default. Set to 1.0 to switch it off.
current       = 1.0
proposed      = 1.0    # keep at 1.0 — see below
investigating = 1.0
```

The problem it solves: a decision record you replaced still matches a
present-tense question about as well as its replacement does, so both were
being injected. Measured on one live vault, a present-tense question about
build tooling returned the superseded record at 0.7420 — above the
0.66 hook gate — directly beneath the record that replaced it. At 0.88 it
reads 0.6530 and is cut.

**The useful range is narrow: roughly 0.880 to 0.889 on that vault.** Below it,
questions that are genuinely about history stop finding the old record. Above
it, the old record stays above the gate. Both bounds were measured by running
the binary 8-12 times per case, and they are corpus-specific — if you change
this number, measure your own with `rhebok bench` and `expect_absent` rather
than trusting these.

**Leave `proposed` at 1.0.** Most notes carry no `status:` key at all and land
there — 99.9% of chunks on the vault this was measured against. Anything below
1.0 demotes your whole corpus to adjust a handful of decision records.

Only `status: superseded` demotes. Every other value, and no value at all, is
treated as current.

### Embedding options

```toml
[embed]
model         = "qwen3-embedding:0.6b"
dimensions    = 512      # truncated from the model's native width, renormalized
concurrency   = 2        # parallel chunk/embed workers during indexing; default half the cores, 1 to 4
batch         = 32       # chunks per embedding request
chunk_tokens  = 512      # target chunk size (approximate: 4 chars/token)
prefix_scheme = "qwen3"  # qwen3 | nomic | e5 | gemma | plain; usually auto-detected
keep_alive    = "30m"    # how long Ollama keeps the model resident after a request
```

rhebok has presets for two models, `qwen3-embedding:0.6b` (the default) and
`embeddinggemma-2:270m-bf16-text` (Ollama 0.40 or newer). `rhebok install
--embed-model qwen3|gemma` and the Model picker in the dashboard's Settings
set `model`, `dimensions`, both `threshold`s and clear `prefix_scheme` in one
write. The gates move with the model because each scores on its own scale:
EmbeddingGemma 2 rates unrelated text around 0.70, so qwen3's 0.66 hook gate
would let every hit through. Its presets are 0.76 for the hook and 0.68 for
MCP. Any other Ollama model still works if you set these fields by hand.

Changing `model`, `dimensions`, `chunk_tokens`, or the prefix scheme
invalidates the index; `rhebok` detects the mismatch and refuses to mix
embedding spaces — rebuild with `rhebok index --reindex`.

### Low-memory machines

The embedding model is the biggest resident cost in the whole pipeline —
bigger than `rhebok` itself, which peaks around 69 MB. Ollama keeps it loaded
for `keep_alive` after every request (default `30m`; ~2.4 GB for the default
model), whether or not another prompt arrives. `rhebok doctor` warns when the
machine has under 8 GiB of total memory.

Two settings bring the footprint down:

```toml
[embed]
keep_alive = "5m"    # the model unloads sooner; a cold reload back in costs
                      # about 2s on the first prompt after it expires
contextual = false    # default — leave off, since `true` loads a second
                       # model (`[embed] enrich_model`) during indexing
```

Do not lower `[hook] quality` to save memory. It saves none that has been
measured, and the `instant` tier costs recall: 0.70 against 0.90 for the
default tier on the project's own benchmark.

`rhebok doctor` reports total memory and the configured `keep_alive` on every
run, so it is the way to check the setting took effect.

### Embedding somewhere other than local Ollama

rhebok embeds against local Ollama by default. To use an OpenAI-compatible
endpoint instead — LM Studio, vLLM, anything serving `/v1/embeddings` — create
a file named `env` beside your `config.toml` (`rhebok status` prints where that
is), readable only by you:

```bash
install -m 600 /dev/null "<the directory rhebok status shows>/env"
```

```
RHEBOK_EMBED_URL=http://gpu-box:1234
RHEBOK_EMBED_MODEL=text-embedding-qwen3-embedding-0.6b
RHEBOK_EMBED_TOKEN=your-token
```

The file holds a token, so rhebok refuses to read it unless it is mode `0600` or
tighter. rhebok reads the file itself rather than relying on your shell, because
the hook runs in a process that never sees your shell profile. A variable set
in the process environment overrides the same key in the file.

Setting `RHEBOK_EMBED_URL` switches embedding **entirely** to that endpoint:
`ollama_url` is ignored for embedding and nothing falls back to localhost, so a
failure is reported rather than silently served by a different model. Features
that run an Ollama *chat* model — episode distillation for memory, contextual
enrichment — still use `ollama_url`, so leave Ollama running if
you use them. With a remote endpoint set and `contextual` off, `rhebok
install` and `rhebok status` say that an unreachable Ollama affects only the
optional session distillation.

A remote server may unload an idle model and take longer to reload it than a
prompt can wait. rhebok loads it in the background at session start, and again
when a prompt times out; that prompt reports `loading it in the background` on
stderr and injects nothing, and the next one finds the model ready.

Run `rhebok doctor` after any change. It checks the file, the endpoint, the
token, the model, the dimensions, and whether the endpoint still agrees with
the vectors already in your index, and exits non-zero if anything fails.

`RHEBOK_EMBED_MODEL` is the name the server uses. The identity stamped into the
index stays `[embed] model` from `config.toml`, so pointing rhebok at another
server running the same model does not force a re-index. `rhebok doctor`'s
agreement check is what tells you whether it really is the same model.

### Scanned PDFs

PDFs with a text layer are read directly. Pages without one are recognized
with OCR, which is optional. OCR needs two native libraries, PDFium and ONNX
Runtime, that rhebok does not bundle; see [OCR libraries](#ocr-libraries)
below. With `ocr = "auto"` (the default), rhebok uses OCR when both
libraries are found and only on pages the detector flags — a text PDF costs
nothing and never loads the recognition machinery. Without the libraries,
text PDFs are indexed as usual and scanned pages are skipped.

Recognition does not hold up your index. `rhebok index` runs two passes: the
first reads every PDF from its text layer alone and publishes, so search works
immediately; the second runs afterwards, recognizes the pages the first pass
could not read, and republishes. A PDF that owes pages deliberately keeps its
previous fingerprint until it has been recognized, which is how the second
pass knows what to re-read. A corpus with no scanned pages never starts the
second pass at all.

```toml
[pdf]
ocr                = "auto"      # auto | off | force
ocr_min_confidence = 0.5         # drop weaker recognition rather than index noise
dpi                = 150         # rasterization resolution for pages routed to OCR
model_downloads    = "if-missing"  # if-missing | offline
```

`dpi` sits just under a cost cliff and that is why it is exposed. Detection
abandons its 960px pass once a page's longest side passes 1920px and runs a
2560px detector instead, which costs substantially more per page — an A4 page
crosses at 164 DPI and US Letter at 174. Lowering it buys less than it looks,
because only rasterization scales with DPI; `rhebok` warns once, on the run
that actually recognizes something, if your setting crosses the threshold.

`model_downloads = "offline"` refuses to fetch a missing OCR model rather than
reaching for the network. Use it with `model_dir` pointing at a model set you
placed yourself:

```toml
[pdf]
model_dir       = "/path/to/ocr-models"
model_downloads = "offline"
```

`ocr = "off"` restores the pre-OCR behaviour exactly: scanned pages are
reported and skipped, nothing is queued, and there is no second pass.

**Migrating a corpus indexed before two-pass ingestion:** run
`rhebok index --reindex` once. An ordinary run stats the corpus against the
previous run's fingerprints, and a PDF indexed by an older binary already has
a current one — so it takes the unchanged fast path, is never re-read, and the
second pass never learns it owes recognition. `--reindex` discards those
fingerprints and reads everything again.

#### OCR libraries

Recognition needs two native libraries that `rhebok` does not bundle: PDFium
and ONNX Runtime (1.17 or newer).

- **ONNX Runtime.** On macOS, `brew install onnxruntime`. On Linux, download a
  release archive from
  [microsoft/onnxruntime](https://github.com/microsoft/onnxruntime/releases)
  and copy `libonnxruntime.so*` from its `lib` folder into `/usr/local/lib`.
- **PDFium.** There is no common package. Prebuilt libraries for macOS and
  Linux are published at
  [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases);
  copy `libpdfium.dylib` (macOS) or `libpdfium.so` (Linux) from its `lib`
  folder into one of the folders below.

rhebok looks for them itself, because the system loader does not search
these folders by default:

- macOS: `libpdfium.dylib` and `libonnxruntime.dylib` in `/opt/homebrew/lib`,
  `/usr/local/lib` and `/usr/lib`.
- Linux: `libpdfium.so` and `libonnxruntime.so` in `/usr/local/lib`,
  `/usr/lib`, `/usr/lib/x86_64-linux-gnu`, `/usr/lib/aarch64-linux-gnu` and
  `/usr/lib64`. If only a versioned file such as `libonnxruntime.so.1.20.1`
  is there, rhebok uses the highest version it finds.

If yours live elsewhere, set `PDFIUM_LIB_PATH` and `ORT_DYLIB_PATH` (these
win), or:

```toml
[pdf]
pdfium_lib_path = "/path/to/libpdfium.so"          # .dylib on macOS
ort_dylib_path  = "/path/to/libonnxruntime.so"     # .dylib on macOS
```

`rhebok status` has an `ocr:` line. When the libraries are missing, it says
OCR is optional and not installed, names the missing one, and says text PDFs
are still indexed. It is information, not a failed check; set `ocr = "off"`
to hide it. Once both are found, you can see OCR will run before a scan goes
missing from search rather than after.

Without them, `rhebok` says so once and falls back to text-layer extraction for
the rest of the run — scanned pages are skipped and reported, exactly as they
were before OCR existed. The first recognized document also downloads a model,
which takes around twenty seconds.

### Calibrating the injection threshold

The prompt hook only injects a match when its relevance clears a
threshold — 0.66 by default, calibrated against a golden set of a few
hundred cases. The right number depends on your embedding
model and your own documents, not on this README. Run:

```
rhebok bench
```

against your actual corpus (or `/rhebok-bench` from Claude Code) and set
`threshold` from what it reports. When relevant and irrelevant results overlap
it says so and keeps the default rather than inventing a number:

```toml
[hook]
threshold = 0.80
```

### The golden set

`rhebok bench` measures against a golden set — queries, and the document each one
should find — at `golden.toml` beside your config, or wherever `RHEBOK_GOLDEN`
points. You do not have to write it by hand:

```
rhebok golden init     # a starter set, seeded with real documents from YOUR index
rhebok golden check    # every problem in it, reported at once
```

`init` is worth using even if you would rather write the file yourself, because
the expensive part is not the schema: every case must name a uri that resolves
in your own index, and one that does not scores 0 forever while looking exactly
like a retrieval miss. `init` takes up to 25 real uris and titles from the
index, spread across it, and writes them as commented-out stubs. The file
therefore parses to zero cases and `rhebok bench` refuses it until you fill some
in — deliberately, because a case with an empty query is a valid case that gets
scored, and on a small corpus an empty query scores as a HIT.

`check` reports the faults that make a measurement mean nothing rather than mean
less: a uri that is no longer in the index, two cases asking the same question,
a case that names no document, a query that repeats its own target's title (the
title was indexed, so such a query is scored on string matching and keeps
passing after a change that broke everything else), and too few `expect_none`
cases to calibrate the threshold above. Read its last line: when a check did not
run — an index it could not open, or a file holding no cases — it says the
result is NOT a clean bill of health.

Keep adding cases as retrieval misses things. One caution: a recall figure
belongs to the golden set AND the corpus it was measured against, so editing
either makes the previous number incomparable. `rhebok bench` records a hash of
the file with every run and tells you when it moved.
