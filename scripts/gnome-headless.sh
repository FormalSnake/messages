#!/usr/bin/env bash
# Screenshots the app under a headless GNOME Shell of its own, so client-side
# decorations render the way they do on a GNOME desktop without touching one.
# GNOME Shell only takes screenshots for a few well-known D-Bus names, so the
# script claims the GNOME portal backend's name inside its private session.
#
#   scripts/gnome-headless.sh <launcher> <out.png>     e.g. ~/.local/bin/messages
#
# Extra env (MESSAGES_DEMO=1) passes through to the app.
set -euo pipefail

if [ -z "${GNOME_HEADLESS_INNER:-}" ]; then
  export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
  unset WAYLAND_DISPLAY DISPLAY
  exec nix shell --impure --expr 'with import <nixpkgs> {}; [ gnome-shell glib gobject-introspection (python3.withPackages (p: [ p.pygobject3 ])) ]' \
    -c env GNOME_HEADLESS_INNER=1 dbus-run-session -- bash "$0" "$@"
fi

bin=$1
out=$(realpath -m "$2")
dir=$(mktemp -d)
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$dir"' EXIT

gnome-shell --headless --wayland --virtual-monitor 1400x900 > "$dir/shell.log" 2>&1 &
for _ in $(seq 1 60); do grep -q "Using Wayland display name" "$dir/shell.log" && break; sleep 0.5; done
socket=$(grep -o "Using Wayland display name '[^']*'" "$dir/shell.log" | cut -d"'" -f2)
[ -n "$socket" ] || { echo "gnome-shell did not start" >&2; exit 1; }

WAYLAND_DISPLAY=$socket MESSAGES_TRACE=1 "$bin" > "$dir/app.log" 2>&1 &
sleep 8
grep -E "decorations|first paint" "$dir/app.log" || { tail -20 "$dir/app.log" >&2; exit 1; }

python3 - "$out" <<'PY'
import sys
from gi.repository import Gio, GLib
bus = Gio.bus_get_sync(Gio.BusType.SESSION)
bus.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName",
              GLib.Variant("(su)", ("org.freedesktop.impl.portal.desktop.gnome", 4)), GLib.VariantType("(u)"), 0, -1, None)
bus.call_sync("org.gnome.Shell.Screenshot", "/org/gnome/Shell/Screenshot", "org.gnome.Shell.Screenshot", "Screenshot",
              GLib.Variant("(bbs)", (False, False, sys.argv[1])), None, 0, -1, None)
PY
echo "shot=$out"
