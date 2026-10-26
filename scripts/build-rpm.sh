#!/usr/bin/env bash
# Build a Fedora .rpm without root. The release binary and the same desktop,
# icon, extension and documentation files as the .deb are staged into a
# source archive, then rpmbuild packages that archive.
#
# rpmlint is not required. Fedora's rpm-build package provides rpmbuild:
#   Fedora: sudo dnf install rpm-build
#   Ubuntu: sudo apt install rpm
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
    x86_64|amd64) HOST_ARCH="x86_64" ;;
    aarch64|arm64) HOST_ARCH="aarch64" ;;
    *) HOST_ARCH="" ;;
esac

usage() {
    cat <<'EOF'
Usage: scripts/build-rpm.sh [--arch x86_64|aarch64] [--skip-build] [--help]

  --arch ARCH    package architecture (default: host architecture)
  --skip-build   use an existing release binary for the selected architecture

Cross builds use cargo-zigbuild and need Zig available to it. For example,
on an x86_64 host:

  scripts/build-rpm.sh --arch aarch64
EOF
}

BUILD=1
TARGET_ARCH=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --arch)
            [ "$#" -ge 2 ] || {
                echo "--arch needs a value: x86_64 or aarch64." >&2
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
    echo "Could not detect the host architecture; pass --arch x86_64 or aarch64." >&2
    exit 1
}

case "${ARCH}" in
    x86_64) RUST_TARGET="x86_64-unknown-linux-gnu" ;;
    aarch64) RUST_TARGET="aarch64-unknown-linux-gnu" ;;
    *)
        echo "Unsupported RPM architecture: ${ARCH}. Use x86_64 or aarch64." >&2
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

BUILD_DIR="${ROOT}/build/rpm-${ARCH}"
SOURCE_DIR="${BUILD_DIR}/source"
RPMBUILD_DIR="${BUILD_DIR}/rpmbuild"

command -v rpmbuild >/dev/null 2>&1 || {
    echo "rpmbuild was not found. Install rpm-build (Fedora) or rpm (Ubuntu)." >&2
    exit 1
}

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

STAGE="${SOURCE_DIR}/daymark-${VERSION}"
umask 022
rm -rf "${BUILD_DIR}"
mkdir -p \
    "${STAGE}/usr/bin" \
    "${STAGE}/usr/share/applications" \
    "${STAGE}/usr/share/icons/hicolor/scalable/apps" \
    "${STAGE}/usr/share/gnome-shell/extensions/${EXT_UUID}" \
    "${STAGE}/usr/share/doc/daymark" \
    "${STAGE}/usr/lib/systemd/user" \
    "${RPMBUILD_DIR}/SOURCES" \
    "${RPMBUILD_DIR}/SPECS" \
    "${RPMBUILD_DIR}/BUILD" \
    "${RPMBUILD_DIR}/RPMS" \
    "${OUT_DIR}"

install -m 0755 "${BINARY}" "${STAGE}/usr/bin/daymark"
install -m 0644 packaging/daymark.service \
    "${STAGE}/usr/lib/systemd/user/daymark.service"
install -m 0644 packaging/daymark.desktop \
    "${STAGE}/usr/share/applications/daymark.desktop"
install -m 0644 assets/daymark.svg \
    "${STAGE}/usr/share/icons/hicolor/scalable/apps/daymark.svg"
install -m 0644 extension/metadata.json \
    "${STAGE}/usr/share/gnome-shell/extensions/${EXT_UUID}/metadata.json"
install -m 0644 extension/extension.js \
    "${STAGE}/usr/share/gnome-shell/extensions/${EXT_UUID}/extension.js"
install -m 0644 packaging/debian/copyright \
    "${STAGE}/usr/share/doc/daymark/copyright"

cat >"${STAGE}/usr/share/doc/daymark/changelog" <<EOF
daymark (${VERSION}) unstable; urgency=medium

  * Release ${VERSION}.

 -- ${MAINTAINER}  $(date -u -R -d "@${SOURCE_DATE_EPOCH}")
EOF

SOURCE_ARCHIVE="${RPMBUILD_DIR}/SOURCES/daymark-${VERSION}.tar.gz"
tar \
    --sort=name \
    --mtime="@${SOURCE_DATE_EPOCH}" \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    -C "${SOURCE_DIR}" \
    -cf - "daymark-${VERSION}" |
    gzip -9n >"${SOURCE_ARCHIVE}"

# Keep the committed spec valid for a manual rpmbuild, while making the script
# authoritative for the version and architecture. Everything above %changelog
# is reused. A foreign BuildArch is rejected by rpmbuild, so a cross build
# drops that line and relies on --target instead.
SPEC="${RPMBUILD_DIR}/SPECS/daymark.spec"
if [ "${CROSS}" = 1 ]; then
    ARCH_SED=(-e '/^BuildArch:/d')
else
    ARCH_SED=(-e "s/^BuildArch:.*/BuildArch:      ${ARCH}/")
fi
awk '/^%changelog/ { exit } { print }' packaging/rpm/daymark.spec |
    sed -e "s/^Version:.*/Version:        ${VERSION}/" "${ARCH_SED[@]}" >"${SPEC}"
{
    echo "%changelog"
    printf '* %s %s - %s-1\n' \
        "$(LC_ALL=C date -u -d "@${SOURCE_DATE_EPOCH}" '+%a %b %d %Y')" \
        "${MAINTAINER}" \
        "${VERSION}"
    echo "- Release ${VERSION}."
} >>"${SPEC}"

RPMBUILD_ARGS=(
    --define "_topdir ${RPMBUILD_DIR}"
    --define "_buildhost reproducible"
    --define "use_source_date_epoch_as_buildtime 1"
    --define "clamp_mtime_to_source_date_epoch 1"
)
if [ "${CROSS}" = 1 ]; then
    RPMBUILD_ARGS+=(--target "${ARCH}")
fi
rpmbuild "${RPMBUILD_ARGS[@]}" -bb "${SPEC}" >/dev/null

shopt -s nullglob
rpms=("${RPMBUILD_DIR}/RPMS/${ARCH}/"*.rpm)
shopt -u nullglob
[ "${#rpms[@]}" -eq 1 ] || {
    echo "Expected one built RPM, found ${#rpms[@]}." >&2
    exit 1
}

RPM="${OUT_DIR}/daymark-${VERSION}-1.${ARCH}.rpm"
rm -f "${RPM}"
cp "${rpms[0]}" "${RPM}"
echo "Built ${RPM}"
