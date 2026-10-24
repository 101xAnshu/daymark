#!/usr/bin/env bash
# Install Daymark for the current user. No root privileges are required.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
BIN="${BIN_DIR}/daymark"
APP_DIR="${HOME}/.local/share/applications"
ICON_DIR="${HOME}/.local/share/icons/hicolor/scalable/apps"
AUTOSTART_DIR="${HOME}/.config/autostart"
EXT_UUID="daymark@daymark.app"

usage() {
    cat <<'EOF'
Usage: ./install.sh [--no-build] [--x11] [--uninstall [--purge]]

  --no-build       use an existing target/release/daymark
  --x11            accepted for compatibility; the X11 backend is built in
  --uninstall      remove the app, desktop entry, autostart entry and extension
  --purge          with --uninstall, also delete all recorded history
EOF
}

uninstall_extension() {
    command -v gnome-extensions >/dev/null 2>&1 && \
        gnome-extensions disable "${EXT_UUID}" >/dev/null 2>&1 || true
    if command -v gsettings >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
        python3 - "${EXT_UUID}" <<'PY' >/dev/null 2>&1 || true
import ast
import subprocess
import sys

uuid = sys.argv[1]
for key in ("enabled-extensions", "disabled-extensions"):
    result = subprocess.run(
        ["gsettings", "get", "org.gnome.shell", key],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        continue
    text = result.stdout.strip()
    try:
        values = ast.literal_eval(text)
    except (ValueError, SyntaxError):
        values = []
    values = [value for value in values if value != uuid]
    rendered = "[" + ", ".join("'" + str(value).replace("'", "") + "'" for value in values) + "]"
    subprocess.run(
        ["gsettings", "set", "org.gnome.shell", key, rendered],
        check=False, capture_output=True,
    )
PY
    fi
    rm -rf "${HOME}/.local/share/gnome-shell/extensions/${EXT_UUID}"
}

case "${1:-}" in
    -h|--help)
        usage
        exit 0
        ;;
    --uninstall|--remove)
        "${BIN}" --quit >/dev/null 2>&1 || true
        uninstall_extension
        rm -f "${BIN}"
        rm -f "${APP_DIR}/daymark.desktop"
        rm -f "${AUTOSTART_DIR}/daymark.desktop"
        rm -f "${ICON_DIR}/daymark.svg"
        if [ "${2:-}" = "--purge" ]; then
            rm -rf "${HOME}/.local/share/daymark" "${HOME}/.config/daymark"
            echo "Removed Daymark and all local history."
        else
            echo "Removed Daymark. History was kept in ~/.local/share/daymark."
            echo "Use --purge to delete it."
        fi
        exit 0
        ;;
esac

BUILD=1
FEATURES=""
for arg in "$@"; do
    case "${arg}" in
        --no-build) BUILD=0 ;;
        --x11) FEATURES="--features x11" ;;
        *) echo "Unknown option: ${arg}" >&2; usage; exit 2 ;;
    esac
done

if [ "${BUILD}" = 1 ]; then
    command -v cargo >/dev/null 2>&1 || {
        echo "cargo was not found. Install Rust from https://rustup.rs and try again." >&2
        exit 1
    }
    command -v pkg-config >/dev/null 2>&1 || {
        echo "pkg-config is required. Install it with: sudo apt install pkg-config" >&2
        exit 1
    }
    (cd "${ROOT}" && cargo build --release ${FEATURES})
fi

[ -x "${ROOT}/target/release/daymark" ] || {
    echo "No release build found. Run ./install.sh without --no-build." >&2
    exit 1
}

mkdir -p "${BIN_DIR}" "${APP_DIR}" "${ICON_DIR}" "${AUTOSTART_DIR}"
install -m 0755 "${ROOT}/target/release/daymark" "${BIN}"
install -m 0644 "${ROOT}/assets/daymark.svg" "${ICON_DIR}/daymark.svg"

cat > "${APP_DIR}/daymark.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Daymark
Comment=A quiet screen time journal
Exec=${BIN}
TryExec=${BIN}
Icon=daymark
Terminal=false
Categories=Utility;
StartupNotify=true
StartupWMClass=daymark
Keywords=screen time;usage;focus;activity;
EOF

cat > "${AUTOSTART_DIR}/daymark.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Daymark
Comment=Local screen time tracking
Exec=${BIN} --background
TryExec=${BIN}
Icon=daymark
Terminal=false
StartupNotify=false
Categories=Utility;
X-GNOME-Autostart-enabled=true
EOF

if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "${APP_DIR}/daymark.desktop" || true
fi
command -v update-desktop-database >/dev/null 2>&1 &&     update-desktop-database "${APP_DIR}" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 &&     gtk-update-icon-cache -q -t -f "${HOME}/.local/share/icons/hicolor" >/dev/null 2>&1 || true

# Install and enable the small GNOME companion. A first install is picked up at
# the next login on Wayland; the app explains this in its setup sheet.
"${BIN}" --setup-tracking || {
    echo "Daymark is installed, but the GNOME companion could not be enabled." >&2
    echo "You can retry later from Settings → Set up tracking." >&2
}

echo
echo "Installed: ${BIN}"
echo "Launch Daymark from the application grid, or run: daymark"
echo "Tracking starts at login. Close the window to keep tracking in the background."
echo "If this is the first install, sign out and back in once so GNOME loads the companion."
