#!/usr/bin/env bash
# Start an RC AppImage on a clean Linux and fail unless its window opens.
#
#   scripts/keys/rc-appimage-launch.sh <ubuntu|fedora> path/to/Sidekick.AppImage
#
# Why this exists (linux-appimage-1). rc1's AppImage built green and exited the moment it ran on
# Fedora: it carried Ubuntu's libwebkit2gtk but none of WebKit's helper programs, and the library
# looks for them in Ubuntu's folder. Nothing in rc-bundle.yml ever started the app, so the first
# person to find out was the first person to download it.
#
# Each run is a fresh container (podman, or docker when podman is absent) holding what a desktop
# has and no WebKit: an X server on a virtual screen, Mesa with GLES, fonts and GTK 3. The AppImage
# leans on the host for pieces of those (libfribidi and libGLESv2 were the first two runs' errors),
# and any desktop has them. It brings the rest or fails. Pass is all three of: the process is alive
# after WAIT seconds, a window named "CoH Sidekick" exists, and the log has no WebKit spawn or IPC
# error.
set -euo pipefail

WAIT=${WAIT:-20}
IMAGES_ubuntu=docker.io/library/ubuntu:24.04
IMAGES_fedora=registry.fedoraproject.org/fedora:44

if [ "${1:-}" = "--inside" ]; then
  distro=$2 app=$3
  case "$distro" in
    ubuntu)
      export DEBIAN_FRONTEND=noninteractive
      apt-get update -qq >/dev/null
      apt-get install -y -qq --no-install-recommends xvfb xdotool libgl1 libegl1 libgl1-mesa-dri \
        fontconfig fonts-dejavu-core libgtk-3-0t64 libgles2 >/dev/null ;;
    fedora)
      dnf install -y -q xorg-x11-server-Xvfb xdotool mesa-libGL mesa-libEGL mesa-dri-drivers \
        fontconfig dejavu-sans-fonts gtk3 libglvnd-gles >/dev/null 2>&1 ;;
  esac
  if ls /usr/lib*/libwebkit2gtk* /usr/lib/*/libwebkit2gtk* >/dev/null 2>&1; then
    echo "SETUP  WebKit got installed, so this would not be a clean host"; exit 2
  fi
  Xvfb :99 -screen 0 1600x1000x24 >/dev/null 2>&1 &
  export DISPLAY=:99
  sleep 2
  # No FUSE in a container; the runtime unpacks itself instead.
  export APPIMAGE_EXTRACT_AND_RUN=1
  home=$(mktemp -d)
  cp "$app" "$home/app.AppImage" && chmod +x "$home/app.AppImage"
  cd "$home"
  HOME=$home "$home/app.AppImage" >"$home/log" 2>&1 &
  pid=$!
  sleep "$WAIT"

  fail=0
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "FAIL  the app exited within ${WAIT}s"; fail=1
  fi
  if ! xdotool search --name "CoH Sidekick" >/dev/null 2>&1; then
    echo "FAIL  no window named \"CoH Sidekick\""; fail=1
  fi
  if grep -E "Failed to spawn|Received invalid message|WebKitNetworkProcess|WebKitWebProcess|injected-bundle" "$home/log" >/dev/null; then
    echo "FAIL  WebKit could not start its helpers"; fail=1
  fi
  echo "---- app log (last 20 lines)"
  tail -n 20 "$home/log"
  [ "$fail" = 0 ] && echo "PASS  window open after ${WAIT}s, no WebKit errors"
  exit "$fail"
fi

distro=${1:?ubuntu or fedora} app=${2:?path to the AppImage}
image_var=IMAGES_$distro
image=${!image_var:?unknown distro $distro}
[ -f "$app" ] || { echo "no file at $app" >&2; exit 2; }
runtime=$(command -v podman || command -v docker) || { echo "need podman or docker" >&2; exit 2; }
here=$(cd "$(dirname "$0")" && pwd)
echo "== $distro ($image): $(basename "$app")"
exec "$runtime" run --rm \
  -v "$(cd "$(dirname "$app")" && pwd)/$(basename "$app"):/app.AppImage:ro,Z" \
  -v "$here/rc-appimage-launch.sh:/launch.sh:ro,Z" \
  -e WAIT="$WAIT" "$image" bash /launch.sh --inside "$distro" /app.AppImage
