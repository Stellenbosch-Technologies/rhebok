#!/usr/bin/env bash
set -euo pipefail
unset RHEBOK_EMBED_URL RHEBOK_EMBED_MODEL RHEBOK_EMBED_TOKEN

TAB="${1:-}"
STATE="${2:-}"
WIDE="${RHEBOK_PARITY_WIDTH:-}"
case "$TAB" in
  graph|search|memories|integrations|health|settings|onboarding) ;;
  *) echo "usage: scripts/parity.sh <graph|search|memories|integrations|health|settings|onboarding> [state]" >&2; exit 2 ;;
esac
QUERY="how often do I feed the sourdough starter"
case "$TAB:$STATE" in
  *:) ;;
  search:empty) ;;
  graph:linked|graph:sessions|graph:document|graph:project|graph:memory|graph:zoom|graph:ring|graph:error|graph:hover|graph:panned) export RHEBOK_PARITY_GRAPH="$STATE" ;;
  search:results) export RHEBOK_PARITY_QUERY="$QUERY" ;;
  search:expanded) export RHEBOK_PARITY_QUERY="$QUERY" RHEBOK_PARITY_EXPAND=1 ;;
  search:outage) export RHEBOK_PARITY_QUERY="$QUERY" ;;
  memories:list|memories:open|memories:confirm|memories:new|memories:disabled|memories:long) export RHEBOK_PARITY_MEMORIES="$STATE" ;;
  integrations:open|integrations:codex|integrations:connected|integrations:confirm|integrations:change|integrations:disconnect|integrations:refusal|integrations:snippets) export RHEBOK_PARITY_INTEGRATIONS="$STATE" ;;
  settings:edit|settings:blocking|settings:conflict|settings:ring|settings:reindex) export RHEBOK_PARITY_SETTINGS="$STATE" ;;
  settings:retrieval|settings:threshold|settings:slider-key|settings:retrieval-end|settings:embedding|settings:embedding-end|settings:remote|settings:token|settings:saved|settings:test-ok|settings:test-fail|settings:model|settings:memory|settings:documents|settings:updates|settings:updates-end|settings:advanced|settings:weights|settings:raw|settings:raw-edit|settings:raw-error|settings:raw-copied) export RHEBOK_PARITY_SETTINGS="$STATE" ;;
  settings:narrow|settings:notice) export RHEBOK_PARITY_SETTINGS="$STATE" RHEBOK_PARITY_WIDTH=1024 ;;
  onboarding:folders|onboarding:added|onboarding:connect|onboarding:index|onboarding:search) export RHEBOK_PARITY_ONBOARDING="$STATE" ;;
  *) echo "parity: $TAB has no state $STATE (graph: linked, sessions, document, project, memory, zoom, ring, error, hover, panned; search: empty, results, expanded, outage; memories: list, open, confirm, new, disabled, long; integrations: open, codex, connected, confirm, change, disconnect, refusal, snippets; settings: edit, blocking, conflict, ring, reindex, narrow, notice, retrieval, threshold, slider-key, retrieval-end, embedding, embedding-end, remote, token, saved, test-ok, test-fail, model, memory, documents, updates, updates-end, advanced, weights, raw, raw-edit, raw-error, raw-copied; onboarding: folders, added, connect, index, search)" >&2; exit 2 ;;
esac
if [ "$TAB" = onboarding ]; then
  export RHEBOK_SANDBOX_FRESH=1
fi
SHOT="$TAB${STATE:+-$STATE}${WIDE:+-w$WIDE}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if [ -d "$HOME/.local/node/bin" ]; then
  export PATH="$HOME/.local/node/bin:$PATH"
fi

OUT="$ROOT/target/parity"
mkdir -p "$OUT"
LOG="$OUT/sandbox.log"
: > "$LOG"

cargo build
cargo build --manifest-path app/Cargo.toml --features shoot
if [ ! -d dashboard/e2e/node_modules ]; then
  (cd dashboard/e2e && npm ci --no-audit --no-fund)
fi

CLI="$ROOT/target/debug/rhebok"
APP="$ROOT/app/target/debug/rhebok-app"
if [ "$("$CLI" --version)" != "rhebok $(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)" ]; then
  echo "parity: $CLI does not match the rhebok version the app is built against" >&2
  exit 1
fi

RHEBOK_BIN="$CLI" node dashboard/e2e/sandbox.mjs > "$LOG" 2>&1 &
SANDBOX=$!
stop_sandbox() {
  kill -TERM "$SANDBOX" 2>/dev/null || return 0
  for _ in $(seq 1 100); do
    kill -0 "$SANDBOX" 2>/dev/null || break
    sleep 0.1
  done
  if kill -0 "$SANDBOX" 2>/dev/null; then kill -KILL "$SANDBOX" 2>/dev/null || true; fi
  wait "$SANDBOX" 2>/dev/null || true
}
trap stop_sandbox EXIT

for _ in $(seq 1 600); do
  grep -q '^READY$' "$LOG" && break
  kill -0 "$SANDBOX" 2>/dev/null || { cat "$LOG" >&2; echo "parity: the sandbox died" >&2; exit 1; }
  sleep 0.5
done
grep -q '^READY$' "$LOG" || { cat "$LOG" >&2; echo "parity: the sandbox never became ready" >&2; exit 1; }

REAL_HOME="$HOME"
value() { grep "^$1=" "$LOG" | head -1 | cut -d= -f2-; }
export RHEBOK_DB="$(value RHEBOK_DB)"
export RHEBOK_CONFIG="$(value RHEBOK_CONFIG)"
export HOME="$(value HOME)"
export CODEX_HOME="$(value CODEX_HOME)"
export RHEBOK_URL="$(value RHEBOK_URL)"
export RHEBOK_PARITY_FOLDER="$(value CORPUS)"

guard_sandbox() {
  local action="$1"
  shift
  case "$HOME" in
    "$REAL_HOME"|"") echo "parity: refusing to $action outside the sandbox" >&2; exit 1 ;;
    */rhebok-e2e-*/home) ;;
    *) echo "parity: $HOME is not a sandbox home" >&2; exit 1 ;;
  esac
  local root
  root="$(dirname "$HOME")"
  for written in "$@"; do
    case "$written" in
      "$root"/*) ;;
      *) echo "parity: $written is outside the sandbox $root" >&2; exit 1 ;;
    esac
  done
}

if [ "$STATE" = outage ]; then
  guard_sandbox "change the Ollama URL" "$RHEBOK_CONFIG"
  sed -i.bak 's|^ollama_url = .*|ollama_url = "http://127.0.0.1:9"|' "$RHEBOK_CONFIG"
  rm -f "$RHEBOK_CONFIG.bak"
fi

if [ "$TAB" = memories ] && [ -n "$STATE" ]; then
  guard_sandbox "write memories" "$RHEBOK_CONFIG" "$RHEBOK_DB"
fi
if [ "$TAB" = memories ] && [ "$STATE" = disabled ]; then
  printf '\n[memory]\nenabled = false\n' >> "$RHEBOK_CONFIG"
elif [ "$TAB" = memories ] && [ "$STATE" = long ]; then
  "$CLI" memory add --kind lesson --global "Line one of a long lesson.
Line two of a long lesson.
Line three of a long lesson.
Line four of a long lesson.
Line five of a long lesson.
Line six of a long lesson.
Line seven of a long lesson.
Line eight of a long lesson.
Line nine of a long lesson." > /dev/null
elif [ "$TAB" = memories ] && [ -n "$STATE" ]; then
  remember() { "$CLI" memory add "$@" > /dev/null; }
  remember --kind lesson --global "Feed the sourdough starter twice a day while the kitchen is warm, and once a day in winter."
  remember --kind fact --project "$(dirname "$HOME")/corpus" "The bread oven runs fifteen degrees hot, so bake at 230 when a recipe says 245."
  remember --kind episode --global "Moved the starter to the fridge before the trip and revived it in two feedings."
  remember --kind lesson --global "Always weigh the flour.
Cups vary by up to twenty percent between scoops."
fi

if [ "$TAB" = graph ] && [ -n "$STATE" ]; then
  guard_sandbox "index" "$RHEBOK_CONFIG" "$RHEBOK_DB"
  CORPUS="$(dirname "$HOME")/corpus"
  mkdir -p "$CORPUS/kitchen"
  note() { printf -- '---\ntags: [%s]\n---\n# %s\n\n%s\n' "$2" "$3" "$4" > "$CORPUS/kitchen/$1.md"; }
  note flour "baking, pantry" "Flour" "Bread flour has more protein than cake flour. Feed the [[sourdough]] starter with it and keep a log in [[bake-log]]."
  note water "baking" "Water" "Use water at room temperature for the starter. See [[flour]] and [[sourdough]]."
  note salt "baking, pantry" "Salt" "Two percent salt by flour weight. Pairs with [[flour]] and [[water]]."
  note oven "baking, equipment" "Oven" "The oven runs hot. Bake at 230 degrees. The [[bake-log]] records every loaf."
  note bake-log "baking" "Bake log" "Each loaf: [[flour]], [[water]], [[salt]], hydration and the [[oven]] temperature."
  note pantry "pantry" "Pantry" "Stock [[flour]] and [[salt]]. Rotate jars from the [[garden]] harvest."
  note compost "garden" "Compost" "Spent starter and peelings go to compost for the [[garden]]."
  note tomatoes "garden, harvest" "Tomatoes" "Stake tomatoes early. Feed them [[compost]] and water at dawn."
  note herbs "garden, harvest" "Herbs" "Basil and thyme by the kitchen door. Dry them for the [[pantry]]."
  note checklist-notes "ops" "Release checklist notes" "Run the [[deploys]] checklist and read the [[runbook]] before a release."
  note on-call "ops" "On call" "The [[runbook]] lists the pager steps. Escalate after thirty minutes."
  note retros "ops" "Retros" "Retros follow every incident from the [[on-call]] rota and the [[deploys]] log."
  sed -i.bak 's|^index_transcripts = false|index_transcripts = true|' "$RHEBOK_CONFIG"
  rm -f "$RHEBOK_CONFIG.bak"
  SESSIONS="$HOME/.claude/projects/-sandbox-kitchen"
  mkdir -p "$SESSIONS"
  printf '%s\n' \
    '{"type":"user","message":{"role":"user","content":"Why does the loaf in kitchen/oven.md come out dark? Check src/oven/bake.rs."},"cwd":"/sandbox/kitchen","sessionId":"7a1f3c2e-0000-4000-8000-000000000001"}' \
    '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"The oven runs hot, so lower the bake in src/oven/bake.rs and note it in kitchen/bake-log.md."}]},"cwd":"/sandbox/kitchen","sessionId":"7a1f3c2e-0000-4000-8000-000000000001"}' \
    > "$SESSIONS/7a1f3c2e-0000-4000-8000-000000000001.jsonl"
  touch -t 202601010000 "$SESSIONS/7a1f3c2e-0000-4000-8000-000000000001.jsonl"
  "$CLI" memory add --kind lesson --project "$CORPUS" "Feed the sourdough starter twice a day while the kitchen is warm." > /dev/null
  "$CLI" memory add --kind fact --global "The bread oven runs fifteen degrees hot." > /dev/null
  "$CLI" index > "$OUT/graph-index.log" 2>&1 || { cat "$OUT/graph-index.log" >&2; echo "parity: the sandbox index failed" >&2; exit 1; }
fi

if [ "$TAB" = integrations ] && [ -n "$STATE" ]; then
  guard_sandbox "write agent configs"
  case "$STATE" in
    codex) mkdir -p "$HOME/.codex" ;;
    connected|confirm|disconnect) mkdir -p "$HOME/.cursor"; "$CLI" connect cursor > /dev/null ;;
    change) mkdir -p "$HOME/.cursor" ;;
    refusal) mkdir -p "$HOME/.gemini"; printf '{\n  // a comment JSON does not allow\n  "theme": "dark",\n}\n' > "$HOME/.gemini/settings.json" ;;
  esac
fi

if [ "$TAB" = settings ] && [ -n "$STATE" ]; then
  guard_sandbox "change settings" "$RHEBOK_CONFIG" "$RHEBOK_DB" "$CODEX_HOME"
  ENV_FILE="$(dirname "$RHEBOK_CONFIG")/env"
  case "$STATE" in
    token|test-fail) printf 'RHEBOK_EMBED_URL=http://127.0.0.1:9/v1\nRHEBOK_EMBED_MODEL=stub-model\nRHEBOK_EMBED_TOKEN=sk-parity-secret\n' > "$ENV_FILE"; chmod 600 "$ENV_FILE" ;;
  esac
fi

if [ "$TAB" = onboarding ]; then
  DATA="$(dirname "$RHEBOK_DB")"
  guard_sandbox "run setup" "$RHEBOK_CONFIG" "$RHEBOK_DB" "$DATA"
  SANDBOX_ROOT="$(dirname "$HOME")"
  FRESH="$SANDBOX_ROOT/fresh"
  rm -rf "$FRESH"
  mkdir -p "$FRESH"
  cp -a "$DATA" "$FRESH/data"
  cp -a "$RHEBOK_CONFIG" "$FRESH/config.toml"
fi

rm -f "$OUT/$SHOT-web-nodes.json" "$OUT/$SHOT-native-nodes.json"
(cd "$ROOT/dashboard/e2e" && HOME="$REAL_HOME" RHEBOK_PARITY_NODES="$OUT/$SHOT-web-nodes.json" node shoot-web.mjs "$TAB" "$OUT/$SHOT-web.png")
if [ "$TAB" = integrations ] && [ "$STATE" = change ]; then
  "$CLI" disconnect cursor > /dev/null
elif [ "$TAB" = integrations ] && [ "$STATE" = disconnect ]; then
  "$CLI" connect cursor > /dev/null
elif [ "$TAB" = settings ] && [ "$STATE" = saved ]; then
  rm -f "$(dirname "$RHEBOK_CONFIG")/env"
elif [ "$TAB" = onboarding ]; then
  web_index() { pgrep -f "$DATA/bin/rhebok index" || true; }
  for _ in $(seq 1 600); do
    [ -z "$(web_index)" ] && break
    sleep 0.1
  done
  if [ -n "$(web_index)" ]; then
    kill -TERM $(web_index) 2>/dev/null || true
    for _ in $(seq 1 100); do
      [ -z "$(web_index)" ] && break
      sleep 0.1
    done
  fi
  if [ -n "$(web_index)" ]; then
    echo "parity: the web's index in $DATA is still running, so it is not removed" >&2
    exit 1
  fi
  rm -rf "$DATA"
  cp -a "$FRESH/data" "$DATA"
  cp -a "$FRESH/config.toml" "$RHEBOK_CONFIG"
fi
if [ "$TAB:$STATE" = graph:panned ]; then
  export RHEBOK_PARITY_PAN="$(node -e 'const p = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).pan; console.log(`${p.dx},${p.dy}`)' "$OUT/$SHOT-web-nodes.json")"
fi
RHEBOK_PARITY_NODES="$OUT/$SHOT-native-nodes.json" "$APP" --screenshot "$TAB" "$OUT/$SHOT-native.png"
if [ "$TAB" = graph ] && [ -f "$OUT/$SHOT-web-nodes.json" ] && [ -f "$OUT/$SHOT-native-nodes.json" ]; then
  node -e '
    const [web, native] = process.argv.slice(1).map((f) => JSON.parse(require("fs").readFileSync(f, "utf8")));
    const at = new Map(native.nodes.map((n) => [n.id, n]));
    let worst = 0, missing = 0;
    for (const n of web.nodes) {
      const m = at.get(n.id);
      if (!m) { missing++; continue; }
      worst = Math.max(worst, Math.hypot(n.sx - m.sx, n.sy - m.sy));
    }
    if (web.tooltip) console.log(`web tooltip: ${JSON.stringify(web.tooltip)}`);
    if (native.tooltip) console.log(`native tooltip: ${JSON.stringify(native.tooltip)}`);
    console.log(`graph nodes: web ${web.nodes.length}, native ${native.nodes.length}, missing ${missing}; zoom web ${web.k.toFixed(6)} native ${native.k.toFixed(6)}; max centre difference ${(worst * 2).toFixed(3)} device px`);
  ' "$OUT/$SHOT-web-nodes.json" "$OUT/$SHOT-native-nodes.json"
fi
"$APP" --side-by-side "$OUT/$SHOT-web.png" "$OUT/$SHOT-native.png" "$OUT/$SHOT-side.png"

echo "$OUT/$SHOT-web.png"
echo "$OUT/$SHOT-native.png"
echo "$OUT/$SHOT-side.png"
