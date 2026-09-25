#!/usr/bin/env bash
# Installs the Rust `messages` binary's desktop entry and icon for the
# current user: `~/.local/share/applications` and the hicolor icon theme,
# so GNOME/Hyprland show a title, icon and alt-tab entry instead of a
# generic one. Run after `cargo build -p messages --release`.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"

BIN="$REPO_DIR/target/release/messages"
[ -x "$BIN" ] || BIN="$REPO_DIR/target/debug/messages"
[ -x "$BIN" ] || { echo "no messages binary; run cargo build -p messages first" >&2; exit 1; }

mkdir -p "$APPS_DIR" "$ICON_DIR" "$BIN_DIR"
ln -sf "$BIN" "$BIN_DIR/messages"
cp "$REPO_DIR/apps/desktop/assets/icon.svg" "$ICON_DIR/es.canarycoders.messages.svg"
sed "s|^Exec=messages|Exec=$BIN_DIR/messages|" \
  "$REPO_DIR/packaging/linux/es.canarycoders.messages.desktop" \
  > "$APPS_DIR/es.canarycoders.messages.desktop"

update-desktop-database "$APPS_DIR" 2>/dev/null || true
gtk-update-icon-cache "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" 2>/dev/null || true

echo "installed $BIN_DIR/messages, $APPS_DIR/es.canarycoders.messages.desktop and the app icon"
