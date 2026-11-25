#!/bin/sh
set -eu

usage="usage: scripts/package-app.sh <rhebok-app> <version> <notices.md|--dev> <archive.tar.gz>"
binary="${1:?$usage}"
version="${2:?$usage}"
notices="${3:?$usage}"
archive="${4:?$usage}"
root="$(cd "$(dirname "$0")/.." && pwd)"
rm -f "$archive"

fail() {
    echo "package-app: $*" >&2
    exit 1
}

[ -x "$binary" ] || fail "$binary is not an executable"
[ -s "$root/LICENSE" ] || fail "$root/LICENSE is missing"
[ -s "$root/app/assets/bundle-icon.png" ] || fail "$root/app/assets/bundle-icon.png is missing"
if [ "$notices" != --dev ]; then
    [ -s "$notices" ] || fail "$notices is missing or empty"
    [ "$(sed -n 1p "$notices")" = "# Third-party notices" ] ||
        fail "$notices is not a notices file written by scripts/third-party-notices.sh"
fi

minos="$(otool -l "$binary" | awk '/LC_BUILD_VERSION/ {f=1} f && $1 == "minos" {print $2; exit}')"
[ -n "$minos" ] || fail "$binary has no LC_BUILD_VERSION minos"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/stage"
app="$stage/Rhebok.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m 755 "$binary" "$app/Contents/MacOS/rhebok-app"
install -m 644 "$root/LICENSE" "$app/Contents/Resources/LICENSE"
if [ "$notices" != --dev ]; then
    install -m 644 "$notices" "$app/Contents/Resources/THIRD-PARTY-NOTICES.md"
fi

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Rhebok</string>
    <key>CFBundleDisplayName</key>
    <string>Rhebok</string>
    <key>CFBundleIdentifier</key>
    <string>com.stelltec.rhebok</string>
    <key>CFBundleExecutable</key>
    <string>rhebok-app</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
    <key>CFBundleIconFile</key>
    <string>Rhebok</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSMinimumSystemVersion</key>
    <string>$minos</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST
chmod 644 "$app/Contents/Info.plist"

iconset="$work/Rhebok.iconset"
mkdir -p "$iconset"
icon="$root/app/assets/bundle-icon.png"
for size in 16 32 64 128 256 512 1024; do
    sips -z "$size" "$size" "$icon" --out "$work/icon_$size.png" >/dev/null || fail "sips could not scale the icon to $size"
done
cp "$work/icon_16.png" "$iconset/icon_16x16.png"
cp "$work/icon_32.png" "$iconset/icon_16x16@2x.png"
cp "$work/icon_32.png" "$iconset/icon_32x32.png"
cp "$work/icon_64.png" "$iconset/icon_32x32@2x.png"
cp "$work/icon_128.png" "$iconset/icon_128x128.png"
cp "$work/icon_256.png" "$iconset/icon_128x128@2x.png"
cp "$work/icon_256.png" "$iconset/icon_256x256.png"
cp "$work/icon_512.png" "$iconset/icon_256x256@2x.png"
cp "$work/icon_512.png" "$iconset/icon_512x512.png"
cp "$work/icon_1024.png" "$iconset/icon_512x512@2x.png"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Rhebok.icns" || fail "iconutil could not build Rhebok.icns"
chmod 644 "$app/Contents/Resources/Rhebok.icns"

codesign --force --sign - --timestamp=none "$app" || fail "codesign failed"
codesign --verify --strict --deep "$app" || fail "the signed bundle does not verify"
[ "$(head -c 5 "$app/Contents/Info.plist")" = "<?xml" ] || fail "Info.plist is no longer XML after signing"

COPYFILE_DISABLE=1 tar --uid 0 --gid 0 --numeric-owner --no-xattrs --no-mac-metadata \
    -czf "$work/archive.tar.gz" -C "$stage" Rhebok.app

python3 - "$work/archive.tar.gz" "$notices" <<'PY' || fail "$archive is not packed as an app archive must be"
import sys, tarfile

dev = sys.argv[2] == "--dev"
files = [
    "Rhebok.app/Contents/Info.plist",
    "Rhebok.app/Contents/MacOS/rhebok-app",
    "Rhebok.app/Contents/Resources/Rhebok.icns",
    "Rhebok.app/Contents/Resources/LICENSE",
    "Rhebok.app/Contents/_CodeSignature/CodeResources",
]
if not dev:
    files.append("Rhebok.app/Contents/Resources/THIRD-PARTY-NOTICES.md")
with tarfile.open(sys.argv[1], "r:gz") as archive:
    members = archive.getmembers()
problems = []
actual_files = sorted(m.name for m in members if m.isreg())
if actual_files != sorted(files):
    problems.append("files are %s, expected %s" % (actual_files, sorted(files)))
for m in members:
    if m.name != "Rhebok.app" and not m.name.startswith("Rhebok.app/"):
        problems.append("%s is outside Rhebok.app" % m.name)
    if not (m.isreg() or m.isdir()):
        problems.append("%s is neither a regular file nor a directory" % m.name)
    if (m.uid, m.gid, m.uname, m.gname) != (0, 0, "", ""):
        problems.append("%s carries owner %s:%s (%s:%s)" % (m.name, m.uid, m.gid, m.uname, m.gname))
    if m.pax_headers:
        problems.append("%s carries extended headers %s" % (m.name, sorted(m.pax_headers)))
    if m.name == "Rhebok.app/Contents/MacOS/rhebok-app" and not m.mode & 0o111:
        problems.append("%s is not executable" % m.name)
for problem in problems:
    print("package-app: " + problem, file=sys.stderr)
sys.exit(1 if problems else 0)
PY
mv "$work/archive.tar.gz" "$archive" || fail "could not move the archive to $archive"
echo "package-app: wrote $archive"
