#!/bin/sh
# PartyLine for Linux and macOS: downloads the latest release into ~/.local/bin and starts it.
#   curl -fsSL https://raw.githubusercontent.com/luannzin/partyline/main/install.sh | sh
# Arguments after `sh -s --` go to the game, e.g. `| sh -s -- --bots 3`.
set -eu

case "$(uname -s)" in
  Linux)
    [ "$(uname -m)" = x86_64 ] || { echo "PartyLine: Linux builds are x86_64 only for now." >&2; exit 1; }
    asset=partyline-linux-x86_64 ;;
  Darwin) asset=partyline-macos ;;
  *) echo "PartyLine: no build for $(uname -s). There are builds for Windows, Linux and macOS." >&2; exit 1 ;;
esac

dir="$HOME/.local/bin"
mkdir -p "$dir"
echo "Downloading PartyLine..."
curl -fL --progress-bar "https://github.com/luannzin/partyline/releases/latest/download/$asset" -o "$dir/partyline.part"
chmod +x "$dir/partyline.part"
mv -f "$dir/partyline.part" "$dir/partyline"

echo "Installed to $dir/partyline. It keeps itself up to date."
case ":$PATH:" in
  *":$dir:"*) echo "Next time just run: partyline" ;;
  *) echo "Next time run: $dir/partyline (or add $dir to your PATH)" ;;
esac
# The game needs the keyboard, not this script's pipe.
exec "$dir/partyline" "$@" </dev/tty
