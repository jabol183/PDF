#!/usr/bin/env bash
# Downloads the pdfium build that matches pdfium-render's "pdfium_latest" API
# into vendor/pdfium/. Usage: scripts/fetch-pdfium.sh [linux-x64|win-x64|mac-arm64|mac-x64]
set -euo pipefail
PDFIUM_BUILD="${PDFIUM_BUILD:-7881}"
cd "$(dirname "$0")/.."
platforms=("${@:-linux-x64 win-x64}")
for platform in ${platforms[@]}; do
  dest="vendor/pdfium/$platform"
  mkdir -p "$dest"
  tmp="$(mktemp -d)"
  curl -fsSL -o "$tmp/pdfium.tgz" \
    "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F${PDFIUM_BUILD}/pdfium-${platform}.tgz"
  tar -xzf "$tmp/pdfium.tgz" -C "$tmp"
  case "$platform" in
    win-*) cp "$tmp/bin/pdfium.dll" "$dest/" ;;
    mac-*) cp "$tmp/lib/libpdfium.dylib" "$dest/" ;;
    *)     cp "$tmp/lib/libpdfium.so" "$dest/" ;;
  esac
  cp "$tmp/LICENSE" vendor/pdfium/PDFIUM-LICENSE
  rm -rf "$tmp"
  echo "pdfium ${PDFIUM_BUILD} -> $dest"
done
