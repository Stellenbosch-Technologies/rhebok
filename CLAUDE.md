# CLAUDE.md

Local RAG plugin for Claude Code. Rust. One binary, no daemon. Embeds through
local Ollama by default, or any OpenAI-compatible `/v1/embeddings` endpoint.
LadybugDB (`lbug`) is the embedded store: it holds the documents, the chunks
with their embeddings, and the property graph. The retrieval pack published
beside it (`src/pack/`) serves vector search (usearch) and BM25, so tier 1
answers without opening the database.

## Commands

```bash
cargo test                  # 1,100+ tests, no live Ollama; the first run needs the network (see below)
cargo clippy --all-targets  # zero warnings is the bar
cargo fmt --all
cargo build --release && target/release/rhebok install   # installs THIS build over the release, hooks and all
cargo run -- dashboard --no-open  # local web dashboard; SPA sources in dashboard/, built dist/ is committed
make dev                    # cargo build --no-default-features: no backup, no OCR, far fewer crates
make test-dev               # cargo test --no-default-features
cargo test --test it        # the one merged integration binary; see "Build and test layout"
make ci                     # scripts/ci-local.sh: every ci.yml and audit.yml job on this machine, plus the mod's tests
```

MSRV is 1.95, and it is the OCR tree that sets it: `oar-ocr` 0.11 and its
sibling crates declare 1.95, reached through rhebok's own `ocr` feature. `nalgebra`, `safe_arch` and `wide` want 1.89 behind the same
feature. The crates this line used to name — `htmd`/`ignore` for edition
2024's 1.85 floor, `cxx`/`time` at 1.88 — are all still there and are all
now slack. The floor therefore moves whenever `oar-ocr` does, and the CI
`msrv` job is what catches it: it builds on exactly that toolchain rather
than on `stable`, so a declaration the tree cannot meet fails loudly
instead of passing on whatever the developer happens to have installed.

**LadybugDB is pinned and checksummed, and `cargo test` is not offline.**
lbug's build script downloads LadybugDB's prebuilt static library. With
`LBUG_VERSION` unset it downloads `releases/latest`, which is newer than the
0.19.1 crate's headers, so `.cargo/config.toml` forces
`LBUG_VERSION = { value = "0.19.1", force = true }` under `[env]`. That build
script checks no checksum. `scripts/fetch-lbug.sh` does: it downloads the
release archive for the host, checks its sha256 against a recorded value, and
unpacks it into the lbug crate's `.cache/lbug-prebuilt/version-0.19.1/lib`,
where the build script looks first, so the build downloads nothing itself. It
refuses when `Cargo.lock` locks a different lbug than the pin, or when it has
no checksum for the host, and it fetches only for the host triple. Every CI
and release job that builds runs `cargo clean -p lbug` and then this script,
and so does `scripts/build-release.sh`. `scripts/ci-local.sh` runs the script,
without the clean, once before the first job it runs that builds. The clean makes the
build script run again against the library the script just placed. Without
it, a `target/` restored by `Swatinem/rust-cache` can hold a build-script
result that points at a library in cargo's registry source directory, which
that cache does not keep. To bump lbug, change
`Cargo.lock`, `LBUG_VERSION` and the checksum table in `fetch-lbug.sh` in one
commit, and expect `scripts/third-party-notices.sh` to fail until its
LadybugDB map is updated too (see "Release archives" below).

The network is needed a second time on first use. The first store opened for
writing runs `INSTALL vector`, which downloads lbug's `vector` extension from
`https://extension.ladybugdb.com/` into `$HOME/.lbdb/extension`
(`tests/it/store.rs` issues `INSTALL vector` too). A reader with no extension
fails with `VectorExtensionMissing`, which names that directory. `rhebok
status` prints it on its store line, search tiers 2-4 fail with it, and the
dashboard shows it instead of "the index is busy". The write path reports a
failed `INSTALL`/`LOAD` on stderr and still writes the index. Tier 1 keeps
answering from the pack.

## Build and test layout

**Two cargo features, both on by default.** `backup` pulls in the S3, Google
Drive and encryption crates behind `rhebok backup`; `ocr` is
`pdf_oxide/rendering` (pure-Rust page drawing for OCR) plus the two engines in
`src/loaders/ocr/`: Apple Vision through `objc2-vision` on macOS, and
`oar-ocr` 0.11 on ONNX Runtime everywhere. PDF text itself comes from
`pdf_oxide` in every build. It is the tree that also sets the MSRV above.
`make dev` and `make test-dev` build and test with both off, which is the fast
inner loop; CI runs `cargo clippy --all-targets --no-default-features` so the
feature-off build cannot rot. A test that needs a feature is gated on it —
`tests/it/main.rs` declares every `backup_*` module under
`#[cfg(feature = "backup")]`.

**The release profile is thin LTO, `codegen-units = 1`, `strip = true`.** It
makes a smaller, faster binary, and nothing a developer runs builds it.
Stripping and LTO are exactly what can drop the symbols lbug's dlopen'd
`vector` extension resolves against the executable, and the ONLY check that
this profile still exports them is `scripts/release-smoke.sh` assertion 5 (a
`--quality 2` search, which opens the store). It runs on every packaged
release target, but for Linux, where the export-dynamic flag spelling differs,
it is the only check anywhere: the perf job and the runtime-budget gate both
stay on tier 1 and never load the extension.

**Release archives hold exactly three files, and `rhebok update` accepts
any added regular file at the archive root.** `scripts/package-release.sh` packs `rhebok`, `LICENSE` and
`THIRD-PARTY-NOTICES.md`, in that order, owned by 0/0 with no extended
headers, and checks the result. `release-smoke.sh` assertion 7 checks the
same three entries in the same order, and assertions 8-10 check their
contents. `scripts/build-release.sh <THIRD-PARTY-NOTICES.md> [<target>]`
fails without the notices file. `scripts/third-party-notices.sh` writes it
from cargo-about 0.9.3 (`about.toml`, `about.hbs`) for the Rust crates, then
LadybugDB's vendored licences, OpenSSL, the dashboard's production npm
packages and the IBM Plex OFL texts. Its LadybugDB section is an explicit map
of the libraries in lbug's `third_party` directory that `liblbug.a` links,
each to its licence source. The script fails when a mapped library is gone,
when a licence text is missing or has no licence terms, and when lbug
bundles a directory the map neither maps nor lists as unlinked. Update the
map when it fails; never bypass it, or a LadybugDB bump drops a notice
without anyone seeing. On the other side, `update::download::extract_single`
takes the one regular file named `rhebok` at the archive root and ignores
every other regular file there, so a release can add a file without every
installed binary refusing the update. It refuses an archive with no `rhebok`
or two, and any entry with a directory component, an absolute path, `..`, a
link, a directory or any other non-regular type; a pax global header is
skipped because it is metadata, not a member. The publisher side
(`package-release.sh`, smoke assertion 7) pins the contents, and the archive
is checksum-verified first, so the consumer needs no allowlist. Do not
reintroduce one: it froze the archive layout. A 0.1.0 binary accepted only an
archive holding the single file `rhebok`, so it cannot unpack these archives
and has to be reinstalled once. Adding a directory, a nested path or a link to
the archive still makes every installed updater refuse the update; only a
regular file at the root is safe to add. The app therefore ships as its own
asset, `rhebok-app-<target>.tar.gz`, because it is a directory.

**Integration tests are ONE binary, `it`, plus thirteen that must stay
separate.** `Cargo.toml` sets `autotests = false`; `tests/it/main.rs` declares
`common` by `#[path]` and one `mod <name>;` per file in `tests/it/`. Linking one
binary instead of one per file is most of what made `cargo test` quick and is what keeps
the Linux runner's link step off the disk limit. The separate `[[test]]`
targets are the twelve files that mutate process environment in-process
(`std::env::set_var`/`remove_var`) — `bench`, `dashboard`, `golden`, `index`,
`loaders_pdf`, `memory_embed_backend`, `memory_self_yield`,
`retrieve_store_wiring`, `session_start_trigger`, `status_lines`,
`transcript_deferral_cli`, `transcript_settle` — because their env locks only
serialise tests within one file, and inside `it` another file's mutation would
interleave mid-test; and `update_notice`, whose
`a_stale_check_spawns_a_detached_check_that_records_its_failure` races a
detached child by construction and flaked once in four runs under `it`'s
parallelism. A new test file goes in `tests/it/` with a `mod` line unless it
touches process-global state.

The timing probes `probe_real_sizes` and `probe_insert_scaling` are ignored by
default. Run them on demand with:

    cargo test --test it -- --ignored probe

With `autotests = false` a file cargo is not told about is never compiled, and
its tests "pass" by not existing. The CI step "Enforce the test target count
and that every test file is compiled" (`scripts/check-test-targets.py`, the
`targets` job in `ci-local.sh`) therefore fails on any `tests/*.rs` that
is not a declared `[[test]]`, on any `tests/it/*.rs` with no `mod` line, and on
more than 14 test targets. There are 14 today (`it` plus the thirteen above),
so the next separate target needs the limit raised. That 14 is deliberate
friction: raise it (`LIMIT` in the script) in the same commit that adds a
target, with the reason, never to make CI green.

**CI runs on GitHub Actions, and locally too.** `ci.yml` runs on pushes to
`main` and on pull requests; `audit.yml` runs `cargo audit` and `npm audit
--omit=dev` for the dashboard on the same events and weekly. Every workflow
declares `permissions: contents: read` (only the release publish job gets
`contents: write`), and every action is pinned to a full commit SHA; keep new
ones pinned the same way. Dependabot opens grouped weekly minor and patch updates and ignores every
major version: a major bump is done by hand, in its own pull request, after
reading the dependency's breaking changes. Its pull request titles follow the
repository's own commit style, so squash-merge them with a plain-sentence
subject. A dashboard npm update also needs `dashboard/dist` rebuilt and
committed, or the `dashboard` job fails. `scripts/ci-local.sh`
(`make ci`) runs every `ci.yml` and `audit.yml` job on this machine and prints a pass/fail
table; `--only`/`--skip` pick jobs, `--perf <base>` adds the perf comparison.
Its `audit` job is skipped when `cargo-audit` is not installed.
The `e2e` job is `dashboard/e2e/`: a Puppeteer suite that indexes a fixture
corpus against a Node stub of Ollama's `/api/embed` (a deterministic
bag-of-words vector, so search results are meaningful without a model), serves
the dashboard under a throwaway `HOME`, and runs every `specs/*.mjs`. Specs
share that one indexed sandbox and run in file order, so a spec that changes
it (connecting an agent, placing `bin/rhebok`) affects every later one; a spec
that exports `fresh = true` gets its own unindexed sandbox with no `sources`
and its own dashboard instead. A browser
console error fails the test that caused it. Screenshots land in
`target/e2e-shots/`. It has its own `package.json` so the dashboard job's
`npm ci` never downloads Chrome. The stub must run in the same event loop as
the runner, so the runner never uses `spawnSync` for `rhebok`: a blocked loop
cannot answer the embed and `rhebok index` times out.

The `plugin` job runs only locally: it is not in `ci.yml`, because a runner has
no `claude`. It runs `claude plugin validate plugin` and `claude plugin test
plugin`, which runs `plugin/hooks/rhebok-prehook.test.ts`, the only tests that
run the rhebok-prehook mod itself; it skips when `claude` is not on the PATH.
`cargo test` only reads the mod's source. Those tests are what pin that every
process the mod starts runs the launcher inside a marketplace install's own
plugin folder.

**Performance guards, and how to recalibrate them.** Two exist and neither
runs on a developer's machine by default.

- `scripts/perf-compare.sh <base> <head>` (the CI `perf` job, PRs only) builds
  a 35,000-chunk synthetic pack with each binary, then runs seven interleaved
  `--reuse` pairs. It fails on tier-1 p50 more than 15% slower in at least 5 of
  7 pairs, median reuse peak RSS or the single build peak RSS more than 10%
  higher, `pack_bytes_per_chunk` more than 10% higher, or `pack_build_ms` more
  than 25% slower. Those margins were chosen by hand, not derived from a
  measured noise floor; what is verified is only that identical binaries pass
  locally and a head with tier-1 `efs` raised to 800 fails. To recalibrate, run the script with the SAME binary as both
  arguments several times on the runner class that runs it (`PERF_PAIRS`,
  `PERF_CHUNKS`, `PERF_SEED` override the defaults) and set each threshold
  above the largest self-versus-self difference seen; a threshold inside that
  noise fails PRs at random. The job skips itself green when the merge base
  has no `bench --synthetic --reuse`.
- `scripts/runtime-budget.sh <binary> 35000 --budget scripts/runtime-budget.json`
  (the release workflow, x86_64 Linux only, in a container clamped to at most
  8 GB and 4 CPUs) measures the hook's p50 and peak RSS, startup time and
  binary size against a stub embedder, and fails on any key over its ceiling.
  The ceilings are about 2x the first Linux run (a dry run on a
  2-CPU runner, so the container got `--cpus=2`): hook p50
  93 ms, hook peak RSS 89 MB, start 4.1 ms, binary 67.6 MB. To recalibrate,
  trigger the release workflow by `workflow_dispatch` (a dry run is the default
  and publishes nothing), read the measured JSON the step prints before its
  verdict, and reset each ceiling to about 2x that value. Only keys present in
  both the budget file and the measurement are compared, so adding a key to
  the JSON is how a new metric becomes gated.

**`rhebok index` runs at nice 10** (`index::lower_priority`), so a prompt's
hook is not starved by an index on a 4-core machine. The cost: under CPU
contention the indexer is descheduled more, and it holds lbug's file lock —
which blocks every store reader — for longer.

**Every auto-committed statement is a full disk flush.** `Store::exec` runs
each statement as its own transaction, and each lbug commit ends in
`WAL::waitForDurabilityNoLock` → `fcntl(F_FULLFSYNC)` on macOS. The indexer
writes at least three statements per chunk, so on the maintainer's corpus a
fresh `--no-embed` index of 1,632 documents and 74,143 chunks spent 72% of
its 1,267 s in that call, measured with macOS `sample`. The writer loop in `index_documents_with`
now runs each document's `upsert_document` and `replace_chunks` inside
`Store::in_transaction`, and the same index took 196 s, with recall identical
at every tier. Dropping the wrapper changes no result, only time, so
`Store` counts the write statements that commit on their own
(`autocommitted_writes`, writes only: `PreparedStatement::is_read_only`
excludes reads, which never flush), and
`the_write_loop_commits_no_write_outside_a_transaction`
(`tests/it/incremental_index.rs`) asserts the loop adds none.
`BEGIN TRANSACTION`, `COMMIT` and `ROLLBACK` go through `exec_literal`
(`conn.query`), not `exec`.

The other per-row write paths are wrapped the same way, and each has a test
that asserts it adds no autocommitted write: `--compact` copies in
transactions of `COPY_BATCH` rows (`copy_snapshot`, measured 1,279 s → 233 s
over 74,143 chunks with vectors), `--backfill` sets each batch's embeddings
in one (`Store::set_chunk_embeddings`), `resolve_links` writes its edges in
one, `delete_document` deletes a document and its chunks in one, and the
memory store's same-id rewrite is one. `in_transaction` joins a transaction
that is already open instead of issuing a second `BEGIN`, because these
compose: `delete_document` runs inside `prune_missing`, and memory writes run
the indexer. A new per-row write path that does not use the wrapper costs a
full flush per statement and nothing will fail; check `autocommitted_writes`
in its test.

**`Store::exec` caches prepared statements, except the ones lbug binds at
prepare time.** Re-preparing the same Cypher for every chunk was the largest
remaining cost after the flush. On a frozen copy of the maintainer's corpus (1,757
documents, 85,376 chunks), a fresh `--no-embed` index ran 102 s and 102 s with
the cache against 194 s without it, run in the order cache, none, cache, and
peak RSS fell from 2.40 GB to 1.14-1.26 GB. The cache is keyed by the statement text,
held in a `Mutex` because the MCP server moves a `Store` across threads, and
cleared in `Store::drop` before the connection is dropped. lbug's
`PreparedStatement` is not `Send`; `CachedStatement` asserts it is, which
holds only because a cached statement is only ever executed through the
connection that prepared it, under that mutex. `is_cacheable` refuses `CALL`
statements and DDL. A `CALL` table function is evaluated when the statement is
PREPARED: with `CALL SHOW_INDEXES()` cached, `has_legacy_vector_index` kept
reporting `chunk_vec` after `drop_legacy_vector_index` had dropped it, and
`store_open_drops_a_legacy_chunk_vec_index` caught it. That drop is not
housekeeping. Older binaries built `chunk_vec`, an HNSW index on
`Chunk`, and databases they wrote still carry it. A chunk written with a NULL
`embedding` beside that index (`rhebok index --no-embed`, then `--backfill`)
makes lbug's HNSW insert read a missing vector and the process dies with
SIGSEGV inside a write transaction, which can leave an empty database.
`Store::open` (the write path, never `open_existing`) therefore checks
`SHOW_INDEXES` and drops `chunk_vec` on every open, and it must keep doing so
although nothing in this tree creates the index any more: the hazard lives
in databases already on disk. Check for the index through the catalog, never
with `strings` on the database file, because lbug never reclaims space and a
dropped index's name survives as dead bytes. So the `CALL` exclusion is not
an optimisation to tune. `indexing_more_documents_prepares_no_new_statements`
pins that the cache is used.

**The pack build's memory peak is `all_rows_for_pack`, and lbug's buffer pool
cannot be capped to fix it.** Every store opens with lbug's default pool, 80%
of physical RAM (12.8 GB on a 16 GB machine), and the one query that reads every
chunk for the pack was the whole peak: RSS checkpoints put it at +1.04 GB on a
fresh 85k-chunk `--no-embed` index and +1.15 GB on a 74k-chunk `--compact`
with vectors, against about 280 MB of records and vectors kept in Rust.
Capping the pool through `SystemConfig::buffer_pool_size` was tried at 512,
256 and 128 MB, and every pack query failed with `Buffer manager exception:
Unable to allocate memory! The buffer pool is full`, because lbug materialises
the result and sorts it in that pool. Do not cap it without streaming the
query first. What did work: `all_rows_for_pack` reads each document's `uri`,
`title`, `source_type` and `meta` once (`documents_for_pack`), reads chunks
without those columns and without `ORDER BY`, and joins and sorts in Rust.
Three alternating pairs on a frozen 86,287-chunk corpus: peak RSS 1.54-1.58 GB
before, 0.87-0.97 GB after, wall time unchanged, and a `--compact` of the same
database produced byte-identical pack files with both binaries.
`pack_rows_come_back_sorted_by_chunk_id_with_their_own_documents_fields` pins
the Rust-side sort and the join.

**A read query that projects `c.text` reads every chunk's text unless it
reaches the chunk by primary key.** lbug 0.19.1 plans `c.id IN $ids`, a second
`MATCH` over `HAS_CHUNK`, and a variable-length walk from `$seeds` alike: a
`SCAN_NODE_TABLE` of all of `Chunk` with every projected column, hash-joined
against the seed side. Only `c.id = $id` becomes a `PRIMARY_KEY_SCAN`.
`Store::expand` used to project `HIT_RETURN` straight from its two walks, so
every tier-2-4 search built a hash table of the whole corpus's text to return
at most twelve rows: a tier-3 search peaked at 310 MB on a 30k-chunk sandbox
and 735 MB on a 100k-chunk one, and the native app measured 1.4-1.5 GB on the
live index. The walks now return only `c.id`, `c.doc_id` and `c.ord`, and
`Store::hydrate` fetches the chosen ids by primary key: 152 and 231 MB on the
same two sandboxes, with byte-identical results over 1,780 expanded hits.

The walks themselves read the whole `Chunk` table too, because they started
from `s.id IN $seeds`. They now anchor each seed with `s.id = $idN` and put one
branch per seed into a single `UNION ALL` statement, and `Store::hydrate` does
the same for its ids. `UNWIND $seeds AS sid MATCH (s:Chunk {id: sid})` does
NOT use the key: on the live 20,689-chunk index it made the adjacent walk take
2.4 s and touch 119,830 rows, against about 30 ms before. The link walk is
written as fixed-length `-[r1:LINKS_TO]->` chains, one per hop count, not
`LINKS_TO*1..n`: per seed, lbug's recursive operator cost about 28 ms at one
hop and 99 ms at two, against 8 and 18 ms for the chains, on a loaded machine.
The selection rules (order, seed exclusion, per-list limit, interleaving) live
in `src/expansion.rs`. Old and new `expand` returned identical hits for 912
seed sets on the live index, at hop and limit settings (1, 5), (1, 8) and
(2, 12). `EXPLAIN <query>` through `Connection::prepare` shows the plan.
`graph_expansion_does_not_read_the_text_of_chunks_it_does_not_return` pins
both halves in a child process of its own, because peak RSS is per process.
What remains is a fixed cost of about 35 MB per expansion, the same with
128-byte or 128 KiB chunks, and macOS malloc keeping fragmented pages after
each search's open and close. The pages are bounded: malloc held 120-128 MB
for 3.5 MB live from the 25th search to the 300th, and
`malloc_zone_pressure_relief` reclaims none of it. Keeping one store open
across searches is worse, not better: footprint rose from 150 to 211 MB over
300 searches, because the buffer pool keeps the pages it read.

## Invariants that are not obvious from the code

**The dashboard answers only requests whose `Host` names it.** `handle()` in
`src/dashboard.rs` calls `host_allowed` on every request, before routing and
before a POST body is read. It serves exactly one `Host` header naming
`127.0.0.1`, `localhost` or `[::1]` with the port the listener is bound to,
in any case (a bare name only on port 80). Anything else, a missing `Host`
included, gets `421 Misdirected Request`. This is the DNS-rebinding defence.
The `Origin` check covers POSTs only and lets a request with no `Origin`
through, so without the `Host` check a rebinding page could read every GET
route: search excerpts, memories, the graph and the config. An SSH tunnel
cannot change headers, so it must use the dashboard's own port at both ends;
a different local port gets 421 on every request. A reverse proxy must
rewrite `Host` to `127.0.0.1:<port>`, and must also rewrite `Origin` to
`http://127.0.0.1:<port>` or drop it, because `origin_allowed` answers 409
to every POST whose `Origin` is not the dashboard's own. The Vite dev
proxy targets `127.0.0.1:7331` and its `changeOrigin` rewrites `Host` only,
so through `npm run dev`, with the dashboard on `--port 7331`, the GET routes
work and every POST gets 409, since the browser sends
`Origin: http://localhost:5173`.
`a_request_naming_another_host_is_refused_on_every_route_and_this_dashboards_names_are_served`
and `a_post_naming_another_host_is_refused_without_waiting_for_its_body`
(`tests/dashboard.rs`) pin it, and every raw-HTTP helper in that file sends
`Host: {addr}`.

**Every read in the dashboard's request handler retries `EINTR`.** `handle()`
sets a read timeout on each connection, and on Linux a `recv` on a socket with
`SO_RCVTIMEO` is never restarted after a signal handler runs: it fails with
`EINTR` even under `SA_RESTART` (signal(7)). `/api/install` starts child
processes, and while it does, a handled signal reaches the dashboard. It used
to treat any read error as the end of the request, so a request whose handler
thread was waiting in `read` at that moment was closed with no response. This
was the flaky Health-tab e2e test: about 1 run in 20 to 40 on Linux CI, never on
macOS. In a container, 300 to 1,500 Health bursts against the real binary failed
3 or 4 times in 10,500 requests, every one a read failing with `EINTR`; with
`read_retrying` it failed 0 times, and without `/api/install` in the burst no
read was interrupted at all. Which signal it is was not pinned: the dashboard's
only handlers are glibc's signal 33 and Rust's SIGSEGV and SIGBUS ones. Any new
read in `handle()` goes through `read_retrying`.
`a_read_interrupted_by_a_signal_is_retried_and_the_request_arrives_whole`
(`src/dashboard/http.rs`) pins it.

**Indexed text reaches a prompt as quoted data, and it cannot close the block
around it.** Whoever can write a note, a fetched page, a session transcript
or a mounted pack can put text in front of the agent. `src/budget.rs` renders
it. `frame_context` opens `<rhebok-context>` with `CONTEXT_PREAMBLE`, which
says the excerpts are data, not instructions, and may be outdated.
`escape_reserved_tags` writes `&lt;` for the `<` of any tag whose name starts
with `rhebok-`, `br8n-` or `system-reminder`, in any case and with
whitespace around the `/`. It skips every Default_Ignorable_Code_Point
(zero-width space and joiners, bidi marks and isolates, soft hyphen, BOM,
variation selectors, the combining grapheme joiner, Hangul fillers and the
Unicode tag characters U+E0000-E0FFF) plus the interlinear annotation
characters U+FFF9-FFFB, anywhere between the `<` and the end of the name,
because `trim_start` skips only White_Space and `<\u{200b}/rhebok-context>`
used to pass through. `is_ignorable` in `src/budget.rs` is that list, written
out by hand from the Unicode property; it is not every Cf character, since
several Cf characters (the Arabic number signs, for one) are visible. It also
reads fullwidth `＜`, small `﹤` and fullwidth `／` as `<` and `/`. Other angle
brackets stay, so code excerpts read normally. `quote_lines` starts every excerpt line with `> `, and `one_line`
puts titles, headings, links and memory scopes on one line, treating `\r`,
`\n`, VT, FF, NEL, U+2028 and U+2029 as breaks. Without both, a note could
carry its own excerpt header or its own `(memory: ..., saved by the user,
...)` line. `rhebok_search` and `rhebok_related` (`RhebokTools::format_hits`
in `src/mcp.rs`) use the same preamble, escaping and quoting, and so does the
plain output of `rhebok search` (`src/main.rs`), which the `/rhebok-search`
slash command has the agent read. Its `--json` form escapes every string
field and keeps its shape. `rhebok_context` returns a `hits` list beside its
block; `ContextHit::from` (`src/context.rs`) puts each title and uri through
`one_line` and the escape, since serde_json leaves `<` alone. Hits from a
mounted pack never carry memory facts (`mounted_hit` in
`src/retrieve/mod.rs`). The tags and the preamble count against `max_tokens`
through `framing_chars`, so the hook, `admitted_by_budget`, the dashboard and
bench agree on what fits. Keep `br8n-` in `RESERVED_TAG_PREFIXES`: audit and
distill still treat the former name as an injection marker. New code that
puts indexed text in front of an agent must go through these functions.
`tests/it/hook_contract.rs`, `tests/it/mcp.rs`, `tests/it/cli.rs` and
`tests/it/context_cli.rs` pin the rendering, and
`a_mounted_pack_cannot_present_its_text_as_one_of_the_users_memories` pins
the mounted-pack rule.

**Lessons say who saved them, and none claims the user's authority.**
`rhebok_remember` lets the agent write lessons itself. So the SessionStart
`<rhebok-lessons>` block (`LESSONS_HEADER` in `src/memory/mod.rs`) says the
lessons were saved in earlier sessions, are to be weighed as guidance, and
that the user's current instructions take precedence. Each lesson is one
escaped line that names its origin: `saved by the user`, `saved by Claude` or
`distilled from a session` (`saved_by`). The context block's memory line
carries the same origin. A lesson that kept its line breaks could forge a
second lesson line that claims the user saved it
(`a_lesson_cannot_forge_a_second_lesson_line_that_claims_the_users_authority`,
`tests/it/memory.rs`). `[memory] min_confidence` (default 80) is checked when
a memory is read as well as when it is written: `meets_confidence_floor`
leaves a memory Claude saved below the floor out of the lessons block, which
says how many it held back, and out of every retrieval through the memory
pack. That covers memories saved before the floor was raised. Memories the
user saved or distillation wrote are never held back, and `rhebok memory
list` still shows all of them.
`a_memory_claude_saved_below_the_confidence_floor_is_not_retrieved`
(`tests/it/retrieve_memory.rs`) pins the read-time floor.

**The MCP surface ships a transcript weight of 0.85; the hook keeps 0.45.**
Ranking weights multiply `relevance`, so a transcript scores at most its
weight. At the global 0.45 that is under both the hook gate (0.66) and the
MCP gate (0.55), so an explicit search could never return a transcript.
`shipped_surface_overrides` in `src/config/mod.rs` gives MCP
`MCP_TRANSCRIPT_WEIGHT`, and a transcript then clears the MCP gate from an
unweighted relevance of about 0.65. The hook keeps 0.45 on purpose, because
it fires unasked on every prompt. The MCP default applies only while the
global `[weights] transcript` equals the shipped value, also when an `[mcp]`
table exists without weights. A global weight the user changed reaches MCP
as before, and `[mcp.weights] transcript` always wins. Because the check
compares values, writing 0.45 into `[weights]` does not opt MCP out; only
`[mcp.weights] transcript` does. `tests/it/surface_weights.rs` pins both
surfaces. `/api/config` reports what each surface actually uses as
`surface_weights` (`Config::weights_for`), because `effective` serializes the
raw struct, where MCP's transcript reads as unset; the Settings tab shows it
in an empty surface cell, marked "shipped default". The hook gate is calibrated together with the transcript weight
(see `default_threshold`), so do not change one without measuring the other.

**A model with no preset gets gates estimated from its own scores.**
`embed::calibrate::measure` embeds the 24 built-in `PAIRS` with the real
embedder, so prefixes and width match indexing. It then places the hook gate
at `HOOK_SPREADS` and the MCP gate at `MCP_SPREADS` standard deviations from
the mean of the 552 unrelated pairs. The two constants are where Qwen3's
0.66/0.55 sit in Qwen3's own probe scores. The anchor is the unrelated side,
because every model scores the matched probe pairs about 0.88-0.91, far above
real answer chunks: a midpoint would put Qwen3 at 0.75. Each gate is then
floored at Qwen3's own on its own (the user's choice: an estimate may tighten
a gate, never loosen it), so `all-minilm` keeps 0.66/0.55. Changing `PAIRS`
moves both constants. Re-derive them and run
`the_built_in_pairs_put_qwen3_back_on_its_own_gates` (`tests/it/embed.rs`,
ignored, needs Ollama with qwen3). `setup::model::apply_custom` estimates on a
model change, and on a re-install only while the gates are still Qwen3's.
`tests/it/setup_model.rs` pins it through a bag-of-words stub Ollama.

**A fresh install pulls only what it needs, and asks separately for the
rest.** `setup::ollama::ollama_models` returns required models (the embedder
unless a remote endpoint embeds, and the enrichment model when `[embed]
contextual` is on) and optional ones (the distill model, `qwen3:4b-instruct`
by default, and only when distillation runs automatically). `Consent` pulls a
required model on `--yes` or an interactive yes. It pulls an optional model
only on an interactive yes: never under `--yes` (the `curl | sh` install) and
never under `--quiet` (how `rhebok update` and the dashboard's Update button
run `install --yes --quiet`). Those print a warning that names `ollama pull
<model>` and `distill_episodes = false` instead, so an unattended install
does not add a 2.5 GB download. Declining the distill model still pulls the
embedder, because each has its own question. `rhebok status` reports a
missing distill model as optional. A 404 from Ollama's generate call stops a
distill run with `DistillModelMissing`, which names the same two fixes. With
a remote embedder, an unreachable Ollama is reported as affecting optional
session distillation only.
`an_unattended_install_or_update_never_pulls_the_optional_distill_model`
(`tests/it/setup_ollama.rs`) and
`install_yes_quiet_as_update_runs_it_does_not_pull_the_distill_model`
(`tests/it/setup_install.rs`) pin it.

**Backups carry the memory store, and a manifest is authenticated when a key
is configured.** Memories are the one store that cannot be rebuilt from
sources. `src/backup/snapshot.rs` archives `memory/db` and `memory/pack`
together under `memory::acquire_lock`; a backup that cannot take the lock
fails rather than write a generation without them. Restore writes only
`config.toml` and `golden.toml` from the config section, never another name a
manifest lists (such as `env`). It restores the memory store when it is
missing locally, replaces an existing one only with `--memories`, renames
the replaced store to `db.before-restore-<time>` instead of deleting it, and
skips memories embedded with another model or width. `--force` never touches
memories. With a backup key, each manifest carries an HMAC-SHA256 `auth` tag
over its canonical JSON minus the tag, under a key derived with its own
label. A tag that does not verify is refused with no override. With a key
configured, an encrypted manifest with no tag, which every earlier version
wrote, needs `--allow-unauthenticated-manifest`, and a manifest that says it
is unencrypted needs `--allow-unencrypted-manifest`. Without a key, neither
check runs. Keep those two flags separate: the
second trusts every byte the bucket's owner chooses, memories included. A
manifest stored under a generation key that its own `created_at` does not
derive is refused, and every restore prints the date and host of what it
restores, because `manifest/latest.json` is not authenticated.
`MANIFEST_VERSION` is 2, so an older binary refuses these backups instead of
silently skipping the memories. Do not change the labels in
`src/backup/crypto.rs`: the key id and the manifest tag of every existing
backup derive from them. `rhebok backup schedule` writes the installed
launcher (or Homebrew's opt link) single-quoted for `/bin/sh`, with `%`
escaped for cron, and refuses a path with a line break or a schedule that is
not five cron fields or an `@` macro. `tests/it/backup_restore.rs` and
`tests/it/backup_cli.rs` pin these. Builds before 0.2.0 wrote the same
`# rhebok-backup` line unquoted, which `/bin/sh` cannot run when the path has
a space. Every `rhebok install` (and so every `rhebok update`) therefore runs
`setup::migrate::quote_unquoted_cron`, which regenerates such a line through
`cron_line` in place, keeping its schedule, program and `RHEBOK_CONFIG`,
leaves a line already in the quoted form byte-identical, and warns about one
it cannot parse. An unquoted `RHEBOK_CONFIG=<config> <program>` is split at
the one ` /` whose tail is an absolute path ending in `rhebok`; more than one
such split is a warning, not a guess. It keeps the old program rather than
switching to `paths.bin` so that an install under a throwaway `HOME` never
points the user's real crontab at a sandbox binary.

**A missing release is a result, not an error.** A 404 from GitHub's
`/releases/latest` makes `update::release::fetch_latest` return `Ok(None)`.
The check saves a record with no latest version and no error, so the next
check waits `CHECK_EVERY` (24 hours) rather than the one-hour retry that
follows a failure, and `rhebok update --check` exits 0. A 401, 403 or 429
says GitHub refused the request, most often for the anonymous rate limit.
`RHEBOK_GITHUB_TOKEN` is optional and only raises that limit. No updater
message may say the repository is private: `tests/it/update.rs` pins that.

## Conventions

**One commit per pull request.** Squash the branch before you open the PR,
and again after review fixes land. Squash with `git reset --soft <base>`, one
`git commit`, and `git push --force-with-lease`; this needs no interactive
rebase.

**No prefix on a commit subject.** Not `docs:`, not `release:`, not
`fix:`. The subject is a plain sentence that says what changed, for
example `the README carries the install guide`.

**No comments in code unless explicitly asked.** Write code that explains
itself: names that say what a thing is, functions that do one thing, no
comment standing in for a clearer name. This applies to code you write or
change. Comments already in the tree stay unless you are asked to remove
them.

