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
require_command chmod
require_command grep
require_command install
require_command mktemp
require_command mv
require_command rm
require_command sha256sum
require_command sed
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

if [ -n "${PAKPOS_DATA_HOME:-}" ]; then
    data_home="$PAKPOS_DATA_HOME"
elif [ -n "${XDG_DATA_HOME:-}" ]; then
    data_home="$XDG_DATA_HOME"
elif [ -n "${HOME:-}" ]; then
    data_home="$HOME/.local/share"
else
    fail "HOME is not set; provide PAKPOS_DATA_HOME"
fi

applications_dir="${data_home}/applications"
icons_dir="${data_home}/icons/hicolor/scalable/apps"

temporary_dir="$(mktemp -d)"
staged_binary=""
staged_desktop=""
staged_icon=""

cleanup() {
    if [ -n "$staged_binary" ] && [ -e "$staged_binary" ]; then
        rm -f -- "$staged_binary"
    fi
    if [ -n "$staged_desktop" ] && [ -e "$staged_desktop" ]; then
        rm -f -- "$staged_desktop"
    fi
    if [ -n "$staged_icon" ] && [ -e "$staged_icon" ]; then
        rm -f -- "$staged_icon"
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
case "$install_dir" in
    /*) ;;
    *) install_dir=$(CDPATH= cd "./${install_dir}" && pwd -P) ;;
esac
staged_binary="${install_dir}/.pakpos.install.$$"
install -m 0755 "${temporary_dir}/pakpos" "$staged_binary"
mv -f -- "$staged_binary" "${install_dir}/pakpos"
staged_binary=""

desktop_source="${temporary_dir}/share/applications/com.bughowi.Pakpos.desktop"
icon_source="${temporary_dir}/share/icons/hicolor/scalable/apps/com.bughowi.Pakpos.svg"

# Releases older than the desktop integration contain only the executable. Keep
# those versions installable and give them a launcher with the system's generic
# development icon.
if [ ! -f "$desktop_source" ]; then
    desktop_source="${temporary_dir}/com.bughowi.Pakpos.desktop"
    {
        printf '%s\n' \
            '[Desktop Entry]' \
            'Type=Application' \
            'Name=Pakpos' \
            'GenericName=HTTP Client' \
            'Comment=Test HTTP APIs with a native desktop client' \
            'Exec=pakpos' \
            'Icon=applications-development' \
            'Terminal=false' \
            'Categories=Development;' \
            'Keywords=HTTP;HTTPS;API;REST;cURL;' \
            'StartupNotify=true' > "$desktop_source"
    }
fi

install -d "$applications_dir"

# Desktop entries do not inherit an interactive shell's PATH consistently.
# Quote and escape the installed executable's absolute path per the Desktop
# Entry Specification so launching works from graphical menus on every distro.
case "${install_dir}/pakpos" in
    *'
'*) fail "the install path must not contain a newline" ;;
esac
desktop_exec=$(printf '%s' "${install_dir}/pakpos" \
    | sed 's/\\/\\\\/g; s/"/\\"/g; s/`/\\`/g; s/\$/\\$/g; s/%/%%/g')
staged_desktop="${applications_dir}/.com.bughowi.Pakpos.desktop.install.$$"
while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
        Exec=pakpos) printf 'Exec="%s"\n' "$desktop_exec" ;;
        *) printf '%s\n' "$line" ;;
    esac
done < "$desktop_source" > "$staged_desktop"
chmod 0644 "$staged_desktop"
mv -f -- "$staged_desktop" "${applications_dir}/com.bughowi.Pakpos.desktop"
staged_desktop=""

if [ -f "$icon_source" ]; then
    install -d "$icons_dir"
    staged_icon="${icons_dir}/.com.bughowi.Pakpos.svg.install.$$"
    install -m 0644 "$icon_source" "$staged_icon"
    mv -f -- "$staged_icon" "${icons_dir}/com.bughowi.Pakpos.svg"
    staged_icon=""

    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache --quiet --ignore-theme-index "${data_home}/icons/hicolor" \
            >/dev/null 2>&1 || :
    fi
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$applications_dir" >/dev/null 2>&1 || :
fi

say "Pakpos installed to ${install_dir}/pakpos"
say "Desktop launcher installed to ${applications_dir}/com.bughowi.Pakpos.desktop"
say "Pakpos requires GTK 4.10 or newer and GtkSourceView 5 at runtime."

case ":${PATH:-}:" in
    *:"$install_dir":*) ;;
    *) say "Add ${install_dir} to PATH before running pakpos." ;;
esac
