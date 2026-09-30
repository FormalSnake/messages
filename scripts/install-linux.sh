#!/usr/bin/env bash
# Builds the release binary and installs a `messages` launcher, a desktop entry
# and the icon for the current user. The desktop entry is named after the
# window's app id, so GNOME and Hyprland group the window under it.
#
#   scripts/install-linux.sh              build, then install
#   scripts/install-linux.sh --no-build   install target/release/messages as it
#                                         is, e.g. one built on a faster box
set -euo pipefail

BUILD=1
[ "${1:-}" = --no-build ] && BUILD=

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS_DIR="$DATA_DIR/applications"
ICON_DIR="$DATA_DIR/icons/hicolor/512x512/apps"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/messages"
APP_ID=es.canarycoders.messages

mkdir -p "$BIN_DIR" "$APPS_DIR" "$ICON_DIR" "$STATE_DIR"
cd "$REPO_DIR"

LIBS=
if command -v nix >/dev/null 2>&1; then
  # The binary dlopens wayland, vulkan and fontconfig, which NixOS keeps off
  # the default search path. The profile is a GC root, so the store paths
  # behind LIBS survive nix-collect-garbage.
  [ -n "$BUILD" ] && nix develop "$REPO_DIR" --profile "$STATE_DIR/devshell" -c cargo build --release -p messages
  LIBS="$(nix develop "$REPO_DIR" --profile "$STATE_DIR/devshell" -c bash -c 'printf %s "$LD_LIBRARY_PATH"')"
elif [ -n "$BUILD" ]; then
  cargo build --release -p messages
fi
[ -x "$REPO_DIR/target/release/messages" ] || { echo "no target/release/messages to install" >&2; exit 1; }

cat > "$BIN_DIR/messages" <<LAUNCHER
#!/usr/bin/env bash
${LIBS:+export LD_LIBRARY_PATH="$LIBS\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"}
exec "$REPO_DIR/target/release/messages" "\$@"
LAUNCHER
chmod +x "$BIN_DIR/messages"

cp "$REPO_DIR/packaging/linux/$APP_ID.png" "$ICON_DIR/$APP_ID.png"
# Earlier versions installed a scalable SVG, which the theme would pick over the PNG.
rm -f "$DATA_DIR/icons/hicolor/scalable/apps/$APP_ID.svg"
sed "s|^Exec=messages|Exec=$BIN_DIR/messages|" "$REPO_DIR/packaging/linux/$APP_ID.desktop" > "$APPS_DIR/$APP_ID.desktop"

# Earlier versions installed messages.desktop, which would now list the app twice.
if grep -qs "^Exec=$BIN_DIR/messages$" "$APPS_DIR/messages.desktop"; then
  rm "$APPS_DIR/messages.desktop"
fi

update-desktop-database "$APPS_DIR" 2>/dev/null || true
gtk-update-icon-cache "$DATA_DIR/icons/hicolor" 2>/dev/null || true

echo "installed $BIN_DIR/messages and $APPS_DIR/$APP_ID.desktop"
echo "first run: MESSAGES_DEMO=1 messages   (fixtures)   or   messages   (asks for the server)"
