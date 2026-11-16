#!/bin/sh
set -eu

usage="usage: scripts/package-release.sh <binary> <THIRD-PARTY-NOTICES.md> <archive.tar.gz>"
binary="${1:?$usage}"
notices="${2:?$usage}"
archive="${3:?$usage}"
root="$(cd "$(dirname "$0")/.." && pwd)"

fail() {
    echo "package-release: $*" >&2
    exit 1
}

[ -x "$binary" ] || fail "$binary is not an executable"
[ -s "$notices" ] || fail "$notices is missing or empty; generate it with scripts/third-party-notices.sh"
[ "$(sed -n 1p "$notices")" = "# Third-party notices" ] ||
    fail "$notices is not a notices file written by scripts/third-party-notices.sh"
[ -s "$root/LICENSE" ] || fail "$root/LICENSE is missing"

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
install -m 755 "$binary" "$stage/rhebok"
install -m 644 "$root/LICENSE" "$stage/LICENSE"
install -m 644 "$notices" "$stage/THIRD-PARTY-NOTICES.md"

if tar --version 2>/dev/null | grep -q 'GNU tar'; then
    tar --owner=0 --group=0 --numeric-owner \
        -czf "$archive" -C "$stage" rhebok LICENSE THIRD-PARTY-NOTICES.md
else
    COPYFILE_DISABLE=1 tar --uid 0 --gid 0 --numeric-owner --no-xattrs --no-mac-metadata \
        -czf "$archive" -C "$stage" rhebok LICENSE THIRD-PARTY-NOTICES.md
fi

python3 - "$archive" <<'PY' || fail "$archive is not packed as a release archive must be"
import sys, tarfile

expected = ["rhebok", "LICENSE", "THIRD-PARTY-NOTICES.md"]
with tarfile.open(sys.argv[1], "r:gz") as archive:
    members = archive.getmembers()
problems = []
if [m.name for m in members] != expected:
    problems.append("entries are %s, expected %s" % ([m.name for m in members], expected))
for m in members:
    if not m.isreg():
        problems.append("%s is not a regular file" % m.name)
    if (m.uid, m.gid, m.uname, m.gname) != (0, 0, "", ""):
        problems.append("%s carries owner %s:%s (%s:%s)" % (m.name, m.uid, m.gid, m.uname, m.gname))
    if m.pax_headers:
        problems.append("%s carries extended headers %s" % (m.name, sorted(m.pax_headers)))
for problem in problems:
    print("package-release: " + problem, file=sys.stderr)
sys.exit(1 if problems else 0)
PY
echo "package-release: wrote $archive"
