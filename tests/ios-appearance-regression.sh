#!/bin/bash
# Run against a disposable, booted iPhone 18 Pro simulator with a signed-out
# Field build already installed. This changes its appearance and foreground app.
set -euo pipefail
: "${FIELD_TEST_SIMULATOR:?Set FIELD_TEST_SIMULATOR to the disposable simulator UDID}"
device="$FIELD_TEST_SIMULATOR"
bundle=com.quantumbox.tachyonfield
tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/field-appearance.XXXXXX")
trap 'rm -rf "$tmpdir"' EXIT

appearance() { xcrun simctl ui "$device" appearance "$1"; }
foreground() { xcrun simctl launch "$device" "$bundle" >/dev/null; }
background() { xcrun simctl openurl "$device" 'App-Prefs:root=DISPLAY'; }
check() {
  local expected="$1" image="$tmpdir/screen.png" bitmap="$tmpdir/screen.bmp"
  # Cold start and app-switch animations can take several seconds on CI.
  for attempt in {1..12}; do
    sleep 2
    xcrun simctl io "$device" screenshot "$image" >/dev/null 2>&1
    sips -s format bmp "$image" --out "$bitmap" >/dev/null
    if python3 - "$bitmap" "$expected" <<'PY'
import struct, sys
path, expected = sys.argv[1:]
data = open(path, 'rb').read()
offset = struct.unpack_from('<I', data, 10)[0]
width, height, planes, bits = struct.unpack_from('<iiHH', data, 18)
if (width, height, planes, bits) != (1206, -2622, 1, 32):
    raise SystemExit(f'Expected iPhone 18 Pro screenshot; got {width}x{height}, {bits}bpp')
stride = ((width * bits // 8) + 3) & ~3
def brightness(x, y):
    i = offset + y * stride + x * 4
    return sum(data[i:i+3]) / 3
# The title stroke and nearby backdrop distinguish both mismatched directions.
title, backdrop = brightness(240, 1110), brightness(10, 1100)
if expected == 'light':
    ok = title < 60 and backdrop > 150
else:
    ok = title > 200 and backdrop < 100
if not ok:
    raise SystemExit(f'{expected} mismatch: title={title:.0f}, backdrop={backdrop:.0f}')
print(f'{expected}: title={title:.0f}, backdrop={backdrop:.0f}')
PY
    then return 0; fi
  done
  echo "Appearance did not settle to $expected" >&2
  return 1
}

xcrun simctl terminate "$device" "$bundle" >/dev/null 2>&1 || true
appearance dark
foreground
check dark
background
appearance light
foreground
check light
background
appearance dark
foreground
check dark
appearance light
check light
appearance dark
check dark
