#!/bin/sh
# Installs the latest rhebok release: download, verify the checksum, then let
# the binary install itself.
#
#   curl -fsSL https://github.com/Stellenbosch-Technologies/rhebok/releases/latest/download/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --yes      # unattended: pulls the model without asking
#
# A person runs this, so it exits non-zero with the reason on every failure.
# RHEBOK_RELEASE_BASE overrides the download prefix; file:// works.
set -eu

BASE="${RHEBOK_RELEASE_BASE:-https://github.com/Stellenbosch-Technologies/rhebok/releases/latest/download}"

os="$(uname -s)"; arch="$(uname -m)"
case "$os" in
    Darwin) os_part=apple-darwin ;;
    Linux)  os_part=unknown-linux-gnu ;;
    *) echo "rhebok: unsupported operating system: $os" >&2; exit 1 ;;
esac
case "$arch" in
    arm64|aarch64) arch_part=aarch64 ;;
    x86_64|amd64)  arch_part=x86_64 ;;
    *) echo "rhebok: unsupported architecture: $arch" >&2; exit 1 ;;
esac
target="$arch_part-$os_part"
if [ "$target" = x86_64-apple-darwin ]; then
    echo "rhebok: Intel Macs are not supported yet, not even by building from source; docs/install.md says why" >&2
    exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
tgz="rhebok-$target.tar.gz"
sha="rhebok-$target.sha256"

curl -fsSL "$BASE/$tgz" -o "$tmp/$tgz" || { echo "rhebok: could not download $BASE/$tgz" >&2; exit 1; }
curl -fsSL "$BASE/$sha" -o "$tmp/$sha" || { echo "rhebok: could not download $BASE/$sha" >&2; exit 1; }

sha256_of() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    elif command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        return 1
    fi
}

actual="$(sha256_of "$tmp/$tgz")" || { echo "rhebok: no sha256 tool (shasum or sha256sum) to verify the download" >&2; exit 1; }
expected="$(awk '{print $1}' "$tmp/$sha")"
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
    echo "rhebok: checksum mismatch for $tgz: expected $expected, got $actual" >&2; exit 1
fi

tar -xzf "$tmp/$tgz" -C "$tmp"
[ -x "$tmp/rhebok" ] || { echo "rhebok: $tgz does not contain an executable named rhebok" >&2; exit 1; }

if [ "$os_part" = apple-darwin ]; then
    app_tgz="rhebok-app-$target.tar.gz"
    app_sha="rhebok-app-$target.sha256"
    app_failed=""
    curl -fsSL "$BASE/$app_tgz" -o "$tmp/$app_tgz" || app_failed="$app_tgz"
    if [ -z "$app_failed" ]; then
        curl -fsSL "$BASE/$app_sha" -o "$tmp/$app_sha" || app_failed="$app_sha"
    fi
    if [ -n "$app_failed" ]; then
        echo "rhebok: could not download the app ($BASE/$app_failed); rhebok install will try again" >&2
        rm -f "$tmp/$app_tgz" "$tmp/$app_sha"
    else
        app_actual="$(sha256_of "$tmp/$app_tgz" || true)"
        app_expected="$(awk '{print $1}' "$tmp/$app_sha")"
        if [ -z "$app_actual" ] || [ "$app_expected" != "$app_actual" ]; then
            echo "rhebok: warning: the app archive failed its checksum, so it was discarded; rhebok install will try again" >&2
            rm -f "$tmp/$app_tgz" "$tmp/$app_sha"
        fi
    fi
fi

# `curl | sh` leaves stdin holding the script, so the model-pull prompt
# would read the script. Hand the install a terminal when there is one.
if (exec 3</dev/tty) 2>/dev/null; then
    "$tmp/rhebok" install "$@" </dev/tty
else
    "$tmp/rhebok" install "$@"
fi
