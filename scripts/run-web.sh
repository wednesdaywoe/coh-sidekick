#!/usr/bin/env bash
# Build the web app and serve it at http://127.0.0.1:3000/ (or the port given), opening a browser
# tab. dx watches the source: an edit rebuilds and reloads the page. Ctrl+C stops it.
#
#   scripts/run-web.sh          # port 3000
#   scripts/run-web.sh 9000     # another port
#
# A debug build with no cloud settings, so sign-in and cloud saves are off; deploy-web.sh is the
# configured release build.
set -euo pipefail
cd "$(dirname "$0")/../crates/app"

port="${1:-3000}"
exec dx serve --platform web --addr 127.0.0.1 --port "$port" --open true
