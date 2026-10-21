#!/usr/bin/env bash
# Sign a built PurePoint.app inside-out with one identity, then verify that
# every Mach-O in the bundle carries that signature with the hardened runtime.
#
# Usage: sign-macos-app.sh <PurePoint.app> <identity>
#   identity "-" signs ad-hoc (CI proof); a Developer ID identity also gets a
#   secure timestamp, which notarization requires.
#
# The release workflow builds the app unsigned in one job and runs only this
# script (plus Apple's tools) in the job that holds the signing identity, so no
# third-party build code runs while the identity is available.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <PurePoint.app> <identity>" >&2
  exit 2
fi
app=$(cd "$1" && pwd)
identity=$2
repo=$(cd "$(dirname "$0")/../.." && pwd)

flags=(--force --sign "$identity" --options runtime)
runtime_flags=()
if [ "$identity" != "-" ]; then
  flags+=(--timestamp)
  runtime_flags+=(--timestamp)
fi
sign() { /usr/bin/codesign "${flags[@]}" "$@"; }

# Point Guard runtime: native modules under Resources/PointGuard and the
# executables in Contents/Helpers (Node gets its allow-jit entitlement).
python3 "$repo/apps/purepoint-mobile/runtime/sign-runtime.py" \
  --app "$app" --identity "$identity" ${runtime_flags[@]+"${runtime_flags[@]}"}

# Rust binaries the Xcode build phase places next to the app executable.
for name in pu-engine pu pu-point-guard-lock; do
  sign "$app/Contents/MacOS/$name"
done

# Embedded frameworks. Sparkle's nested helpers are signed innermost first, as
# Sparkle documents for signing outside Xcode.
for framework in "$app"/Contents/Frameworks/*; do
  [ -e "$framework" ] || continue
  if [ "$(basename "$framework")" = Sparkle.framework ]; then
    current="$framework/Versions/B"
    sign "$current/XPCServices/Installer.xpc"
    sign --preserve-metadata=entitlements "$current/XPCServices/Downloader.xpc"
    sign "$current/Autoupdate"
    sign "$current/Updater.app"
  fi
  sign "$framework"
done

# The app itself, last.
sign --entitlements "$repo/apps/purepoint-macos/purepoint-macos/PurePoint.entitlements" "$app"

/usr/bin/codesign --verify --deep --strict --verbose=2 "$app"

# --verify --deep does not look at Mach-O files outside the standard nested
# code locations (e.g. Node native modules in Resources), so check each one.
macho_files() {
  python3 - "$app" <<'PY'
import os, sys
magics = {b'\xcf\xfa\xed\xfe', b'\xce\xfa\xed\xfe', b'\xfe\xed\xfa\xcf', b'\xfe\xed\xfa\xce', b'\xca\xfe\xba\xbe'}
for root, dirs, files in os.walk(sys.argv[1]):
    for name in files:
        path = os.path.join(root, name)
        if os.path.islink(path):
            continue
        with open(path, 'rb') as stream:
            if stream.read(4) in magics:
                sys.stdout.write(path + '\0')
PY
}
failed=0
checked=0
while IFS= read -r -d '' file; do
  checked=$((checked + 1))
  details=$(/usr/bin/codesign -dvv "$file" 2>&1) || { echo "unsigned: $file" >&2; failed=1; continue; }
  if ! grep -q 'flags=.*runtime' <<<"$details"; then
    echo "no hardened runtime: $file" >&2; failed=1
  fi
  if [ "$identity" != "-" ]; then
    grep -q '^Authority=Developer ID Application' <<<"$details" || { echo "not Developer ID signed: $file" >&2; failed=1; }
    grep -q '^Timestamp=' <<<"$details" || { echo "no secure timestamp: $file" >&2; failed=1; }
  fi
done < <(macho_files)
echo "Checked $checked Mach-O files"
if [ "$failed" -ne 0 ] || [ "$checked" -eq 0 ]; then
  exit 1
fi
echo "Signed and verified: $app"
