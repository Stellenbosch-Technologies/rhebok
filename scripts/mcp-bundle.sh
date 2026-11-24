#!/bin/sh
set -eu

usage="usage: scripts/mcp-bundle.sh <version> <dir holding the release archives> <output dir>"
version="${1:?$usage}"
dist="${2:?$usage}"
out="${3:?$usage}"
root="$(cd "$(dirname "$0")/.." && pwd)"
repo="Stellenbosch-Technologies/rhebok"
targets="aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu"

fail() {
    echo "mcp-bundle: $*" >&2
    exit 1
}

for t in $targets; do
    [ -f "$dist/rhebok-$t.tar.gz" ] || fail "missing $dist/rhebok-$t.tar.gz"
done
[ -d "$out" ] || fail "$out is not a directory"
out="$(cd "$out" && pwd)"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/bundle/server"

for t in $targets; do
    mkdir -p "$stage/bundle/server/$t"
    tar -xzf "$dist/rhebok-$t.tar.gz" -C "$stage/bundle/server/$t" rhebok
    chmod 755 "$stage/bundle/server/$t/rhebok"
done
first="${targets%% *}"
tar -xzf "$dist/rhebok-$first.tar.gz" -C "$stage/bundle" LICENSE THIRD-PARTY-NOTICES.md
install -m 644 "$root/plugin/icon.png" "$stage/bundle/icon.png"

cat > "$stage/bundle/server/rhebok-mcp" <<'LAUNCHER'
#!/bin/sh
here="$(cd "$(dirname "$0")" && pwd)"
for candidate in \
    "$(command -v rhebok)" \
    "${RHEBOK_DB:+${RHEBOK_DB%/*}/bin/rhebok}" \
    "${XDG_DATA_HOME:-$HOME/.local/share}/rhebok/bin/rhebok" \
    "$HOME/Library/Application Support/rhebok/bin/rhebok"
do
    if [ -n "$candidate" ] && [ -f "$candidate" ] && [ -x "$candidate" ]; then
        exec "$candidate" "$@"
    fi
done
case "$(uname -s) $(uname -m)" in
    "Darwin arm64") target=aarch64-apple-darwin ;;
    "Linux x86_64") target=x86_64-unknown-linux-gnu ;;
    "Linux aarch64" | "Linux arm64") target=aarch64-unknown-linux-gnu ;;
    *)
        echo "rhebok has no build for $(uname -s) $(uname -m); see https://github.com/Stellenbosch-Technologies/rhebok#install" >&2
        exit 1
        ;;
esac
exec "$here/$target/rhebok" "$@"
LAUNCHER
chmod 755 "$stage/bundle/server/rhebok-mcp"

cat > "$stage/bundle/manifest.json" <<MANIFEST
{
  "manifest_version": "0.3",
  "name": "rhebok",
  "display_name": "rhebok",
  "version": "$version",
  "description": "Local search over your notes, PDFs and past agent sessions.",
  "long_description": "rhebok indexes the folders you choose and your past Claude Code and Codex sessions, and lets the agent search them with vector, keyword and link-graph search. Embeddings run on your machine through Ollama, or through an OpenAI-compatible endpoint you choose. Install rhebok and run rhebok install first, so that an index and an embedding model exist: this bundle runs the rhebok you installed, and its own copy only when it finds none.",
  "author": {
    "name": "Stellenbosch Technologies",
    "url": "https://github.com/Stellenbosch-Technologies"
  },
  "repository": {
    "type": "git",
    "url": "https://github.com/$repo"
  },
  "homepage": "https://stelltec.com/rhebok/",
  "documentation": "https://github.com/$repo/tree/main/docs",
  "support": "https://github.com/$repo/issues",
  "icon": "icon.png",
  "server": {
    "type": "binary",
    "entry_point": "server/rhebok-mcp",
    "mcp_config": {
      "command": "\${__dirname}/server/rhebok-mcp",
      "args": ["mcp"]
    }
  },
  "keywords": ["rag", "search", "memory", "notes", "local-first", "ollama"],
  "license": "AGPL-3.0-only",
  "compatibility": {
    "platforms": ["darwin", "linux"]
  }
}
MANIFEST
python3 -m json.tool "$stage/bundle/manifest.json" >/dev/null || fail "manifest.json is not valid JSON"

(cd "$stage/bundle" && zip -q -X -r "$stage/rhebok-mcp.mcpb" .)

if command -v shasum >/dev/null 2>&1; then
    digest="$(shasum -a 256 "$stage/rhebok-mcp.mcpb" | awk '{print $1}')"
else
    digest="$(sha256sum "$stage/rhebok-mcp.mcpb" | awk '{print $1}')"
fi

cat > "$stage/server.json" <<SERVER
{
  "\$schema": "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
  "name": "io.github.$repo",
  "title": "rhebok",
  "description": "Local search over your notes, PDFs and past agent sessions, with embeddings on your machine.",
  "version": "$version",
  "websiteUrl": "https://stelltec.com/rhebok/",
  "repository": {
    "url": "https://github.com/$repo",
    "source": "github"
  },
  "icons": [
    {
      "src": "https://raw.githubusercontent.com/$repo/v$version/plugin/icon.png",
      "mimeType": "image/png",
      "sizes": ["512x512"]
    }
  ],
  "packages": [
    {
      "registryType": "mcpb",
      "identifier": "https://github.com/$repo/releases/download/v$version/rhebok-mcp.mcpb",
      "version": "$version",
      "fileSha256": "$digest",
      "transport": {
        "type": "stdio"
      }
    }
  ]
}
SERVER
python3 -m json.tool "$stage/server.json" >/dev/null || fail "server.json is not valid JSON"

mv "$stage/rhebok-mcp.mcpb" "$out/rhebok-mcp.mcpb"
mv "$stage/server.json" "$out/server.json"
