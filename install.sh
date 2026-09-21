#!/bin/sh

set -eu

repository="albugowy15/pakpos"
asset="pakpos-linux-x86_64.tar.gz"
version="${1:-${PAKPOS_VERSION:-latest}}"

say() {
    printf '%s\n' "$*"
}

fail() {
    say "pakpos installer: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

case "$(uname -s)" in
    Linux) ;;
    *) fail "only Linux is currently supported" ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) ;;
    *) fail "only x86_64 is currently supported" ;;
esac

require_command curl
require_command grep
require_command install
require_command mktemp
require_command mv
require_command rm
require_command sha256sum
require_command tar
require_command uname

if [ "$version" = "latest" ]; then
    release_url="https://github.com/${repository}/releases/latest/download"
else
    if ! printf '%s\n' "$version" | grep -Eq '^v?[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'; then
        fail "invalid version '$version' (expected latest, vX.Y.Z, or X.Y.Z)"
    fi
    case "$version" in
        v*) ;;
        *) version="v${version}" ;;
    esac
    release_url="https://github.com/${repository}/releases/download/${version}"
fi

if [ -n "${PAKPOS_INSTALL_DIR:-}" ]; then
    install_dir="$PAKPOS_INSTALL_DIR"
elif [ -n "${XDG_BIN_HOME:-}" ]; then
    install_dir="$XDG_BIN_HOME"
elif [ -n "${HOME:-}" ]; then
    install_dir="$HOME/.local/bin"
else
    fail "HOME is not set; provide PAKPOS_INSTALL_DIR"
fi

temporary_dir="$(mktemp -d)"
staged_binary=""

cleanup() {
    if [ -n "$staged_binary" ] && [ -e "$staged_binary" ]; then
        rm -f -- "$staged_binary"
    fi
    if [ -n "$temporary_dir" ] && [ -d "$temporary_dir" ]; then
        rm -rf -- "$temporary_dir"
    fi
}

trap cleanup EXIT HUP INT TERM

say "Downloading Pakpos ${version}..."
curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
    "${release_url}/${asset}" \
    --output "${temporary_dir}/${asset}"
curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
    "${release_url}/${asset}.sha256" \
    --output "${temporary_dir}/${asset}.sha256"

(
    cd "$temporary_dir"
    sha256sum --check "${asset}.sha256"
    tar -xzf "$asset"
)

[ -f "${temporary_dir}/pakpos" ] || fail "release archive does not contain pakpos"

install -d "$install_dir"
staged_binary="${install_dir}/.pakpos.install.$$"
install -m 0755 "${temporary_dir}/pakpos" "$staged_binary"
mv -f -- "$staged_binary" "${install_dir}/pakpos"
staged_binary=""

say "Pakpos installed to ${install_dir}/pakpos"
say "Pakpos requires GTK 4.10 or newer and GtkSourceView 5 at runtime."

case ":${PATH:-}:" in
    *:"$install_dir":*) ;;
    *) say "Add ${install_dir} to PATH before running pakpos." ;;
esac
