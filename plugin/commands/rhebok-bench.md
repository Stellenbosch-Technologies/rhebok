---
description: Measure retrieval recall and latency at each quality tier
allowed-tools: Bash(rhebok:*)
---

Run `rhebok bench`. Present the per-tier recall@5 and p50/p95 latency as a table,
then recommend a tier for the prompt hook and one for the MCP tool based on the
measurements. Explain the tradeoff in one sentence.
