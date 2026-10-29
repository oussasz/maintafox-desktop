#!/usr/bin/env bash
# Builds the OpenSSL that SQLCipher links on Android, once per ABI, outside the repo.
#
# Why not the vendored build (`bundled-sqlcipher-vendored-openssl`)? On a Windows host openssl-src
# builds inside `target/<abi>/debug/build/openssl-sys-*/out/...`; under `.claude/worktrees/...` those
# paths pass Windows' 260-character limit and source files go missing. This script builds the SAME
# OpenSSL source (the `openssl-src` crate version pinned in Cargo.lock) with the SAME options
# openssl-src uses, in a short folder, and `scripts/android/env.sh` points openssl-sys at it
# (`<TARGET>_OPENSSL_DIR`, static). The desktop build is not affected.
#
# Needs (Windows, Git Bash): the Android NDK, Git's perl plus pure-Perl modules copied from
# Strawberry Perl (Locale::Maketext::Simple, ExtUtils::MakeMaker, Pod::*; env.sh does it) —
# Strawberry's own perl writes Windows paths and OpenSSL refuses it — and GNU make
# (Strawberry's gmake.exe, exposed as `make` by env.sh).
# Usage: source scripts/android/env.sh && scripts/android/build-openssl.sh [x86_64|aarch64 ...]
set -euo pipefail

: "${NDK_HOME:?source scripts/android/env.sh first}"
: "${MFX_ANDROID_HOME:?source scripts/android/env.sh first}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
version="$(awk '/^name = "openssl-src"$/{getline; gsub(/version = |"/, ""); print; exit}' "$repo_root/src-tauri/Cargo.lock")"
[ -n "$version" ] || { echo "openssl-src not found in Cargo.lock" >&2; exit 1; }
src="$(ls -d "$HOME"/.cargo/registry/src/*/"openssl-src-$version"/openssl 2>/dev/null | head -1)"
[ -d "$src" ] || { echo "openssl-src $version not in the cargo registry: run a cargo fetch first" >&2; exit 1; }

tc="$NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin"
# libcrypto.a is archived from ~1000 objects in one `ar` call: past Windows' 32 767-character
# command-line limit the list is cut and llvm-ar fails ("crypto/ct/libcrypto-lib-c: No such file").
# This wrapper hands the arguments to llvm-ar through a response file instead.
ar_wrapper="$MFX_ANDROID_HOME/bin/llvm-ar-rsp"
cat > "$ar_wrapper" <<EOF
#!/usr/bin/env bash
rsp="\$(mktemp)"
printf '%s\n' "\$@" > "\$rsp"
"$tc/llvm-ar.exe" "@\$(cygpath -m "\$rsp")"
status=\$?
rm -f "\$rsp"
exit \$status
EOF
chmod +x "$ar_wrapper"
abis=("$@")
[ ${#abis[@]} -gt 0 ] || abis=(x86_64 aarch64)

for abi in "${abis[@]}"; do
  case "$abi" in
    x86_64) os_target=linux-x86_64; arch_flag=-m64 ;;
    aarch64) os_target=linux-aarch64; arch_flag= ;;
    *) echo "unknown ABI $abi" >&2; exit 1 ;;
  esac
  triple="$abi-linux-android"
  prefix="$MFX_ANDROID_HOME/openssl/$triple"
  if [ -f "$prefix/lib/libcrypto.a" ] && [ "$(cat "$prefix/VERSION" 2>/dev/null)" = "$version" ]; then
    echo "openssl $version for $triple already built: $prefix"
    continue
  fi
  build="$MFX_ANDROID_HOME/b-$abi"
  rm -rf "$build" "$prefix"
  cp -r "$src" "$build"
  (
    cd "$build"
    export CC="$tc/clang.exe" AR="$ar_wrapper" RANLIB="$tc/llvm-ranlib.exe"
    # Same options as openssl-src 300.x for Android targets (no-stdio etc. are required by it).
    perl ./Configure --prefix="$prefix" --openssldir=/usr/local/ssl \
      no-shared no-module no-tests no-comp no-zlib no-zlib-dynamic --libdir=lib \
      no-ssl3 no-md2 no-rc5 no-weak-ssl-ciphers no-camellia no-idea no-seed no-stdio \
      "$os_target" -O2 -DANDROID -ffunction-sections -fdata-sections -fPIC $arch_flag \
      "--target=$triple${ANDROID_API_LEVEL:-29}" -w
    # Strawberry's native gmake hands recipes to the shell through a ~8 191-character command
    # line; OpenSSL's `$(AR) $(ARFLAGS) lib.a <objects>` lines are up to 23 000 characters and get
    # cut. Split each into lines of 40 objects (`ar qc` appends, so the archive is the same).
    awk '
      /^\t\$\(AR\) \$\(ARFLAGS\) / {
        n = split($0, f, " ")
        head = "	" f[1] " " f[2] " " f[3]   # split() drops the recipe tab
        line = head; count = 0
        for (i = 4; i <= n; i++) {
          line = line " " f[i]
          if (++count == 40) { print line; line = head; count = 0 }
        }
        if (count > 0) print line
        next
      }
      { print }
    ' makefile > makefile.split && mv makefile.split makefile
    make depend
    make -j"${CARGO_BUILD_JOBS:-4}" build_libs
    make install_dev
  )
  echo "$version" > "$prefix/VERSION"
  rm -rf "$build"
  echo "built openssl $version for $triple: $prefix"
done
