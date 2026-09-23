#!/usr/bin/env sh
# Renders og/card.html to public/og.png at exactly the 1200x630 the social
# networks want. Chrome does the drawing, so the card can be written in the
# same CSS as the site.
#
#   npm run og
#   CHROME=/path/to/chrome npm run og
set -eu

chrome=${CHROME:-}
if [ -z "$chrome" ]; then
  for candidate in \
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
    "/Applications/Chromium.app/Contents/MacOS/Chromium" \
    "$(command -v google-chrome || true)" \
    "$(command -v chromium || true)" \
    "$(command -v chromium-browser || true)"; do
    if [ -n "$candidate" ] && [ -x "$candidate" ]; then
      chrome=$candidate
      break
    fi
  done
fi

if [ -z "$chrome" ]; then
  echo "no chrome found: set CHROME to one" >&2
  exit 1
fi

cd "$(dirname "$0")/.."
"$chrome" --headless --disable-gpu --hide-scrollbars \
  --window-size=1200,630 --virtual-time-budget=8000 \
  --screenshot=public/og.png "file://$PWD/og/card.html" >/dev/null 2>&1

echo "public/og.png $(file -b public/og.png)"
