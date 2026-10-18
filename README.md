<p align="center"><img src="packaging/linux/dev.formalsnake.messages.png" width="160" alt="Messages icon"></p>

# Messages

iMessage on Linux and macOS. A native Rust client on
[NativeDesktop](https://github.com/FormalSnake/NativeDesktop) (AppKit on macOS, GPUI on
Linux) that talks to a
[BlueBubbles server](https://github.com/BlueBubblesApp/bluebubbles-server)
running on your Mac.

![A group thread](docs/images/thread.png)

```
 Linux / macOS                          Mac
┌──────────────────┐  http + socket.io  ┌──────────────────────────────┐
│ Messages (Rust)  │ ─────────────────▶ │ BlueBubbles server           │
│ NativeDesktop    │ ◀───────────────── │ reads chat.db, drives        │
└──────────────────┘                    │ Messages.app (Private API)   │
                                        └──────────────────────────────┘
```

Threads update live, sends are optimistic and queue while offline, and the
window opens on the last known state before the server answers. It idles at
0% CPU.

| | |
|---|---|
| ![Details panel](docs/images/details.png) | ![Conversation switcher](docs/images/switcher.png) |

## Features

| | SIP on | Private API (SIP off) |
|---|---|---|
| Read and send text, photos, files, audio, stickers, link previews | yes | yes |
| Search with `from:`, `has:`, `before:`, `after:`, `in:` | yes | yes |
| Scheduled sends, notifications, Markdown export | yes | yes |
| Pin, mute, draft sync between clients | yes | yes |
| Tapbacks | receive | yes |
| Typing indicators, read receipts, replies, unsend, effects | no | yes |
| Rename groups, add and remove people | no | yes |

Optional extras: Find My locations on contact cards (needs the Mac agent),
GIF search through [Klipy](https://klipy.com), and summarize/translate/transcribe
through a [CanaryLLM](https://canaryllm.canarycoders.es) gateway.

Not working: FaceTime (upstream
[helper#38](https://github.com/BlueBubblesApp/bluebubbles-helper/issues/38),
[server#776](https://github.com/BlueBubblesApp/bluebubbles-server/issues/776)),
sending custom emoji tapbacks, and editing on macOS 26 (hidden until the
helper is fixed).

## Install

### Nix

```
nix run github:FormalSnake/messages
```

Home Manager:

```nix
{
  inputs.messages = {
    url = "github:FormalSnake/messages";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  # in your home-manager configuration
  imports = [ inputs.messages.homeModules.default ];

  programs.messages = {
    enable = true;
    autostart = true;                             # start with the graphical session
    environmentFile = "/run/agenix/messages.env"; # optional, server password etc.
    settings.notifications = true;                # typed, written to config.toml
    theme = { primary = "#6099c0"; };             # optional, writes theme.json
  };
}
```

This installs the app, its desktop entry and icon. `overlays.default` adds
`pkgs.messages`.

### Other Linux

Needs Rust 1.89+, Vulkan, and `libxkbcommon`, `wayland`, `vulkan-loader`,
`fontconfig`, `freetype`, `alsa-lib`. `ffmpeg` is optional (video posters,
playback).

```
git clone https://github.com/FormalSnake/messages
cd messages
./scripts/install-linux.sh
```

This installs `~/.local/bin/messages` and a desktop entry.

### Windows

Install [rustup](https://rustup.rs) and the Visual Studio C++ build tools, then build with the
msvc toolchain and install it with a Start menu shortcut:

```powershell
cargo +1.95.0-x86_64-pc-windows-msvc build --release -p messages
powershell -ExecutionPolicy Bypass -File packaging\windows\install.ps1
```

## Mac setup

1. Install the BlueBubbles server and give it Full Disk Access.
2. Turn off "Encrypt communications". Reach the Mac over Tailscale or a VPN.
3. For the Private API column: disable SIP and turn on Private API in
   BlueBubbles.

On first launch, paste the server address and password. `MESSAGES_DEMO=1
messages` runs on fixtures with no Mac.

## Configuration

Settings live in `config.toml` (`~/.config/messages/` on Linux,
`~/Library/Application Support/messages/` on macOS), editable from the settings window
(`Cmd+,`), from Nix as `programs.messages.settings`, or per key as `MESSAGES_<KEY>`
(`MESSAGES_SERVER_URL`, `MESSAGES_DEMO=1`):

```toml
server_url = "http://your-mac:1234"
agent_url = "http://your-mac:1236"
notifications = true
tray = true
```

Secrets stay in `~/.config/messages/config.json`, which the app writes itself (and which
also holds per-chat pins and mutes):

```json
{
  "agent": { "url": "http://your-mac:1236", "token": "..." },
  "klipy": { "apiKey": "..." },
  "canaryllm": { "apiKey": "...", "model": "gemini/gemini-2.5-flash-lite", "language": "Spanish" }
}
```

Environment overrides for the secrets: `MESSAGES_SERVER_PASSWORD`,
`MESSAGES_AGENT_TOKEN`, `MESSAGES_KLIPY_KEY`, `MESSAGES_CANARYLLM_KEY`.

The first launch after the GPUI build (app id `es.canarycoders.messages`) moves the server,
agent and notification settings out of `config.json` into `config.toml`; caches stay where
they were. The app id is now `dev.formalsnake.messages`.

On Linux, `~/.config/messages/theme.json` is a matugen palette in NativeDesktop's format
(Material roles such as `primary` and `surface`, per scheme), reloaded live.

### Mac agent

`apps/mac-agent` runs on the Mac and adds Find My locations, Apple Maps
snapshots, and pins, mutes and GIF favorites synced across clients.

1. `apps/mac-agent/scripts/install-mac-agent.sh` installs it as a launchd agent
   (port 1236, token in `~/.config/messages/agent.json`).
2. For Find My: boot with SIP off and
   `sudo nvram boot-args="amfi_get_out_of_my_way=1"`, then run
   `scripts/findmy-keys-mac.sh` once to extract the keys.
3. Add `agent` to the client config as above.

## Keyboard

| | |
|---|---|
| `Ctrl+N` | new message |
| `Ctrl+F` | search |
| `Ctrl+K` | jump to a conversation |
| `Ctrl+I` | conversation details |
| `Ctrl+[` / `Ctrl+]` | previous / next conversation |
| `Ctrl+1` to `Ctrl+6` | tapback the last message |
| `Shift+Ctrl+U` | mark unread |

`Cmd` on macOS. The clock beside the composer schedules a message.

## Development

```
MESSAGES_DEMO=1 cargo run --release -p messages   # fixtures
cargo test --workspace
nix build .#messages                              # Linux package
bun run agent                                     # the Mac agent
```

`crates/core` is the store, transport and BlueBubbles client with no UI
dependencies. `crates/desktop` is the window on NativeDesktop, checked out beside this repo
(`../nativedesktop`). `CLAUDE.md` has the architecture notes, `docs/nd-parity.md` what the
port keeps of the GPUI build.

## License

[MIT](LICENSE). The Noto Sans faces in `packaging/fonts` are under the
[OFL](packaging/fonts/OFL.txt).
