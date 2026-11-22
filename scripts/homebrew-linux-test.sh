#!/bin/sh
set -eu

usage() {
  echo "usage: $0 [--arch arm64|amd64] [--formula <rhebok.rb>]" >&2
  exit 2
}

arch=""
formula=""
while [ $# -gt 0 ]; do
  case "$1" in
    --arch) arch="${2:-}"; shift 2 ;;
    --formula) formula="${2:-}"; shift 2 ;;
    -h|--help) usage ;;
    *) usage ;;
  esac
done

docker_cmd="${DOCKER:-docker}"
platform_args=""
case "$arch" in
  "") ;;
  arm64|amd64) platform_args="--platform linux/$arch" ;;
  *) usage ;;
esac

mount_args=""
host_arch="$(uname -m)"
case "$host_arch" in
  x86_64|amd64) host_arch=amd64 ;;
  aarch64|arm64) host_arch=arm64 ;;
esac
if [ -n "$arch" ] && [ "$arch" != "$host_arch" ]; then
  echo "refusing --arch $arch on an $host_arch host: under emulation Homebrew rejects the CPU or hangs." >&2
  echo "Run it on an $arch machine, or run the homebrew workflow, which tests both on GitHub's runners." >&2
  exit 2
fi
if [ -n "$formula" ]; then
  [ -f "$formula" ] || { echo "no such formula: $formula" >&2; exit 2; }
  formula_dir="$(cd "$(dirname "$formula")" && pwd)"
  mount_args="$mount_args -v $formula_dir/$(basename "$formula"):/formula/rhebok.rb:ro"
fi

"$docker_cmd" run --rm -i $platform_args $mount_args ubuntu:24.04 sh -s <<'INNER'
set -eu
step() { printf '\n==> %s\n' "$*"; }

step "system: $(uname -m), $(ldd --version | head -1)"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq build-essential curl git ca-certificates procps file sudo >/dev/null
useradd -m -s /bin/bash brewer
echo 'brewer ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/brewer

su - brewer -c 'sh -s' <<'USER'
set -eu
step() { printf '\n==> %s\n' "$*"; }

step "install Homebrew"
NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)" >/tmp/brew-install.log 2>&1 \
  || { tail -30 /tmp/brew-install.log; exit 1; }
eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_ENV_HINTS=1
brew --version | head -1

step "tap stellenbosch-technologies/tap"
brew tap stellenbosch-technologies/tap
if [ -f /formula/rhebok.rb ]; then
  cp /formula/rhebok.rb "$(brew --repository stellenbosch-technologies/tap)/Formula/rhebok.rb"
  echo "testing the local formula, not the published one"
fi

step "brew install rhebok"
brew install stellenbosch-technologies/tap/rhebok

step "brew test rhebok"
brew test stellenbosch-technologies/tap/rhebok

step "rhebok on PATH"
command -v rhebok
rhebok --version

step "index two notes without Ollama, then search them"
mkdir -p ~/notes
printf '# Billing deploys\nWe drain the green pool for ninety seconds before the cutover.\n' > ~/notes/deploys.md
printf '# Sourdough\nFeed the starter at a 1:5:5 ratio.\n' > ~/notes/sourdough.md
rhebok config set sources '["~/notes"]'
rhebok index --no-embed
out="$(rhebok search "drain the green pool" 2>/dev/null)"
printf '%s\n' "$out" | head -20
printf '%s\n' "$out" | grep -q 'deploys.md' || { echo "FAIL: search did not find deploys.md"; exit 1; }

step "PASS: Homebrew installs a working rhebok on $(uname -m) Linux"
USER
INNER
