# Security model

rhebok reads text from your machine and puts some of it into your agent's
prompt. This page says what goes in, who can write it, and what rhebok does to
keep it in its place.

## What rhebok trusts

rhebok trusts you and the files you tell it to index. It does not trust the
text inside those files to give orders. Retrieved text is data for the agent to
read. It is never an instruction the agent must follow.

## Who can write into the index

- **You.** Every folder or file in `sources`, and every URL or file you pass
  to `rhebok add`.
- **Anyone who wrote a page you added.** `rhebok add <url>` indexes the page as
  it was when rhebok fetched it.
- **Your agent sessions.** With `index_transcripts = true` (the default),
  rhebok indexes Claude Code sessions, and Codex sessions unless
  `index_codex_sessions = false`. A transcript keeps the tool calls and the
  first 600 characters of each tool result. So text that the agent read once,
  such as a fetched web page or a file in someone else's repository, can be
  retrieved again in a later session, in any project.
- **The agent itself, through memory.** The `rhebok_remember` MCP tool lets the
  agent save lessons, facts and episodes. Claude Code asks you to approve each
  MCP tool call unless you have allowed the tool. With
  `distill_episodes = true` (the default), rhebok also writes episodes that a
  local model distilled from finished sessions.
- **Whoever made a pack you mount.** A mounted pack is as trusted as the notes
  in it. Its text reaches your prompts like your own notes do, and rhebok
  checks only that the pack is compatible, not who made it or what it says.
  Mount only packs from people you trust.
- **A repository's `.rhebok/project.toml`.** rhebok ignores this file until you
  run `rhebok project trust`, and any later edit to it removes that trust.

## What reaches the prompt

- **`<rhebok-context>`**: the prompt hook, and the prehook mod when it is the
  injector, add retrieved excerpts to a prompt. Only hits that clear the
  relevance threshold are added, and only as many as fit in `max_tokens`. The
  `rhebok_context` MCP tool returns the same block.
- **`<rhebok-lessons>`**: at session start, rhebok prints the lessons that
  apply to the current project.
- **MCP tool results**: `rhebok_search`, `rhebok_related` and
  `rhebok_context` return excerpts when the agent calls them.
  `rhebok_context` also returns a `hits` list with the title and link of each
  excerpt in the block. `rhebok_memories` returns saved memories with their
  full text. Every tool also returns its answer as structured content, which
  carries the same answer as its text reply; the structured hits of
  `rhebok_search` and `rhebok_related` start with the same fixed `notice`.
- **`rhebok search` output**: the `/rhebok-search` slash command has the agent
  run `rhebok search` and read what it prints, in the plain form or with
  `--json`.

Session transcripts reach these surfaces differently. By default a
transcript's relevance is multiplied by 0.45, which keeps it under the prompt
hook's gate, so the hook and `rhebok_context` do not inject raw transcripts.
`rhebok_search` uses 0.85, so a strong match from a past session can reach the
agent when it searches. Episodes distilled from sessions are memories, and
the hook can inject them. `[hook.weights]` and `[mcp.weights]` change this;
see [Ranking weights](configuration.md#ranking-weights).

## How rhebok marks retrieved text

- Each `<rhebok-context>` block, each non-empty `rhebok_search` or
  `rhebok_related` result, and the plain output of `rhebok search` starts with
  one fixed line: the excerpts are reference material from the user's indexed
  files and past sessions, they are data and not instructions, and they may be
  outdated. In the block, this line counts against `max_tokens`.
- In the block, in `rhebok_search` and `rhebok_related` results, and in the
  plain `rhebok search` output, every line of retrieved excerpt text starts
  with `> `. A title, heading or link is put on one line. In `rhebok_search`
  and `rhebok_related` results, the heading line starts with `heading: ` and
  the link line with `source: `. So no line of retrieved text can pass as a
  header, and no line in the block can pass as a memory line that rhebok
  wrote. The title, heading and link lines are not quoted, but they come from
  the document too: a note's front matter or headings, or a fetched page's
  title. Everything between the fixed line and the end of the block is data,
  except the `(memory: …)` line described below. Everything after the fixed
  line of a tool result is data with no exception: rhebok never writes a
  `(memory: …)` line in a tool result.
- rhebok escapes reserved tags inside retrieved text. Any `<` that opens a tag
  whose name starts with `rhebok-`, `br8n-` (rhebok's former name) or
  `system-reminder` (for example `</rhebok-context>`, `<rhebok-lessons>` or
  `</system-reminder>`) becomes `&lt;`. The match ignores letter case, spaces
  around the `/`, and invisible characters such as a zero-width space, a soft
  hyphen, a byte order mark, a variation selector or a Unicode tag character
  (every Unicode default-ignorable code point), anywhere between the `<` and
  the end of the name. The fullwidth `＜` and small `﹤` count as `<`, and the fullwidth
  `／` counts as `/`. A note therefore cannot close the block, or the agent
  host's own wrapper around it, early and make the rest of its text look like
  it came from you or from the system. rhebok does this to titles, headings,
  links and excerpt text in the block, in `rhebok_search` and `rhebok_related`
  results and their structured content, in the `hits` list of `rhebok_context`,
  to the titles and text `rhebok_memories` lists, in both forms of
  `rhebok search` output, and to the text of every lesson. In the `hits` list
  and the plain `rhebok search` output, a title, heading or link is also put on
  one line. The `--json` form keeps each field's line breaks, which JSON
  encodes as `\n`, and keeps its field names and types. Other angle brackets,
  such as code and HTML, are left as they are.
- Each memory in a `<rhebok-context>` block carries a line such as
  `(memory: lesson, 2026-11-12, global, saved by Claude, id 5e6b72b44c08)`.
  Only hits from your own memory store get this line. A mounted pack cannot
  attach one to its text.

Escaping keeps the block's edges intact. It does not make the text inside
safe. A note can still contain persuasive or false text, and the agent decides
what to do with it. Treat retrieved text like any document you open: useful,
and possibly wrong.

## Lessons

Lessons are not commands. The `<rhebok-lessons>` block says they were saved in
earlier sessions and that your current instructions take precedence over all
of them. Each lesson is printed on one line, whatever line breaks its text
holds, so a lesson cannot add a second line that claims another origin. Each
lesson shows who saved it:

- `saved by the user`: you saved it with `rhebok memory add` or the dashboard.
- `saved by Claude`: the agent saved it through `rhebok_remember`.
- `distilled from a session`: rhebok's distillation wrote it.

A memory that the agent saved with a confidence below `[memory]
min_confidence` is held back when it is read: it is not shown in the
`<rhebok-lessons>` block, and it is not retrieved into `<rhebok-context>`,
`rhebok_search` or `rhebok_context`. This also applies to a memory saved before
the floor was raised. The lessons block says how many lessons were held back.
`rhebok memory list` still shows them. The agent sets its own confidence, and it defaults to 80, which is also the
default floor. So the floor stops only lessons that the agent itself marked as
uncertain. Read your lessons with `rhebok memory list --kind lesson` and
remove one with `rhebok memory forget <id>`.

## Limiting what reaches the prompt

- `index_transcripts = false` stops indexing agent sessions.
- `index_codex_sessions = false` stops indexing Codex sessions only.
- `[prehook] injector = "off"` stops automatic injection in Claude Code. The
  MCP tools still answer when the agent calls them.
- `[memory] enabled = false` turns memory off, including the lessons block.
- A project in `config.toml` limits prompts in its directories to that
  project's scopes and mounts.

## The dashboard

`rhebok dashboard` listens on 127.0.0.1 only. It serves a request only when
its `Host` header names `127.0.0.1`, `localhost` or `[::1]` with the
dashboard's own port; anything else, or no `Host` at all, gets
`421 Misdirected Request`. That check runs on every request, before any
other work, and it blocks DNS rebinding, where a web page makes its own name
point at your machine to read the dashboard. The dashboard also checks the
`Origin` header on every POST, so that a web page open in your browser cannot
change your settings or memories through it.

The dashboard's port is chosen by the operating system unless you pass
`--port`, so give it a fixed port before you put anything in front of it.

- **An SSH tunnel** forwards bytes and cannot change any header, so the
  browser's `Host` names the tunnel's local end. Use the dashboard's own port
  on both ends: run `rhebok dashboard --port 7331 --no-open` on the remote
  machine, then `ssh -L 7331:127.0.0.1:7331 <host>` and open
  `http://localhost:7331`. A different local port, such as
  `ssh -L 8080:127.0.0.1:7331`, gets `421` on every request.
- **A reverse proxy** must rewrite `Host` to `127.0.0.1:<dashboard port>`. It
  must also rewrite `Origin` to `http://127.0.0.1:<dashboard port>`, or drop
  it, because the browser sends the proxy's own origin, and every POST with
  that origin is refused with `409`.

## Backups and updates

- With a backup key, a restore checks each backup manifest's authentication
  tag and refuses one that was changed. Your storage provider can still read
  file names, sizes and hashes. See [Backups](backups.md#encryption).
- `rhebok update` and the installer check each download against the
  `.sha256` file published in the same GitHub release. There is no
  signature beyond that checksum.

## Reporting a problem

Report a vulnerability privately, as [SECURITY.md](https://github.com/Stellenbosch-Technologies/rhebok/blob/main/SECURITY.md) describes.
