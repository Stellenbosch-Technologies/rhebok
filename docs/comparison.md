# How rhebok compares

Checked on 2026-11-20 against each project's own README, docs and source on
its `main` branch. Every fact has a source below. Projects change quickly, so
if a row is out of date, please open an issue or a pull request.

Four tools are compared:
- **claude-mem** records what the agent does, compresses it with an LLM, and
  feeds it back into later sessions.
- **mem0** is a memory layer for agents: a library, a self-hosted server and a
  cloud platform. Its OpenMemory MCP server was retired in July 2026 [8], so this
  page compares its Claude Code plugin.
- **basic-memory** is a Markdown knowledge base that the AI reads and writes
  through MCP.
- **mcp-memory-service** is a self-hosted memory backend with a REST API, MCP
  and a knowledge graph.

|  | rhebok | claude-mem | mem0 (Claude Code plugin) | basic-memory | mcp-memory-service |
|---|---|---|---|---|---|
| Searches on every prompt | Yes. It adds only passages that clear a relevance gate, so most prompts get none | At session start. Per-prompt injection exists but is off by default [2][4] | On the first prompt of a session only [7] | At session start and before compaction [13] | At session start, and mid-conversation when the prompt matches trigger patterns [17][18] |
| Searches the notes you already have | Yes: Markdown, PDF, web pages, any folder you name | No. `/learn-codebase` can load a repository [1] | No [7] | Yes: Markdown folders, Obsidian-compatible [11] | Documents you ingest (PDF, text, Markdown) [15] |
| Searches past agent sessions | Yes: Claude Code and Codex transcripts, as they are | Its own compressed observations of sessions [1] | Memories extracted from sessions [7] | Imports Claude.ai and ChatGPT exports [11] | Harvests learnings from transcripts [15] |
| Embeddings | Local, through Ollama, or any OpenAI-compatible server you choose | Local all-MiniLM-L6-v2 [3] | Mem0's cloud. The library defaults to OpenAI and supports Ollama [6][7][9] | Local bge-small-en-v1.5 by default [12] | Local all-MiniLM-L6-v2 [15][16] |
| Needs a cloud account or API key | No | No. Its observer LLM uses your Claude plan or a key you supply [1][2] | Yes, a Mem0 Platform key [7] | No. Cloud is optional and paid [11] | No. Cloudflare sync is optional [15] |
| Runs a background process | No. Ollama must be running | Yes, a local worker and a Chroma subprocess [1][3] | A background flush worker [7] | No, a stdio MCP server [11] | An HTTP server on 127.0.0.1 for the hooks [18] |
| Search | Vector, BM25 and link graph, fused | Vector and full-text keyword [1] | Semantic, BM25 and entity matching [6] | Vector and full-text, wikilink graph [11] | Vector and BM25, knowledge graph [15] |
| Retrieval quality numbers | `rhebok bench` measures recall@5 per tier on a golden set you write for your own notes | None published [1] | Answer accuracy on public benchmarks, on the managed platform [6] | Benchmark harness, no figures published [14] | Recall on the LongMemEval dataset [16] |
| Agents | Claude Code, Codex, Gemini CLI, Cursor, Claude Desktop | Claude Code, Codex, Cursor, Gemini, Windsurf and more [1][5] | Claude Code, Codex, Cursor, OpenCode and more [10] | Claude Desktop, Claude Code, Codex, Cursor, VS Code, ChatGPT [11] | Claude Desktop, Claude Code, Codex, Cursor, Gemini CLI and more [15] |
| Licence | AGPL-3.0 | Apache-2.0 | Apache-2.0 | AGPL-3.0 | Apache-2.0 |

## What the others do that rhebok does not

- claude-mem records the agent's tool activity and summarises it with an LLM,
  so it remembers what happened without anyone writing a note.
- mem0 offers a hosted platform, so memory can follow a user across machines
  and apps.
- basic-memory lets the AI write and edit your Markdown notes, not only read
  them.
- mcp-memory-service has a REST API and optional sync through Cloudflare, so
  several machines can share one store.

## Reading the quality numbers

The numbers above measure different things and cannot be compared:
- mem0's are answer accuracy, judged by an LLM, on its managed platform.
- mcp-memory-service's are retrieval recall on a public conversation dataset.
- rhebok's are retrieval recall on your own notes, against questions you
  write yourself. They tell you how well rhebok finds your material, not how
  it ranks against another tool.

## Sources

- [1] https://github.com/thedotmack/claude-mem
- [2] https://github.com/thedotmack/claude-mem/blob/main/src/shared/SettingsDefaultsManager.ts
- [3] https://github.com/thedotmack/claude-mem/blob/main/src/services/sync/ChromaMcpManager.ts
- [4] https://github.com/thedotmack/claude-mem/blob/main/src/cli/handlers/session-init.ts
- [5] https://github.com/thedotmack/claude-mem/tree/main/src/cli/adapters
- [6] https://github.com/mem0ai/mem0
- [7] https://github.com/mem0ai/mem0/blob/main/docs/integrations/claude-code.mdx
- [8] https://github.com/mem0ai/mem0/pull/6530 (OpenMemory retired)
- [9] https://github.com/mem0ai/mem0/blob/main/docs/components/embedders/models/ollama.mdx
- [10] https://github.com/mem0ai/mem0/tree/main
- [11] https://github.com/basicmachines-co/basic-memory
- [12] https://github.com/basicmachines-co/basic-memory/blob/main/src/basic_memory/config_models.py
- [13] https://github.com/basicmachines-co/basic-memory/blob/main/plugins/claude-code/hooks/hooks.json
- [14] https://github.com/basicmachines-co/basic-memory/blob/main/benchmarks/README.md
- [15] https://github.com/doobidoo/mcp-memory-service
- [16] https://github.com/doobidoo/mcp-memory-service/blob/main/docs/BENCHMARKS.md
- [17] https://github.com/doobidoo/mcp-memory-service/blob/main/claude-hooks/README-NATURAL-TRIGGERS.md
- [18] https://github.com/doobidoo/mcp-memory-service/blob/main/claude-hooks/README.md
