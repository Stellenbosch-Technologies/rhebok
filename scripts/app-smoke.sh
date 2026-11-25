#!/bin/sh

set -u

BIN_IN="${1:?usage: app-smoke.sh <rhebok-binary> <rhebok-app archive> <version>}"
ARCHIVE="${2:?usage: app-smoke.sh <rhebok-binary> <rhebok-app archive> <version>}"
VERSION="${3:?usage: app-smoke.sh <rhebok-binary> <rhebok-app archive> <version>}"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

FAILS=0
ok() {
    if [ "$2" = "$3" ]; then
        printf 'ok    %s: %s\n' "$1" "$2"
    else
        printf 'FAIL  %s: got [%s] want [%s]\n' "$1" "$2" "$3"
        [ -n "${4:-}" ] && printf '%s\n' "$4" | sed 's/^/      | /'
        FAILS=$((FAILS + 1))
    fi
}

S="$(mktemp -d)" || exit 1
APP_PID=""
INSTALLED=""
cleanup() {
    [ -n "$APP_PID" ] && kill "$APP_PID" 2>/dev/null
    [ -n "$INSTALLED" ] && [ -e "$INSTALLED" ] && "$LSREGISTER" -u "$INSTALLED" >/dev/null 2>&1
    rm -rf "$S"
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM HUP

BIN_IN="$(cd "$(dirname "$BIN_IN")" && pwd)/$(basename "$BIN_IN")"
ARCHIVE="$(cd "$(dirname "$ARCHIVE")" && pwd)/$(basename "$ARCHIVE")"
NAME="$(basename "$ARCHIVE")"

mkdir -p "$S/unpacked"
tar -xzf "$ARCHIVE" -C "$S/unpacked" || { echo "FAIL  cannot untar $ARCHIVE"; exit 1; }
APP="$S/unpacked/Rhebok.app"
EXE="$APP/Contents/MacOS/rhebok-app"

outside="$(tar -tzf "$ARCHIVE" | grep -v -e '^Rhebok\.app$' -e '^Rhebok\.app/' || true)"
ok "1 every entry is under Rhebok.app/" "${outside:-none}" "none"

OUT="$(codesign --verify --strict --deep "$APP" 2>&1)"
ok "2 codesign verifies" "$?" "0" "$OUT"

LIBS="$(otool -L "$EXE" 2>&1)"
ok "3a otool -L reads the executable" "$?" "0" "$LIBS"
foreign="$(printf '%s\n' "$LIBS" | awk 'NR > 1 { print $1 }' | grep -E '^/(opt/homebrew|usr/local|opt/local)/' || true)"
ok "3b otool -L has no package-manager paths" "${foreign:-none}" "none"

ok "4 --version" "$("$EXE" --version 2>&1)" "rhebok-app $VERSION"

PLIST="$APP/Contents/Info.plist"
ok "5a CFBundleShortVersionString" "$(plutil -extract CFBundleShortVersionString raw "$PLIST" 2>&1)" "$VERSION"
ok "5b CFBundleIdentifier" "$(plutil -extract CFBundleIdentifier raw "$PLIST" 2>&1)" "com.stelltec.rhebok"

HOME_DIR="$S/home with space"
mkdir -p "$S/dl" "$S/stubs" "$S/tmp" "$HOME_DIR"
cp "$BIN_IN" "$S/dl/rhebok"
cp "$ARCHIVE" "$S/dl/$NAME"
SHA_NAME="${NAME%.tar.gz}.sha256"
if [ -f "${ARCHIVE%.tar.gz}.sha256" ]; then
    cp "${ARCHIVE%.tar.gz}.sha256" "$S/dl/$SHA_NAME"
else
    printf '%s  %s\n' "$(shasum -a 256 "$ARCHIVE" | awk '{print $1}')" "$NAME" > "$S/dl/$SHA_NAME"
fi
printf '#!/bin/sh\nexit 0\n' > "$S/stubs/crontab"
chmod 755 "$S/stubs/crontab"
printf '[embed]\nollama_url = "http://127.0.0.1:9"\n' > "$S/config.toml"

run() {
    env -i HOME="$HOME_DIR" PATH="$S/stubs:/usr/bin:/bin:/usr/sbin:/sbin" \
        RHEBOK_LINK_DIR="$HOME_DIR/bin" RHEBOK_CONFIG="$S/config.toml" TMPDIR="$S/tmp" "$@"
}

INSTALLED="$HOME_DIR/Applications/Rhebok.app"
OUT="$(run "$S/dl/rhebok" install --yes --quiet 2>&1)"
ok "6a install exits 0" "$?" "0" "$OUT"
ok "6b install placed the bundle" "$([ -d "$INSTALLED" ] && echo present || echo absent)" "present" "$OUT"

ok "7 rhebok app --version" "$(run "$HOME_DIR/bin/rhebok" app --version 2>&1)" "rhebok-app $VERSION"

PREF="$HOME/Library/Preferences/com.stelltec.rhebok.plist"
[ -e "$PREF" ] && pref_before=present || pref_before=absent
(exec env -i HOME="$HOME_DIR" PATH="$S/stubs:/usr/bin:/bin:/usr/sbin:/sbin" RHEBOK_LINK_DIR="$HOME_DIR/bin" RHEBOK_CONFIG="$S/config.toml" TMPDIR="$S/tmp" "$HOME_DIR/bin/rhebok" app >"$S/app.log" 2>&1) &
APP_PID=$!
sleep 5
kill -0 "$APP_PID" 2>/dev/null && alive=alive || alive=dead
ok "8a rhebok app stays alive" "$alive" "alive" "$(cat "$S/app.log")"
if [ "$(launchctl managername 2>/dev/null)" = Aqua ]; then
    windows="$(osascript -l JavaScript -e "
ObjC.import('CoreGraphics');
var list = ObjC.deepUnwrap(ObjC.castRefToObject(\$.CGWindowListCopyWindowInfo(\$.kCGWindowListOptionAll, \$.kCGNullWindowID)));
list.filter(function (w) { return w.kCGWindowOwnerName === 'Rhebok' && w.kCGWindowOwnerPID === $APP_PID; }).length;
" 2>&1)"
    case "$windows" in
        ''|*[!0-9]*|0) win=missing ;;
        *) win=found ;;
    esac
    ok "8b a window owned by Rhebok exists" "$win" "found" "windows: $windows"
else
    echo "skip  8b no window server: alive check only"
fi
kill "$APP_PID" 2>/dev/null
waited=0
while kill -0 "$APP_PID" 2>/dev/null && [ "$waited" -lt 10 ]; do
    sleep 0.5
    waited=$((waited + 1))
done
kill -0 "$APP_PID" 2>/dev/null && kill -9 "$APP_PID" 2>/dev/null
wait "$APP_PID" 2>/dev/null
APP_PID=""
if [ "$pref_before" = absent ]; then
    defaults delete com.stelltec.rhebok >/dev/null 2>&1
    sleep 1
    ok "8c the app left no preferences in the real home" "$([ -e "$PREF" ] && echo present || echo absent)" "absent"
else
    echo "skip  8c $PREF existed before the run; left alone"
fi

"$LSREGISTER" -u "$INSTALLED" >/dev/null 2>&1
OUT="$(run "$HOME_DIR/bin/rhebok" uninstall --yes 2>&1)"
ok "9a uninstall exits 0" "$?" "0" "$OUT"
ok "9b uninstall removed the bundle" "$([ -e "$INSTALLED" ] && echo present || echo absent)" "absent" "$OUT"

if [ "$FAILS" -eq 0 ]; then
    echo "app-smoke: 9 assertions, all passed"
    exit 0
fi
echo "app-smoke: $FAILS assertion(s) FAILED"
exit 1
