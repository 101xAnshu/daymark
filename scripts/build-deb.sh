#!/usr/bin/env bash
# Build a self-contained Daymark .deb without root.
#
# The binary, desktop entry, icon and GNOME Shell companion are staged into a
# temporary tree and handed to dpkg-deb. Set SOURCE_DATE_EPOCH to pin file
# timestamps; otherwise the latest commit time is used.
#
# The host architecture is packaged by default. Pass --arch to package another
# supported architecture; a cross build uses cargo-zigbuild.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

OUT_DIR="${ROOT}/dist"
EXT_UUID="daymark@daymark.app"
MAINTAINER="Daymark contributors <maintainers@daymark.app>"
GLIBC_FLOOR="2.35"

HOST_UNAME="$(uname -m)"
case "${HOST_UNAME}" in
    x86_64|amd64) HOST_ARCH="amd64" ;;
    aarch64|arm64) HOST_ARCH="arm64" ;;
    *) HOST_ARCH="" ;;
esac

usage() {
    cat <<'EOF'
Usage: scripts/build-deb.sh [--arch amd64|arm64] [--skip-build] [--help]

  --arch ARCH    package architecture (default: host architecture)
  --skip-build   use an existing release binary for the selected architecture

Cross builds use cargo-zigbuild and need Zig available to it. For example,
on an amd64 host:

  scripts/build-deb.sh --arch arm64
EOF
}

BUILD=1
TARGET_ARCH=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --arch)
            [ "$#" -ge 2 ] || {
                echo "--arch needs a value: amd64 or arm64." >&2
                exit 2
            }
            TARGET_ARCH="$2"
            shift 2
            ;;
        --arch=*)
            TARGET_ARCH="${1#*=}"
            shift
            ;;
        --skip-build)
            BUILD=0
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

ARCH="${TARGET_ARCH:-${HOST_ARCH}}"
[ -n "${ARCH}" ] || {
    echo "Could not detect the host architecture; pass --arch amd64 or arm64." >&2
    exit 1
}

case "${ARCH}" in
    amd64) RUST_TARGET="x86_64-unknown-linux-gnu" ;;
    arm64) RUST_TARGET="aarch64-unknown-linux-gnu" ;;
    *)
        echo "Unsupported Debian architecture: ${ARCH}. Use amd64 or arm64." >&2
        exit 2
        ;;
esac

if [ -n "${HOST_ARCH}" ] && [ "${ARCH}" = "${HOST_ARCH}" ]; then
    CROSS=0
    BINARY="${ROOT}/target/release/daymark"
else
    CROSS=1
    BINARY="${ROOT}/target/${RUST_TARGET}/release/daymark"
fi

BUILD_DIR="${ROOT}/build/deb-${ARCH}"

if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    epoch="$(git -C "${ROOT}" log -1 --format=%ct 2>/dev/null || true)"
    if [ -z "${epoch}" ]; then
        epoch="$(date +%s)"
    fi
    SOURCE_DATE_EPOCH="${epoch}"
fi
export SOURCE_DATE_EPOCH

if [ "${BUILD}" = 1 ]; then
    if [ "${CROSS}" = 1 ]; then
        if ! command -v cargo-zigbuild >/dev/null 2>&1; then
            echo "Cross-building ${ARCH} needs cargo-zigbuild and Zig." >&2
            echo "Install them, or build the target and pass --skip-build." >&2
            exit 1
        fi
        cargo zigbuild --release --locked \
            --target "${RUST_TARGET}.${GLIBC_FLOOR}"
    else
        cargo build --release --locked
    fi
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
    "${BUILD_DIR}/usr/share/doc/daymark" \
    "${BUILD_DIR}/usr/lib/systemd/user"
mkdir -p "${OUT_DIR}"

install -m 0755 "${BINARY}" "${BUILD_DIR}/usr/bin/daymark"
install -m 0644 packaging/daymark.service \
    "${BUILD_DIR}/usr/lib/systemd/user/daymark.service"
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
Architecture: ${ARCH}
Installed-Size: ${installed_size}
Maintainer: ${MAINTAINER}
Section: utils
Priority: optional
Depends: libc6 (>= 2.35), libgcc-s1
Recommends: gnome-shell (>= 46), gnome-shell-extension-appindicator | gnome-shell-ubuntu-extensions
Description: quiet local screen time journal for GNOME
 Daymark records how long each application is used and presents a calm
 daily view, the shape of the week, and fourteen days of history.
 Everything is stored in a small SQLite file on the local machine.
 .
 A tiny GNOME Shell companion extension reports focus, idle and lock
 state. Daymark opens no web view, makes no network requests, and needs
 no account.
 .
 GNOME Shell 46 or newer is required; on Ubuntu that means 24.04 or newer.
 .
 This package installs the application, desktop entry, icon and the
 companion extension.
EOF

# Keep the archive reproducible: every payload and control file gets the same
# timestamp. dpkg-deb records ownership as root:root via --root-owner-group.
find "${BUILD_DIR}" -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

DEB="${OUT_DIR}/daymark_${VERSION}_${ARCH}.deb"
rm -f "${DEB}"
dpkg-deb --build --root-owner-group -Zxz -z9 "${BUILD_DIR}" "${DEB}" >/dev/null

echo "Built ${DEB}"
