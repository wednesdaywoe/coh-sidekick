#!/usr/bin/env bash
# Put WebKit's helper programs inside an RC AppImage, point its WebKit library at them, and leave
# Wayland to the host.
#
#   scripts/keys/rc-appimage-webkit.sh path/to/Sidekick.AppImage     (rewrites it in place)
#
# Run on the Ubuntu that built the AppImage: the helpers must come from the same WebKit install
# as the library linuxdeploy bundled, and the script refuses when the two differ.
#
# Why (linux-appimage-2). linuxdeploy bundles libwebkit2gtk-4.1.so.0 but not the programs it starts
# -- WebKitNetworkProcess, WebKitWebProcess, the injected bundle -- and the library looks for them
# at Ubuntu's compiled-in path. On Fedora that folder does not exist and the app exits at launch;
# on a host that does have it, the host's helpers talk to our library, which breaks as soon as the
# versions differ. This is the approach Tauri's AppImages ship:
#
# - the helpers are copied to the same place under usr/, and their RUNPATH set so they load the
#   AppImage's libraries rather than the host's;
# - "/usr/lib/x86_64-linux-gnu" in the library becomes "././/lib/x86_64-linux-gnu" -- the same
#   length, so the bytes can be replaced in place -- which resolves against the working directory;
# - AppRun becomes a launcher that starts the app with its working directory at $APPDIR/usr.
#   The app never reads its working directory (checked 2026-10-17), and the AppImage runtime keeps
#   the original one in $OWD.
#
# The repack reuses the AppImage's own runtime (the bytes before --appimage-offset) and its
# squashfs compression, so nothing is downloaded here.
set -euo pipefail

app=$(realpath "${1:?path to the AppImage}")
system=/usr/lib/x86_64-linux-gnu
helpers=$system/webkit2gtk-4.1
[ -d "$helpers" ] || { echo "no $helpers here: run this on the Ubuntu that built the AppImage" >&2; exit 2; }
for tool in unsquashfs mksquashfs patchelf readelf; do
  command -v "$tool" >/dev/null || { echo "need $tool (apt: squashfs-tools patchelf binutils)" >&2; exit 2; }
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Where the squashfs starts: the end of the runtime ELF, read from its header rather than by running
# the AppImage to ask (--appimage-offset gives the same number).
offset=$(python3 -c 'import struct, sys
h = open(sys.argv[1], "rb").read(64)
shoff, = struct.unpack_from("<Q", h, 0x28)
shentsize, shnum = struct.unpack_from("<HH", h, 0x3A)
print(shoff + shentsize * shnum)' "$app")
comp=$(unsquashfs -s -o "$offset" "$app" | sed -n 's/^Compression //p')
unsquashfs -no-xattrs -q -o "$offset" -d "$work/AppDir" "$app" >/dev/null
appdir=$work/AppDir

# The helpers have to be the same build as the library, or they cannot talk to each other. Compared
# by build ID, not bytes: linuxdeploy rewrites a bundled library's RUNPATH, so the bytes never match.
build_id() { readelf -n "$1" | sed -n 's/.*Build ID: //p'; }
[ "$(build_id "$appdir/usr/lib/libwebkit2gtk-4.1.so.0")" = "$(build_id "$system/libwebkit2gtk-4.1.so.0")" ] || {
  echo "the bundled libwebkit2gtk-4.1 is not this system's, so this system's helpers would not match it" >&2
  exit 1
}

dest=$appdir/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1
mkdir -p "$dest/injected-bundle"
for exe in "$helpers"/WebKit*Process; do
  cp -L "$exe" "$dest/"
  patchelf --set-rpath '$ORIGIN/../..' "$dest/$(basename "$exe")"
done
for so in "$helpers"/injected-bundle/*.so; do
  cp -L "$so" "$dest/injected-bundle/"
  patchelf --set-rpath '$ORIGIN/../../..' "$dest/injected-bundle/$(basename "$so")"
done

python3 - "$appdir/usr/lib/libwebkit2gtk-4.1.so.0" <<'EOF'
import sys
path = sys.argv[1]
old, new = b"/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1", b"././/lib/x86_64-linux-gnu/webkit2gtk-4.1"
assert len(old) == len(new)
data = open(path, "rb").read()
n = data.count(old)
if n == 0:
    sys.exit("the library does not name " + old.decode() + "; WebKit changed how it finds its helpers")
open(path, "wb").write(data.replace(old, new))
print(f"rewrote {n} helper path(s) in {path.rsplit('/', 1)[1]}")
EOF

# Wayland's libraries belong to the host's graphics stack. Ubuntu's libwayland-client, loaded ahead
# of Fedora 44's, makes Mesa's EGL refuse ("Could not create default EGL display:
# EGL_BAD_PARAMETER") and WebKit's page process abort, leaving an empty window -- measured
# 2026-10-17, and the window drew once this one library was gone. Every Linux desktop has all four.
rm -f "$appdir"/usr/lib/libwayland-*.so*

rm -f "$appdir/AppRun"
cat >"$appdir/AppRun" <<'EOF'
#!/bin/sh
# WebKit finds its helpers at ././/lib/x86_64-linux-gnu/webkit2gtk-4.1, relative to here.
HERE=$(dirname "$(readlink -f "$0")")
cd "$HERE/usr" || exit 1
exec "$HERE/usr/bin/Sidekick" "$@"
EOF
chmod 755 "$appdir/AppRun"

head -c "$offset" "$app" >"$work/out.AppImage"
mksquashfs "$appdir" "$work/fs" -root-owned -noappend -quiet -comp "$comp" >/dev/null
cat "$work/fs" >>"$work/out.AppImage"
chmod 755 "$work/out.AppImage"
mv "$work/out.AppImage" "$app"
echo "carried $(ls "$dest" | grep -c Process) helper program(s) and the injected bundle into $(basename "$app")"
