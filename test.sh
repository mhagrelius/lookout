#!/usr/bin/env bash
#
# Everything CI would run, in the order that fails fastest.
#
#   ./test.sh              against your own session
#   ./test.sh --headless   under Xvfb and a private D-Bus session
#
# The headless mode exists for the UI tests: GTK needs a display, and a test
# run must not attach to the developer's real session bus, where it would talk
# to a live instance of the app instead of itself.

set -euo pipefail

cd "$(dirname "$0")"

headless=false
if [[ "${1:-}" == "--headless" ]]; then
    headless=true
    shift
fi

# Accessibility bridges and the GSettings backend both reach out to session
# services that may not exist. Neither is under test.
export GTK_A11Y=none
export GSETTINGS_BACKEND=memory
export RUST_BACKTRACE=1

# Every XDG directory the app reads, pointed somewhere disposable.
#
# XDG_CONFIG_HOME is the DiskStation connection and XDG_DATA_HOME is the
# recorded history. Redirecting both matters more here than it looks: the
# application reads the config on startup and would otherwise connect to the
# developer's real NAS and start polling it during a test run.
xdg_home="$(mktemp -d)"
runtime_dir=""
cleanup() {
    rc=$?
    [[ -n "$runtime_dir" ]] && fusermount3 -u "$runtime_dir/doc" 2>/dev/null
    rm -rf "$xdg_home" ${runtime_dir:+"$runtime_dir"}
    exit $rc
}
trap cleanup EXIT
export XDG_DATA_HOME="$xdg_home/data"
export XDG_CONFIG_HOME="$xdg_home/config"
export XDG_STATE_HOME="$xdg_home/state"
export XDG_CACHE_HOME="$xdg_home/cache"
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"

# The private bus activates its own xdg-document-portal, which mounts a FUSE fs
# at $XDG_RUNTIME_DIR/doc. Inheriting the login session's runtime dir means that
# mount lands on /run/user/$UID/doc, on top of the real portal's; the real one
# exits 21 and every flatpak launch fails until it is restarted. Hand the
# session a throwaway runtime dir so its portals stay inside it.
if $headless; then
    runtime_dir="$(mktemp -d)"
    chmod 700 "$runtime_dir"
    export XDG_RUNTIME_DIR="$runtime_dir"
fi

run() {
    echo "==> $*"
    if $headless; then
        xvfb-run -a dbus-run-session -- "$@"
    else
        "$@"
    fi
}

# Formatting and lints need no display, so they never go through the wrapper.
echo "==> cargo fmt --check"
cargo fmt --all -- --check

echo "==> cargo clippy"
cargo clippy --workspace --all-targets -- -D warnings

# --workspace so lookout-core is tested too. Without it cargo checks only the
# root package, and the half of the suite that needs no display — which is most
# of it — silently stops running.
run cargo test --workspace --all-targets

echo
echo "All green."
