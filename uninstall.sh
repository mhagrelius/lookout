#!/usr/bin/env bash
#
# Reverses install.sh. Leaves the config and recorded history alone; those are
# the user's, and removing them on an upgrade would be a surprise.

set -euo pipefail

prefix="${PREFIX:-$HOME/.local}"
app_id="us.hagreli.Lookout"

rm -f "$prefix/bin/lookout" \
      "$prefix/share/applications/$app_id.desktop" \
      "$prefix/share/metainfo/$app_id.metainfo.xml" \
      "$prefix/share/icons/hicolor/scalable/apps/$app_id.svg" \
      "$prefix/share/icons/hicolor/symbolic/apps/$app_id-symbolic.svg"

echo "Removed. Config and history remain in ~/.config/lookout and ~/.local/share/lookout."
