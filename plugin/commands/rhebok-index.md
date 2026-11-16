---
description: Index or re-index your knowledge base
allowed-tools: Bash(rhebok:*)
---

Run `rhebok index`. Report how many documents were added, updated, skipped, and
pruned, and how many chunks and graph edges resulted.

If the output mentions an embedding-model mismatch, explain that the index was
built with a different model and that `rhebok index --reindex` rebuilds it.
