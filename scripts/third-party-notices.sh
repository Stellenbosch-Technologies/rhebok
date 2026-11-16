#!/bin/sh
set -eu

out="${1:?usage: scripts/third-party-notices.sh <output.md>}"
cargo_about_version=0.9.3

cd "$(dirname "$0")/.."

fail() {
    echo "third-party-notices: $*" >&2
    exit 1
}

installed="$(cargo about --version 2>/dev/null || true)"
[ "$installed" = "cargo-about $cargo_about_version" ] ||
    fail "needs cargo-about $cargo_about_version (found: ${installed:-none}); cargo install --locked --features cli cargo-about@$cargo_about_version"
[ -d dashboard/node_modules ] ||
    fail "dashboard/node_modules is missing; run npm ci --ignore-scripts in dashboard first"

lbug_dir="$(cargo metadata --format-version 1 --locked | python3 -c '
import json, os, sys
crates = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "lbug" and p["source"]]
if len(crates) != 1:
    sys.exit("expected one registry lbug package, found %d" % len(crates))
print(os.path.dirname(crates[0]["manifest_path"]))
')" || fail "cargo metadata could not locate the lbug crate"
third_party="$lbug_dir/lbug-src/third_party"
[ -d "$third_party" ] || fail "$third_party does not exist"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fenced() {
    printf '````text\n'
    cat "$1"
    printf '\n````\n\n'
}

linked_lbug_libraries() {
    cat <<'MAP'
alp file LICENSE
antlr4_runtime file LICENSE
brotli file LICENSE
fast_float header include/fast_float.h DEALINGS IN THE SOFTWARE.
fastpfor file LICENSE
glob header glob/glob.hpp SOFTWARE.
httplib mit-header httplib.h MIT License
lz4 file LICENSE
mbedtls file LICENSE
miniz file LICENSE
parquet file LICENSE
pcg file LICENSE
pyparse header pyparse.h SOFTWARE.
re2 file LICENSE
roaring_bitmap header roaring.h SPDX-License-Identifier: MIT
simsimd file LICENSE
snappy file LICENSE
thrift file LICENSE
utf8proc file LICENSE.md
yyjson file LICENSE
zstd file LICENSE
MAP
}

unlinked_lbug_libraries="antlr4_cypher cppjieba ports pybind11 spdlog taywee_args"

header_through() {
    line="$(grep -n -F -m 1 -- "$2" "$1" | cut -d: -f1)"
    [ -n "$line" ] || fail "$1 no longer contains \"$2\"; its licence block moved, so update the map in $0"
    head -n "$line" "$1"
}

lbug_licence_text() {
    dir="$third_party/$1"
    [ -d "$dir" ] || fail "LadybugDB no longer bundles $1; update the map in $0"
    case "$2" in
        file) [ -s "$dir/$3" ] || fail "$dir/$3 is missing or empty"
              cat "$dir/$3" ;;
        header) header_through "$dir/$3" "$4" ;;
        mit-header) header_through "$dir/$3" "$4"
                    printf '\n'
                    cat scripts/licenses/mit-permission-notice.txt ;;
        *) fail "unknown licence source $2 for $1" ;;
    esac
}

for dir in "$third_party"/*/; do
    name="$(basename "$dir")"
    linked_lbug_libraries | awk -v n="$name" '$1 == n { found = 1 } END { exit !found }' && continue
    case " $unlinked_lbug_libraries " in *" $name "*) continue ;; esac
    fail "LadybugDB bundles $name, which $0 neither maps to a licence nor lists as unlinked"
done

linked_lbug_libraries > "$work/lbug.map"
: > "$work/lbug.md"
while read -r name kind path marker; do
    lbug_licence_text "$name" "$kind" "$path" "$marker" > "$work/lbug-licence.txt"
    grep -q -i 'licen[cs]e\|permission is hereby granted\|redistribution and use' "$work/lbug-licence.txt" ||
        fail "the licence text found for $name carries no licence terms"
    { printf '### %s\n\n' "$name"; fenced "$work/lbug-licence.txt"; } >> "$work/lbug.md"
done < "$work/lbug.map"

cargo about generate --locked --fail about.hbs -o "$work/rust.md"
node scripts/dashboard-notices.mjs dashboard > "$work/dashboard.md"

{
    printf '# Third-party notices\n\n'
    printf 'rhebok is licensed under the GNU Affero General Public License v3.0 only; see LICENSE.\n'
    printf 'The rhebok binary also contains the third-party software listed below, each under its own licence.\n\n'
    cat "$work/rust.md"

    printf '## LadybugDB bundled libraries\n\n'
    printf 'The LadybugDB static library (lbug %s) links these libraries from its third_party directory.\n\n' \
        "$(basename "$lbug_dir" | sed 's/^lbug-//')"
    cat "$work/lbug.md"

    printf '## OpenSSL\n\n'
    printf 'The macOS binary links OpenSSL 3 statically; the Linux binaries load the system libssl at run time.\n'
    printf 'OpenSSL 3 is Copyright The OpenSSL Project Authors and is licensed under the Apache License 2.0,\n'
    printf 'reproduced in full in the Rust crates section above.\n\n'

    printf '## Dashboard JavaScript packages\n\n'
    printf 'The dashboard served by rhebok dashboard is compiled from these npm packages.\n\n'
    cat "$work/dashboard.md"
    printf '\n'

    printf '## Dashboard fonts\n\n'
    for font in dashboard/dist/fonts/*-OFL.txt; do
        [ -f "$font" ] || fail "no OFL licence texts in dashboard/dist/fonts"
        printf '### %s\n\n' "$(basename "$font" .txt)"
        fenced "$font"
    done
} > "$work/notices.md"

mv "$work/notices.md" "$out"
echo "third-party-notices: wrote $out"
