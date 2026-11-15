#!/usr/bin/env bash
# Builds the Android field app APK on a Windows PC without Developer Mode.
#   source scripts/android/env.sh && scripts/android/build-apk.sh <x86_64|aarch64> [debug|release]
#
# `tauri android build` compiles the engine (with the Android settings the Tauri CLI passes: JNI
# package names, tauri.android.conf.json, the mobile page) and then links the library into the
# Gradle project with a symbolic link. Windows refuses symbolic links unless Developer Mode is on
# (a machine setting this script never changes). So: run the CLI, accept ONLY that failure, copy
# the library where the link would point, and run Gradle without its Rust step.
# Extra arguments for the Tauri CLI can be given in TAURI_ANDROID_ARGS (e.g. --ignore-version-mismatches).
set -euo pipefail

: "${MFX_ANDROID_HOME:?source scripts/android/env.sh first}"
abi="${1:?ABI: x86_64 or aarch64}"
profile="${2:-debug}"
case "$abi" in
  x86_64) jni=x86_64; flavor=X86_64; flavor_dir=x86_64 ;;
  aarch64) jni=arm64-v8a; flavor=Arm64; flavor_dir=arm64 ;;
  *) echo "unknown ABI $abi" >&2; exit 1 ;;
esac
case "$profile" in
  debug) profile_flag=--debug; Profile=Debug ;;
  release) profile_flag=; Profile=Release ;;
  *) echo "unknown profile $profile" >&2; exit 1 ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# The licence server is compiled into the app (src-tauri/build.rs). Without VITE_ADMIN_API_BASE_URL
# the build talks to the real Maintafox server (shared/control-plane.json); with it, to a test
# server, and every screen of the app shows a red "test server" marker. Set the server with this
# environment variable only (not a .env file): the app's marker reads the same variable.
if [ -n "${VITE_ADMIN_API_BASE_URL:-}" ]; then
  echo "NOTE: TEST build, licence server = $VITE_ADMIN_API_BASE_URL (marked in red in the app)" >&2
else
  echo "Licence server: the real Maintafox server (shared/control-plane.json)"
fi
log="$MFX_ANDROID_HOME/tauri-$abi-$profile.log"
cd "$repo_root"
set +e
# beforeBuildCommand without `tsc --noEmit`: typechecking is CI's job; the bundle is the same.
pnpm tauri android build $profile_flag --target "$abi" --apk --ci ${TAURI_ANDROID_ARGS:-} \
  -c '{"build":{"beforeBuildCommand":"pnpm exec vite build"}}' > "$log" 2>&1
status=$?
set -e
if [ $status -ne 0 ] && ! grep -q "Failed to create a symbolic link" "$log"; then
  tail -30 "$log" >&2
  echo "tauri android build failed (log: $log)" >&2
  exit $status
fi

so="src-tauri/target/$abi-linux-android/$profile/libmaintafox_lib.so"
[ -f "$so" ] || { echo "engine library not built: $so" >&2; exit 1; }
mkdir -p "src-tauri/gen/android/app/src/main/jniLibs/$jni"
cp "$so" "src-tauri/gen/android/app/src/main/jniLibs/$jni/"

cd src-tauri/gen/android
./gradlew.bat "assemble$flavor$Profile" -x "rustBuild$flavor$Profile" --console=plain > "$MFX_ANDROID_HOME/gradle-$abi-$profile.log" 2>&1 \
  || { tail -30 "$MFX_ANDROID_HOME/gradle-$abi-$profile.log" >&2; exit 1; }
find "app/build/outputs/apk/$flavor_dir/$profile" -name '*.apk' -exec ls -l {} \;
