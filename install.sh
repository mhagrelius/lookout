#!/usr/bin/env bash
#
# Release build, installed under ~/.local. `./uninstall.sh` reverses it.

set -euo pipefail
cd "$(dirname "$0")"

prefix="${PREFIX:-$HOME/.local}"
app_id="us.hagreli.Lookout"

echo "==> building"
cargo build --release

echo "==> installing to $prefix"
install -Dm755 target/release/lookout "$prefix/bin/lookout"
install -Dm644 "data/$app_id.desktop" "$prefix/share/applications/$app_id.desktop"
install -Dm644 "data/$app_id.metainfo.xml" "$prefix/share/metainfo/$app_id.metainfo.xml"
install -Dm644 "data/icons/hicolor/scalable/apps/$app_id.svg" \
    "$prefix/share/icons/hicolor/scalable/apps/$app_id.svg"
install -Dm644 "data/icons/hicolor/symbolic/apps/$app_id-symbolic.svg" \
    "$prefix/share/icons/hicolor/symbolic/apps/$app_id-symbolic.svg"

if command -v update-desktop-database >/dev/null; then
    update-desktop-database -q "$prefix/share/applications" || true
fi
if command -v gtk-update-icon-cache >/dev/null; then
    gtk-update-icon-cache -qtf "$prefix/share/icons/hicolor" 2>/dev/null || true
fi

echo "Installed. Run: lookout"
