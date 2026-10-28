#!/usr/bin/env bash
# build_and_run.sh — one command from source to a running Sotto.app.
#
#   npm run app:start          (or: bash scripts/build_and_run.sh)
#
# Steps: toolchain check → signing identity → JS deps → type-check + Vite
# build of the UI → release build + bundle (.app and .dmg) → quit any running
# copy → launch the new one.

set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n\033[1;36m▶ %s\033[0m\n' "$*"; }

step "Toolchain check"
command -v cargo >/dev/null || { echo "Rust is missing: https://rustup.rs" >&2; exit 1; }
command -v cmake >/dev/null || { echo "cmake is missing (needed to build whisper.cpp): brew install cmake" >&2; exit 1; }
command -v npm >/dev/null || { echo "Node.js/npm is missing: brew install node" >&2; exit 1; }

step "Code signing identity"
# A real signature keeps macOS privacy approvals (Microphone, Accessibility)
# across rebuilds. Ad-hoc signing makes every build a "new app" to macOS, so
# the user would have to approve it again each time.
if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  APPLE_SIGNING_IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
    | awk '/"(Apple Development|Developer ID Application):/ {print $2; exit}')
fi
if [ -n "$APPLE_SIGNING_IDENTITY" ]; then
  export APPLE_SIGNING_IDENTITY
  echo "signing with certificate $APPLE_SIGNING_IDENTITY"
else
  echo "WARNING: no Apple Development certificate found — ad-hoc signing." >&2
  echo "         macOS will ask for Microphone/Accessibility again after every rebuild." >&2
  echo "         (Xcode → Settings → Accounts → Manage Certificates → + Apple Development)" >&2
fi

step "JS dependencies"
[ -d node_modules ] || npm install
[ -d ui/node_modules ] || npm install --prefix ui

step "Frontend: type-check + production build (Tailwind v4 via Vite)"
npm run build --prefix ui

step "Release build (Sotto + whisper.cpp with Metal)"
# frontend already built above; strip beforeBuildCommand to avoid doing it twice
npx tauri build --bundles app --config '{"build": {"beforeBuildCommand": ""}}'

APP="src-tauri/target/release/bundle/macos/Sotto.app"
[ -d "$APP" ] || { echo "no .app bundle found" >&2; exit 1; }
codesign --verify --strict "$APP" || { echo "the app's code signature is invalid" >&2; exit 1; }

step "Launching $APP"
# quit a running copy first so the new build is the one that starts
pkill -f "Sotto.app/Contents/MacOS/" 2>/dev/null && sleep 1 || true
open "$APP"
