# rhebok for Claude Code

rhebok gives Claude Code your own knowledge base: your notes, papers, saved web
pages and past Claude Code and Codex sessions, indexed and searched on your
own machine. Before each prompt it adds the most relevant passages, but only
when they clear a confidence gate, and Claude can search the knowledge base
itself through rhebok's MCP tools.

## You also need the rhebok program

This plugin connects rhebok to Claude Code. The indexing and searching are done
by the `rhebok` program, which you install separately:

```bash
curl -fsSL https://github.com/Stellenbosch-Technologies/rhebok/releases/latest/download/install.sh | sh
```

The [install guide](https://github.com/Stellenbosch-Technologies/rhebok/blob/main/docs/install.md)
lists what you need first (a Mac with Apple silicon or Linux on x86_64 or
arm64, and [Ollama](https://ollama.com)) and other ways to install. Until the program is
installed, the plugin says so at the start of each session and otherwise does
nothing.

## What the plugin adds

- The `rhebok-prehook` mod, which adds relevant passages from your knowledge
  base to each prompt. It asks rhebok's MCP server through the `rhebok_get_context`
  tool, falls back to running `rhebok` itself when the server does not answer,
  and shows what it found on the status line. It also runs `rhebok` once when a
  session starts, so the prompt hook knows the mod is injecting.
- A prompt hook that injects instead when the mod is not running, or when you
  choose it. `/rhebok-prehook mod`, `/rhebok-prehook command` or
  `/rhebok-prehook off` picks which one injects, in every session.
- A session-start hook that shows your rhebok memories and refreshes the index
  in the background.
- MCP tools to get the context for a message, search the knowledge base,
  follow its links, re-index it, and save and forget memories.
- Slash commands, among them `/rhebok-search`, `/rhebok-status` and `/rhebok-index`.

## Where your data goes

By default rhebok embeds your files with a local model through Ollama and
keeps its index on your disk. It has no telemetry. It goes online only to
check for a newer release, to download its vector-search extension the first
time it builds an index, to fetch the OCR model the first time it reads a
scanned PDF, and to fetch pages you add with `rhebok add <url>`. If you turn
them on, it also sends text to an embedding endpoint you choose (every note it
indexes, and the text of each prompt it searches for) and backs up to your own
S3 bucket or Google Drive.

Retrieved text is added to your prompts as reference data, marked so that it
cannot pass as your own words. The
[security model](https://github.com/Stellenbosch-Technologies/rhebok/blob/main/docs/security-model.md)
says what can reach your prompts and how to limit it.

rhebok is free software under the
[GNU AGPL v3.0](https://github.com/Stellenbosch-Technologies/rhebok/blob/main/LICENSE).
