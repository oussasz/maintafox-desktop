# Android build environment for THIS shell only (Git Bash on Windows). Source it:
#   source scripts/android/env.sh
# Nothing global is changed (no system env vars, no CARGO_TARGET_DIR). Every Rust/OpenSSL variable
# is prefixed by the Android target triple, so a desktop build in the same shell is unaffected.
# Paths can be overridden before sourcing (ANDROID_HOME, NDK_HOME, JAVA_HOME, MFX_ANDROID_HOME).

_mfx_u() { cygpath -u "$1"; }

export ANDROID_HOME="${ANDROID_HOME:-$(_mfx_u "$LOCALAPPDATA")/Android/Sdk}"
if [ -z "${NDK_HOME:-}" ]; then
  NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
fi
export NDK_HOME ANDROID_NDK_HOME="$NDK_HOME"
export JAVA_HOME="${JAVA_HOME:-/c/Program Files/Microsoft/jdk-21.0.12.8-hotspot}"
# Short folder outside the repo: the OpenSSL build (scripts/android/build-openssl.sh), the Perl
# modules it needs and the `make` shim.
export MFX_ANDROID_HOME="${MFX_ANDROID_HOME:-$(_mfx_u "$LOCALAPPDATA")/mfx-android}"
export ANDROID_API_LEVEL="${ANDROID_API_LEVEL:-29}"   # Android 10 (BYOD minimum, advisor 2026-10-23)

mkdir -p "$MFX_ANDROID_HOME/bin" "$MFX_ANDROID_HOME/perl5lib"
# GNU make: Strawberry Perl ships gmake.exe; OpenSSL's build calls `make`.
if [ ! -x "$MFX_ANDROID_HOME/bin/make.exe" ] && [ -x /c/Strawberry/c/bin/gmake.exe ]; then
  cp /c/Strawberry/c/bin/gmake.exe "$MFX_ANDROID_HOME/bin/make.exe"
fi
# Git's perl lacks a few pure-Perl modules OpenSSL's Configure needs; copy them from Strawberry.
_p5="$MFX_ANDROID_HOME/perl5lib"
if [ ! -f "$_p5/.complete" ] && [ -d /c/Strawberry/perl/lib ]; then
  _sl=/c/Strawberry/perl/lib
  mkdir -p "$_p5/Locale/Maketext" "$_p5/ExtUtils"
  cp "$_sl/Locale/Maketext/Simple.pm" "$_p5/Locale/Maketext/"
  cp -r "$_sl/ExtUtils/MakeMaker.pm" "$_sl/ExtUtils/MakeMaker" "$_sl"/ExtUtils/MM*.pm "$_sl"/ExtUtils/MY.pm \
    "$_sl"/ExtUtils/Liblist* "$_sl/ExtUtils/Manifest.pm" "$_sl/ExtUtils/Mkbootstrap.pm" "$_sl/ExtUtils/Mksymlists.pm" \
    "$_p5/ExtUtils/"
  cp -r "$_sl/Pod" "$_p5/"
  touch "$_p5/.complete"
fi
export PERL5LIB="$_p5"
# Keep PERL5LIB as a POSIX path when cargo (a Windows program) passes it on to Git's perl.
export MSYS2_ENV_CONV_EXCL="PERL5LIB${MSYS2_ENV_CONV_EXCL:+;$MSYS2_ENV_CONV_EXCL}"
export PATH="$MFX_ANDROID_HOME/bin:$JAVA_HOME/bin:$PATH"

_tc="$NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin"
for _abi in x86_64 aarch64; do
  _t="${_abi}_linux_android"
  _T="$(echo "$_t" | tr a-z A-Z)"
  export "CC_${_t}=$_tc/clang.exe"
  export "CFLAGS_${_t}=--target=${_abi}-linux-android${ANDROID_API_LEVEL}"
  export "AR_${_t}=$_tc/llvm-ar.exe"
  export "RANLIB_${_t}=$_tc/llvm-ranlib.exe"
  export "CARGO_TARGET_${_T}_LINKER=$_tc/${_abi}-linux-android${ANDROID_API_LEVEL}-clang.cmd"
  # SQLCipher's OpenSSL: the one built by build-openssl.sh, linked statically (no vendored build).
  export "${_T}_OPENSSL_NO_VENDOR=1"
  export "${_T}_OPENSSL_STATIC=1"
  export "${_T}_OPENSSL_DIR=$(cygpath -m "$MFX_ANDROID_HOME/openssl/${_abi}-linux-android")"
done
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
unset _abi _t _T _tc _p5 _sl
