#!/usr/bin/env bash
# Downloads Stockfish into engines/stockfish (gitignored). Linux and macOS.
# Usage: scripts/setup-stockfish.sh [version] [destination-dir]
set -euo pipefail

VERSION="${1:-sf_19}"
DEST="${2:-$(cd "$(dirname "$0")/.." && pwd)/engines}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  ASSET="stockfish-linux-x86-64-universal.tar.gz" ;;
  Linux-aarch64) ASSET="stockfish-linux-arm64-universal.tar.gz" ;;
  Darwin-*)      ASSET="stockfish-macos-universal.tar.gz" ;;
  *) echo "Unsupported platform: $(uname -s)-$(uname -m). Download Stockfish manually and set STOCKFISH_PATH." >&2; exit 1 ;;
esac

URL="https://github.com/official-stockfish/Stockfish/releases/download/${VERSION}/${ASSET}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "Downloading $URL"
curl -fL --retry 3 -o "$WORK/$ASSET" "$URL"
mkdir "$WORK/extracted"
tar -xzf "$WORK/$ASSET" -C "$WORK/extracted"

BIN="$(find "$WORK/extracted" -type f -name 'stockfish*' -perm -u+x | grep -Ev '/src/|/scripts/' | head -n 1)"
if [ -z "$BIN" ]; then
  echo "No Stockfish executable found inside $ASSET" >&2
  exit 1
fi

mkdir -p "$DEST"
cp "$BIN" "$DEST/stockfish"
chmod +x "$DEST/stockfish"

REPLY="$(printf 'uci\nquit\n' | "$DEST/stockfish")"
case "$REPLY" in
  *uciok*) echo "Installed $(printf '%s\n' "$REPLY" | grep 'id name') at $DEST/stockfish" ;;
  *) echo "$DEST/stockfish did not answer the UCI handshake" >&2; exit 1 ;;
esac
