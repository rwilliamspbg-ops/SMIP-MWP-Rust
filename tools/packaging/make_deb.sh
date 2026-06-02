#!/usr/bin/env bash
set -euo pipefail

VERSION=${1:-0.0.0-local}
ARCH=${2:-amd64}

PKGDIR=$(mktemp -d)
trap 'rm -rf "$PKGDIR"' EXIT

echo "Creating package in $PKGDIR"

mkdir -p "$PKGDIR/DEBIAN"
mkdir -p "$PKGDIR/usr/local/bin"
chmod 0755 "$PKGDIR/DEBIAN"

# Find release binaries
if [ -d target/release ]; then
  BIN_DIR=target/release
else
  echo "No target/release directory found" >&2
  exit 1
fi

shopt -s nullglob
for f in "$BIN_DIR"/*; do
  if [ -x "$f" ] && [ ! -d "$f" ]; then
    echo "Adding binary $(basename "$f")"
    cp "$f" "$PKGDIR/usr/local/bin/"
  fi
done

cat > "$PKGDIR/DEBIAN/control" <<EOF
Package: smip-mwp
Version: $VERSION
Section: base
Priority: optional
Architecture: $ARCH
Maintainer: SMIP-MWP Dev <devnull@example.com>
Description: SMIP-MWP datapath stack (minimal packaging)
EOF
chmod 0644 "$PKGDIR/DEBIAN/control"

OUT=dist/smip-mwp_${VERSION}_${ARCH}.deb
mkdir -p dist
dpkg-deb --build "$PKGDIR" "$OUT"
echo "Created $OUT"
