#!/usr/bin/env bash
# Extracts the three Find My keys on this Mac and hands them to the agent.
# Needs SIP off and a boot with amfi_get_out_of_my_way=1 (the extractor attaches
# lldb to Find My). Run it after that reboot; keys survive later reboots.
set -euo pipefail

# The extractor uses BSD stat/id/pkill; a Nix or Homebrew coreutils earlier in PATH breaks it.
export PATH="/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

EXTRACTOR="${FINDMY_EXTRACTOR:-$HOME/Developer/findmy-key-extractor}"
KEY_DIR="$HOME/.config/messages/findmy"
LABEL="es.canarycoders.messages.agent"

if [ "$(nvram boot-args 2>/dev/null | cut -f2- | tr -d '\t')" != "amfi_get_out_of_my_way=1" ]; then
  echo "boot-args do not contain amfi_get_out_of_my_way=1; lldb cannot attach to Find My" >&2
  exit 1
fi
if [ ! -x "$EXTRACTOR/extract.sh" ]; then
  echo "extractor not found at $EXTRACTOR (git clone https://github.com/manonstreet/findmy-key-extractor)" >&2
  exit 1
fi

cd "$EXTRACTOR"
[ -d .venv ] || ./extract.sh --setup
./extract.sh "$@"

mkdir -p "$KEY_DIR" && chmod 700 "$KEY_DIR"
for f in LocalStorage.key FMFDataManager.bplist FMIPDataManager.bplist; do
  if [ -s "keys/$f" ]; then
    install -m 600 "keys/$f" "$KEY_DIR/$f"
    echo "installed $f"
  else
    echo "missing keys/$f" >&2
  fi
done

launchctl kickstart -k "gui/$(id -u)/$LABEL" 2>/dev/null || true
sleep 2
curl -s --max-time 5 http://127.0.0.1:1236/health || echo "agent not answering on 1236"
echo
echo "boot-args are still set; clear them with: sudo nvram -d boot-args   (then reboot)"
