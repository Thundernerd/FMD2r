#!/usr/bin/env bash
# Renders the home-screen icons in web/static (named in manifest.webmanifest and app.html) from
# web/static/favicon.svg. Needs rsvg-convert (librsvg).
#
# The square variant drops the favicon's rounded corners: the phone masks the icon to its own shape
# (Android's maskable icons, iOS's touch icon), and the glyph already sits inside the maskable safe
# zone (a centred circle 80% across).
set -euo pipefail
static="$(cd "$(dirname "$0")/../web/static" && pwd)"
svg="$static/favicon.svg"
square="$(sed 's/ rx="6"//' "$svg")"

rsvg-convert -w 192 -h 192 "$svg" -o "$static/icon-192.png"
rsvg-convert -w 512 -h 512 "$svg" -o "$static/icon-512.png"
rsvg-convert -w 512 -h 512 -o "$static/icon-maskable-512.png" <<<"$square"
rsvg-convert -w 180 -h 180 -o "$static/apple-touch-icon.png" <<<"$square"
echo "wrote icons to $static"
