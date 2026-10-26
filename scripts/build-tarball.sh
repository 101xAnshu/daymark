#!/usr/bin/env bash
# Build the portable per-user fallback archive: the release binary plus the
# existing install.sh, so no package manager or root is needed.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

BINARY="${ROOT}/target/release/daymark"
BUILD_DIR="${ROOT}/build/tarball"
OUT_DIR="${ROOT}/dist"

usage() {
    cat <<'EOF'
Usage: scripts/build-tarball.sh [--skip-build] [--help]

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

NAME="daymark-${VERSION}"
STAGE="${BUILD_DIR}/${NAME}"

umask 022
rm -rf "${BUILD_DIR}"
mkdir -p "${STAGE}/target/release" "${STAGE}/assets" "${OUT_DIR}"

install -m 0755 "${BINARY}" "${STAGE}/target/release/daymark"
install -m 0755 install.sh "${STAGE}/install.sh"
install -m 0644 assets/daymark.svg "${STAGE}/assets/daymark.svg"
install -m 0644 README.md "${STAGE}/README.md"
install -m 0644 LICENSE "${STAGE}/LICENSE"

cat >"${STAGE}/INSTALL.txt" <<EOF
Daymark ${VERSION} portable build for x86_64 Linux
=================================================

This archive contains a release build of Daymark and the per-user installer.

    ./install.sh --no-build

The installer writes only inside your home directory, enables autostart and
installs the GNOME Shell companion. Run ./install.sh --uninstall to remove it.
The first install needs one sign-out and sign-in so GNOME loads the companion.

For a system-wide installation on Ubuntu 22.04 or newer, use the .deb package
from the same release instead.
EOF

find "${STAGE}" -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

ARCHIVE="${OUT_DIR}/${NAME}-x86_64-linux.tar.gz"
rm -f "${ARCHIVE}"
tar \
    --sort=name \
    --mtime="@${SOURCE_DATE_EPOCH}" \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    -C "${BUILD_DIR}" \
    -cf - "${NAME}" |
    gzip -9n >"${ARCHIVE}"

echo "Built ${ARCHIVE}"
