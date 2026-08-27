#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <SimShredder.dmg> [evidence.json]" >&2
  exit 64
fi
if [[ "$(uname -s)" != 'Darwin' ]]; then
  echo 'the macOS clean-user verifier must run on macOS' >&2
  exit 69
fi
if [[ "$(id -u)" == '0' ]]; then
  echo 'refusing to verify from the root account' >&2
  exit 77
fi
if id -Gn | tr ' ' '\n' | grep -qx 'admin'; then
  echo 'the current account belongs to the macOS admin group; log in to a clean standard account' >&2
  exit 77
fi
if [[ "$(uname -m)" != 'arm64' ]]; then
  echo 'the macOS clean-user verifier requires Apple Silicon' >&2
  exit 69
fi
if [[ -z "${HOME:-}" || "$HOME" != /* || "$HOME" == '/' ]]; then
  echo 'the macOS clean-user verifier requires a safe absolute user home' >&2
  exit 77
fi
os_version=$(sw_vers -productVersion)
if (( ${os_version%%.*} < 26 )); then
  echo "the macOS clean-user verifier requires macOS 26 or newer (found $os_version)" >&2
  exit 69
fi

dmg=$1
evidence=${2:-}
if [[ ! -f "$dmg" ]]; then
  echo "DMG is not a file: $dmg" >&2
  exit 66
fi
dmg=$(cd "$(dirname "$dmg")" && pwd -P)/$(basename "$dmg")

control_root="$HOME/Library/Application Support/SimShredder"
legacy_root="$HOME/Library/Application Support/dev.simshredder.desktop"
exports_root="$HOME/Documents/SimShredder Exports"
for path in "$control_root" "$legacy_root" "$exports_root"; do
  if [[ -e "$path" || -L "$path" ]]; then
    echo "clean-user verification requires absent application data: $path" >&2
    exit 78
  fi
done
if [[ -n "$evidence" ]]; then
  if [[ "$evidence" != /* ]]; then
    echo 'the evidence path must be absolute' >&2
    exit 64
  fi
  case "$evidence" in
    "$control_root"|"$control_root"/*|"$legacy_root"|"$legacy_root"/*|"$exports_root"|"$exports_root"/*)
      echo 'the evidence path cannot be inside application data that the verifier removes' >&2
      exit 64
      ;;
  esac
fi

applications="$HOME/Applications"
mkdir -p "$applications"
install_root=$(mktemp -d "$applications/.simshredder-clean-user.XXXXXX")
scratch=$(mktemp -d "${TMPDIR:-/tmp}/simshredder-clean-user.XXXXXX")
mountpoint="$scratch/mount"
app="$install_root/SimShredder.app"
app_pid=''
mounted=0

cleanup() {
  if [[ -n "$app_pid" ]]; then
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  if [[ "$mounted" == '1' ]]; then hdiutil detach "$mountpoint" >/dev/null 2>&1 || true; fi
  rm -rf "$install_root" "$scratch"
  rm -rf "$control_root" "$legacy_root" "$exports_root"
}
trap cleanup EXIT

hdiutil verify "$dmg" >/dev/null
mkdir "$mountpoint"
hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$mountpoint" "$dmg" >/dev/null
mounted=1
if [[ ! -d "$mountpoint/SimShredder.app" ]]; then echo 'SimShredder.app is missing from the DMG' >&2; exit 65; fi
ditto "$mountpoint/SimShredder.app" "$app"
hdiutil detach "$mountpoint" >/dev/null
mounted=0

binary="$app/Contents/MacOS/simshredder-desktop"
if [[ ! -x "$binary" ]]; then echo 'installed application executable is missing' >&2; exit 65; fi
if [[ "$(stat -f '%u' "$app")" != "$(id -u)" ]]; then echo 'per-user application copy is not owned by the standard account' >&2; exit 77; fi
if [[ "$(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$app/Contents/Info.plist")" != '26.0' ]]; then
  echo 'installed application has an unexpected macOS deployment target' >&2
  exit 65
fi
for license in LICENSE NOTICE PRIVACY.md THIRD_PARTY_NOTICES.md rust-third-party-licenses.md node-third-party-licenses.md; do
  if [[ ! -f "$app/Contents/Resources/licenses/$license" ]]; then echo "installed license resource is missing: $license" >&2; exit 65; fi
done

"$binary" >/dev/null 2>"$scratch/app.stderr" &
app_pid=$!
sleep 5
if ! kill -0 "$app_pid" 2>/dev/null; then
  echo 'clean-user GUI exited during launch' >&2
  sed -n '1,80p' "$scratch/app.stderr" >&2
  exit 70
fi
if [[ ! -d "$control_root" || "$(stat -f '%u' "$control_root")" != "$(id -u)" ]]; then
  echo 'clean-user GUI did not create its expected user-owned application data root' >&2
  exit 70
fi
if [[ -e "$legacy_root" || -L "$legacy_root" ]]; then
  echo 'clean-user GUI unexpectedly created the legacy bundle-identifier data root' >&2
  exit 70
fi
kill "$app_pid"
wait "$app_pid" 2>/dev/null || true
app_pid=''

dmg_hash=$(shasum -a 256 "$dmg" | awk '{print $1}')
json=$(printf '{"schema":1,"platform":"macos-aarch64","standard_account":true,"admin_member":false,"os_version":"%s","dmg_sha256":"%s","install_root":"~/Applications","data_root":"~/Library/Application Support/SimShredder","launch_seconds":5}\n' "$os_version" "$dmg_hash")
if [[ -n "$evidence" ]]; then
  mkdir -p "$(dirname "$evidence")"
  printf '%s' "$json" >"$evidence"
  chmod 600 "$evidence"
fi
printf '%s' "$json"
