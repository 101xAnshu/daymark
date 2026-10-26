#!/usr/bin/env bash
# Build a self-contained Daymark .deb without root.
#
# The binary, desktop entry, icon and GNOME Shell companion are staged into a
# temporary tree and handed to dpkg-deb. Set SOURCE_DATE_EPOCH to pin file
# timestamps; otherwise the latest commit time is used.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

BINARY="${ROOT}/target/release/daymark"
BUILD_DIR="${ROOT}/build/deb"
OUT_DIR="${ROOT}/dist"
EXT_UUID="daymark@daymark.app"
MAINTAINER="Daymark contributors <maintainers@daymark.app>"

usage() {
    cat <<'EOF'
Usage: scripts/build-deb.sh [--skip-build] [--help]

  --skip-build   use an existing target/release/daymark
EOF
}

BUILD=1
for arg in "$@"; do
    case "${arg}" in
        --skip-build) BUILD=0 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: ${arg}" >&2; usage >&2; exit 2 ;;
    esac
done

if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    epoch="$(git -C "${ROOT}" log -1 --format=%ct 2>/dev/null || true)"
    if [ -z "${epoch}" ]; then
        epoch="$(date +%s)"
    fi
    SOURCE_DATE_EPOCH="${epoch}"
fi
export SOURCE_DATE_EPOCH

if [ "${BUILD}" = 1 ]; then
    cargo build --release --locked
fi

[ -x "${BINARY}" ] || {
    echo "No release build at ${BINARY}. Run without --skip-build." >&2
    exit 1
}

VERSION="$(awk '
    /^\[package\]/ { in_package = 1; next }
    /^\[/ { in_package = 0 }
    in_package && /^version *=/ {
        gsub(/.*version *= *"|".*/, "", $0)
        print
        exit
    }
' Cargo.toml)"
[ -n "${VERSION}" ] || {
    echo "Could not read the package version from Cargo.toml." >&2
    exit 1
}

umask 022
rm -rf "${BUILD_DIR}"
mkdir -p \
    "${BUILD_DIR}/DEBIAN" \
    "${BUILD_DIR}/usr/bin" \
    "${BUILD_DIR}/usr/share/applications" \
    "${BUILD_DIR}/usr/share/icons/hicolor/scalable/apps" \
    "${BUILD_DIR}/usr/share/gnome-shell/extensions/${EXT_UUID}" \
    "${BUILD_DIR}/usr/share/doc/daymark"
mkdir -p "${OUT_DIR}"

install -m 0755 "${BINARY}" "${BUILD_DIR}/usr/bin/daymark"
install -m 0644 packaging/daymark.desktop \
    "${BUILD_DIR}/usr/share/applications/daymark.desktop"
install -m 0644 assets/daymark.svg \
    "${BUILD_DIR}/usr/share/icons/hicolor/scalable/apps/daymark.svg"
install -m 0644 extension/metadata.json \
    "${BUILD_DIR}/usr/share/gnome-shell/extensions/${EXT_UUID}/metadata.json"
install -m 0644 extension/extension.js \
    "${BUILD_DIR}/usr/share/gnome-shell/extensions/${EXT_UUID}/extension.js"
install -m 0644 packaging/debian/copyright \
    "${BUILD_DIR}/usr/share/doc/daymark/copyright"

cat >"${BUILD_DIR}/usr/share/doc/daymark/changelog" <<EOF
daymark (${VERSION}) unstable; urgency=medium

  * Release ${VERSION}.

 -- ${MAINTAINER}  $(date -u -R -d "@${SOURCE_DATE_EPOCH}")
EOF
gzip -9n "${BUILD_DIR}/usr/share/doc/daymark/changelog"

install -m 0755 packaging/debian/postinst "${BUILD_DIR}/DEBIAN/postinst"
install -m 0755 packaging/debian/postrm "${BUILD_DIR}/DEBIAN/postrm"

installed_size="$(
    find "${BUILD_DIR}" -path "${BUILD_DIR}/DEBIAN" -prune -o -type f -printf '%s\n' |
        awk '{ total += int(($1 + 1023) / 1024) } END { print total + 0 }'
)"

cat >"${BUILD_DIR}/DEBIAN/control" <<EOF
Package: daymark
Version: ${VERSION}
Architecture: amd64
Installed-Size: ${installed_size}
Maintainer: ${MAINTAINER}
Section: utils
Priority: optional
Depends: libc6 (>= 2.35), libgcc-s1
Recommends: gnome-shell (>= 45)
Description: quiet local screen time journal for GNOME
 Daymark records how long each application is used and presents a calm
 daily view, the shape of the week, and fourteen days of history.
 Everything is stored in a small SQLite file on the local machine.
 .
 A tiny GNOME Shell companion extension reports focus, idle and lock
 state. Daymark opens no web view, makes no network requests, and needs
 no account.
 .
 This package installs the application, desktop entry, icon and the
 companion extension.
EOF

# Keep the archive reproducible: every payload and control file gets the same
# timestamp. dpkg-deb records ownership as root:root via --root-owner-group.
find "${BUILD_DIR}" -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

DEB="${OUT_DIR}/daymark_${VERSION}_amd64.deb"
rm -f "${DEB}"
dpkg-deb --build --root-owner-group -Zxz -z9 "${BUILD_DIR}" "${DEB}" >/dev/null

echo "Built ${DEB}"
