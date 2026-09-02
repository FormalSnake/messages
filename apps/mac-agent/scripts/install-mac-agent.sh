#!/usr/bin/env bash
# Installs the Find My agent as a launchd user agent, running under `bun` and
# restarted on crash and login. Run from a clone of the repo on the Mac.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
AGENT_DIR="$REPO_DIR/apps/mac-agent"
LABEL="es.canarycoders.messages.agent"
PLIST_PATH="$HOME/Library/LaunchAgents/$LABEL.plist"
LOG_DIR="$HOME/Library/Logs/messages"
BUN_PATH="$(command -v bun)"

cd "$REPO_DIR"
bun install

mkdir -p "$HOME/Library/LaunchAgents" "$LOG_DIR"

cat > "$PLIST_PATH" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>$BUN_PATH</string>
    <string>$AGENT_DIR/src/agent.ts</string>
  </array>
  <key>WorkingDirectory</key>
  <string>$AGENT_DIR</string>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>$LOG_DIR/agent.log</string>
  <key>StandardErrorPath</key>
  <string>$LOG_DIR/agent.log</string>
</dict>
</plist>
PLIST

launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST_PATH"

echo "installed $PLIST_PATH and started $LABEL"
echo "logs: $LOG_DIR/agent.log"
echo "check it: curl localhost:1236/health"
