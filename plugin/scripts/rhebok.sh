#!/bin/sh
for candidate in \
    "$(command -v rhebok)" \
    "${RHEBOK_DB:+${RHEBOK_DB%/*}/bin/rhebok}" \
    "${XDG_DATA_HOME:-$HOME/.local/share}/rhebok/bin/rhebok" \
    "$HOME/Library/Application Support/rhebok/bin/rhebok" \
    "${HOMEBREW_PREFIX:+$HOMEBREW_PREFIX/opt/rhebok/bin/rhebok}" \
    "/opt/homebrew/opt/rhebok/bin/rhebok" \
    "/usr/local/opt/rhebok/bin/rhebok" \
    "/home/linuxbrew/.linuxbrew/opt/rhebok/bin/rhebok"
do
    if [ -f "$candidate" ] && [ -x "$candidate" ]; then
        exec "$candidate" "$@"
    fi
done

missing="rhebok is not installed, so the rhebok plugin cannot search your notes. To install it, follow the steps at https://github.com/Stellenbosch-Technologies/rhebok#install, then restart Claude Code."
case "$1 $2" in
"hook session-start")
    printf '{"systemMessage": "%s", "hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": "%s"}}\n' "$missing" "$missing"
    ;;
"hook "*)
    ;;
*)
    echo "$missing" >&2
    exit 1
    ;;
esac
