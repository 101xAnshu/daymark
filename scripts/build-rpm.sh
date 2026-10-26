#!/usr/bin/env bash
# Build a Fedora .rpm without root. The release binary and the same desktop,
# icon, extension and documentation files as the .deb are staged into a
# source archive, then rpmbuild packages that archive.
#
# rpmlint is not required. Fedora's rpm-build package provides rpmbuild:
#   Fedora: sudo dnf install rpm-build
#   Ubuntu: sudo apt install rpm
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT}"

BINARY="${ROOT}/target/release/daymark"
BUILD_DIR="${ROOT}/build/rpm"
SOURCE_DIR="${BUILD_DIR}/source"
RPMBUILD_DIR="${BUILD_DIR}/rpmbuild"
OUT_DIR="${ROOT}/dist"
EXT_UUID="daymark@daymark.app"
MAINTAINER="Daymark contributors <maintainers@daymark.app>"

usage() {
    cat <<'EOF'
Usage: scripts/build-rpm.sh [--skip-build] [--help]

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

STAGE="${SOURCE_DIR}/daymark-${VERSION}"
umask 022
rm -rf "${BUILD_DIR}"
mkdir -p \
    "${STAGE}/usr/bin" \
    "${STAGE}/usr/share/applications" \
    "${STAGE}/usr/share/icons/hicolor/scalable/apps" \
    "${STAGE}/usr/share/gnome-shell/extensions/${EXT_UUID}" \
    "${STAGE}/usr/share/doc/daymark" \
    "${RPMBUILD_DIR}/SOURCES" \
    "${RPMBUILD_DIR}/SPECS" \
    "${RPMBUILD_DIR}/BUILD" \
    "${RPMBUILD_DIR}/RPMS" \
    "${OUT_DIR}"

install -m 0755 "${BINARY}" "${STAGE}/usr/bin/daymark"
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
# authoritative for the version and changelog. Everything above %changelog is
# reused; the version line is replaced from Cargo.toml.
SPEC="${RPMBUILD_DIR}/SPECS/daymark.spec"
awk '/^%changelog/ { exit } { print }' packaging/rpm/daymark.spec |
    sed "s/^Version:.*/Version:        ${VERSION}/" >"${SPEC}"
{
    echo "%changelog"
    printf '* %s %s - %s-1\n' \
        "$(LC_ALL=C date -u -d "@${SOURCE_DATE_EPOCH}" '+%a %b %d %Y')" \
        "${MAINTAINER}" \
        "${VERSION}"
    echo "- Release ${VERSION}."
} >>"${SPEC}"

rpmbuild \
    --define "_topdir ${RPMBUILD_DIR}" \
    --define "_buildhost reproducible" \
    --define "use_source_date_epoch_as_buildtime 1" \
    --define "clamp_mtime_to_source_date_epoch 1" \
    -bb "${SPEC}" >/dev/null

shopt -s nullglob
rpms=("${RPMBUILD_DIR}/RPMS/x86_64/"*.rpm)
shopt -u nullglob
[ "${#rpms[@]}" -eq 1 ] || {
    echo "Expected one built RPM, found ${#rpms[@]}." >&2
    exit 1
}

rm -f "${OUT_DIR}/daymark-${VERSION}-1.x86_64.rpm"
cp "${rpms[0]}" "${OUT_DIR}/"
echo "Built ${OUT_DIR}/$(basename "${rpms[0]}")"
