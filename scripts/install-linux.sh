#!/usr/bin/env bash
# Installs a `messages` launcher and a desktop entry for the current user.
# Run from a clone of the repo on NixOS (or any distro with bun and nix).
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/messages"

cd "$REPO_DIR"
bun install
mkdir -p "$BIN_DIR" "$APPS_DIR" "$STATE_DIR"

# The prebuilt renderer dlopens wayland, vulkan and friends. Resolve the dev
# shell once and keep it as a GC root so the store paths outlive nix-collect-garbage.
LIBS="$(nix develop "$REPO_DIR" --profile "$STATE_DIR/devshell" -c bash -c 'printf %s "$LD_LIBRARY_PATH"')"

cat > "$BIN_DIR/messages" <<LAUNCHER
#!/usr/bin/env bash
export LD_LIBRARY_PATH="$LIBS\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
cd "$REPO_DIR/apps/desktop"
exec bun app.tsx "\$@"
LAUNCHER
chmod +x "$BIN_DIR/messages"

cat > "$APPS_DIR/messages.desktop" <<ENTRY
[Desktop Entry]
Type=Application
Name=Messages
Comment=iMessage on Linux
Exec=$BIN_DIR/messages
Icon=$REPO_DIR/apps/desktop/assets/icon.svg
Terminal=false
Categories=Network;InstantMessaging;
Keywords=iMessage;SMS;chat;
ENTRY

echo "installed $BIN_DIR/messages and $APPS_DIR/messages.desktop"
echo "first run: MESSAGES_DEMO=1 messages   (fixtures)   or   messages   (asks for the server)"
