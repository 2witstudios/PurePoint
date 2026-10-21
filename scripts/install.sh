#!/bin/sh
# Install the PurePoint CLI (pu) and daemon (pu-engine) into ~/.pu/bin.
#
#   curl -fsSL https://raw.githubusercontent.com/2witstudios/PurePoint/main/scripts/install.sh | sh
#
# Environment:
#   PU_VERSION      release to install, e.g. 1.4.0 (default: latest)
#   PU_INSTALL_DIR  install directory (default: ~/.pu/bin)
#   PU_ARCHIVE      install from a local purepoint-*.tar.gz instead of downloading
#                   (not verified: you vouch for the file)
#   PU_INSTALL_SKIP_VERIFY=1
#                   install a download even if its authenticity can't be
#                   verified (prints a warning)
#   PU_WITH_PHONE=1 also install phone access (the Point Guard bridge payload)
#                   into ~/.pu/pointguard/<version>/; needs glibc >= 2.28
#   PU_PHONE_ARCHIVE
#                   install phone access from a local
#                   purepoint-pointguard-*-linux-gnu.tar.gz (implies
#                   PU_WITH_PHONE=1; not verified: you vouch for the file)
#
# Downloads are verified before anything is installed. The release key below
# must have signed the archive (checked with minisign, or with OpenSSL 3 when
# minisign isn't installed), and when the GitHub CLI is installed and signed
# in, the archive's build provenance must verify with `gh attestation verify`.
# At least one of the two must succeed, and any check that runs must pass.
#
# Linux builds of pu and pu-engine are static (musl) and run on any
# distribution. Phone access bundles the official glibc Node.js build, so it
# needs glibc >= 2.28 (not Alpine/musl). On macOS, prefer the PurePoint app,
# which bundles everything.
set -eu

REPO="2witstudios/PurePoint"
# Minisign public key for release archives (.github/scripts/minisign.sh).
RELEASE_PUBLIC_KEY="RWSEvitv5QIgchUQgebD93PuXamKznz5sMJMavmViuMEYSM8nVtcrRv6"
INSTALL_DIR="${PU_INSTALL_DIR:-$HOME/.pu/bin}"

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }
warn() { printf 'install.sh: warning: %s\n' "$*" >&2; }

detect_arch() {
    arch=$(uname -m)
    case "$arch" in
        x86_64 | amd64) printf 'x86_64' ;;
        aarch64 | arm64) printf 'aarch64' ;;
        *) die "unsupported architecture: $arch" ;;
    esac
}

detect_target() {
    os=$(uname -s)
    arch=$(detect_arch)
    case "$os" in
        Linux) printf '%s-unknown-linux-musl' "$arch" ;;
        *) die "no prebuilt $os binaries — on macOS install the PurePoint app, elsewhere build with cargo (see README)" ;;
    esac
}

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --proto '=https' "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2"
    else
        die "need curl or wget"
    fi
}

latest_version() {
    url="https://api.github.com/repos/$REPO/releases/latest"
    fetch "$url" "$tmp/latest.json" || die "could not query $url"
    sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -n 1
}

release_version() {
    if [ -z "${version:-}" ]; then
        version="${PU_VERSION:-}"
        [ -n "$version" ] || version=$(latest_version)
        [ -n "$version" ] || die "could not determine the latest release"
        version="${version#v}"
    fi
    printf '%s' "$version"
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        die "need sha256sum or shasum to verify the download"
    fi
}

b64decode() {
    openssl base64 -d -A
}

# bytes <file> <offset> <count>
bytes() {
    dd if="$1" bs=1 skip="$2" count="$3" 2>/dev/null
}

hex() {
    od -An -v -tx1 | tr -d ' \n'
}

# Ed25519 verification with OpenSSL 3: ed25519_verify <raw-key> <message> <sig>
ed25519_verify() {
    # DER SubjectPublicKeyInfo prefix for an Ed25519 key.
    { printf '\060\052\060\005\006\003\053\145\160\003\041\000'; cat "$1"; } > "$1.der"
    openssl pkeyutl -verify -pubin -keyform DER -inkey "$1.der" -rawin \
        -in "$2" -sigfile "$3" >/dev/null 2>&1
}

# Checks a minisign signature (prehashed "ED" format, the minisign default)
# against RELEASE_PUBLIC_KEY with OpenSSL, including the trusted comment.
openssl_minisign_verify() {
    file=$1 sig=$2 work="$tmp/verify"
    mkdir -p "$work"
    printf '%s' "$RELEASE_PUBLIC_KEY" | b64decode > "$work/pub" 2>/dev/null || return 1
    [ "$(wc -c < "$work/pub" | tr -d ' ')" = 42 ] || return 1
    [ "$(bytes "$work/pub" 0 2)" = Ed ] || return 1
    bytes "$work/pub" 10 32 > "$work/key"

    [ "$(sed -n 1p "$sig" | cut -c 1-19)" = "untrusted comment: " ] || return 1
    sed -n 2p "$sig" | b64decode > "$work/sig-line" 2>/dev/null || return 1
    [ "$(wc -c < "$work/sig-line" | tr -d ' ')" = 74 ] || return 1
    [ "$(bytes "$work/sig-line" 0 2)" = ED ] || return 1
    [ "$(bytes "$work/sig-line" 2 8 | hex)" = "$(bytes "$work/pub" 2 8 | hex)" ] || return 1
    bytes "$work/sig-line" 10 64 > "$work/sig"

    trusted=$(sed -n 3p "$sig")
    case "$trusted" in "trusted comment: "*) ;; *) return 1 ;; esac
    sed -n 4p "$sig" | b64decode > "$work/global" 2>/dev/null || return 1
    [ "$(wc -c < "$work/global" | tr -d ' ')" = 64 ] || return 1

    openssl dgst -blake2b512 -binary "$file" > "$work/hash" || return 1
    ed25519_verify "$work/key" "$work/hash" "$work/sig" || return 1
    { cat "$work/sig"; printf '%s' "${trusted#trusted comment: }"; } > "$work/global-input"
    ed25519_verify "$work/key" "$work/global-input" "$work/global"
}

openssl_can_verify() {
    command -v openssl >/dev/null 2>&1 || return 1
    case "$(openssl version 2>/dev/null)" in
        "OpenSSL 0."* | "OpenSSL 1."* | LibreSSL*) return 1 ;;
        OpenSSL*) return 0 ;;
        *) return 1 ;;
    esac
}

# Prints "verified", "unavailable" (nothing to check with) or "failed".
verify_signature() {
    file=$1 sig_url=$2
    if ! fetch "$sig_url" "$tmp/archive.minisig" 2>/dev/null; then
        warn "the release has no signature ($sig_url)"
        echo unavailable
        return
    fi
    if command -v minisign >/dev/null 2>&1; then
        if minisign -Vm "$file" -x "$tmp/archive.minisig" -P "$RELEASE_PUBLIC_KEY" >/dev/null 2>&1; then
            echo verified
        else
            echo failed
        fi
    elif openssl_can_verify; then
        if openssl_minisign_verify "$file" "$tmp/archive.minisig"; then
            echo verified
        else
            echo failed
        fi
    else
        warn "install minisign or OpenSSL 3 to check the release signature"
        echo unavailable
    fi
}

# Prints "verified", "unavailable" or "failed".
verify_attestation() {
    file=$1
    if ! command -v gh >/dev/null 2>&1 || ! gh auth status >/dev/null 2>&1; then
        echo unavailable
        return
    fi
    if gh attestation verify "$file" --repo "$REPO" >/dev/null 2>&1; then
        echo verified
    else
        echo failed
    fi
}

verify_download() {
    file=$1 sig_url=$2
    signature=$(verify_signature "$file" "$sig_url")
    [ "$signature" != failed ] || die "signature check failed for $(basename "$file"); not installing"
    attestation=$(verify_attestation "$file")
    [ "$attestation" != failed ] || die "gh attestation verify failed for $(basename "$file"); not installing"
    if [ "$signature" = verified ] || [ "$attestation" = verified ]; then
        [ "$signature" = verified ] && say "verified release signature"
        [ "$attestation" = verified ] && say "verified build provenance (gh attestation)"
        return 0
    fi
    if [ "${PU_INSTALL_SKIP_VERIFY:-}" = 1 ]; then
        warn "PU_INSTALL_SKIP_VERIFY=1: installing $(basename "$file") without verifying who built it"
        return 0
    fi
    die "could not verify $(basename "$file"): install minisign or OpenSSL 3, or sign in with the GitHub CLI (gh auth login). To install anyway, set PU_INSTALL_SKIP_VERIFY=1."
}

tmp=$(mktemp -d)
phone_stage=""
cleanup() {
    rm -rf "$tmp"
    if [ -n "$phone_stage" ]; then rm -rf "$phone_stage"; fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

if [ -n "${PU_ARCHIVE:-}" ]; then
    archive="$PU_ARCHIVE"
    [ -f "$archive" ] || die "PU_ARCHIVE not found: $archive"
else
    target=$(detect_target)
    version=$(release_version)
    name="purepoint-$version-$target"
    base="https://github.com/$REPO/releases/download/v$version"
    archive="$tmp/$name.tar.gz"
    say "downloading $name"
    fetch "$base/$name.tar.gz" "$archive" || die "download failed: $base/$name.tar.gz"
    fetch "$base/$name.tar.gz.sha256" "$tmp/sum" || die "checksum download failed"
    expected=$(cut -d ' ' -f 1 "$tmp/sum")
    actual=$(sha256_of "$archive")
    [ "$expected" = "$actual" ] || die "checksum mismatch for $name.tar.gz"
    verify_download "$archive" "$base/$name.tar.gz.minisig"
fi

mkdir -p "$tmp/x"
tar -xzf "$archive" -C "$tmp/x"
pu=$(find "$tmp/x" -type f -name pu | head -n 1)
engine=$(find "$tmp/x" -type f -name pu-engine | head -n 1)
[ -n "$pu" ] && [ -n "$engine" ] || die "archive does not contain pu and pu-engine"

mkdir -p "$INSTALL_DIR"
# Replace by rename so a running daemon keeps its (unlinked) binary intact.
# The temp name includes $$ so concurrent installs cannot interleave cp/mv
# and publish a half-written binary.
for bin in "$pu" "$engine"; do
    dest="$INSTALL_DIR/$(basename "$bin")"
    staging="$dest.new.$$"
    cp "$bin" "$staging"
    chmod 755 "$staging"
    mv -f "$staging" "$dest"
done
say "installed pu and pu-engine to $INSTALL_DIR"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        say ""
        say "add it to your PATH, e.g.:"
        say "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.profile"
        ;;
esac

say ""
say "next steps on this server:"
say "  $INSTALL_DIR/pu daemon install --now   # run the daemon as a user service"
say "  cd <repo> && $INSTALL_DIR/pu init        # register each project"
say "If the daemon is already running, restart it to pick up the new version (pu daemon stop && pu daemon start)."

[ "${PU_WITH_PHONE:-}" = 1 ] || [ -n "${PU_PHONE_ARCHIVE:-}" ] || exit 0

# ---------------------------------------------------------------------------
# Phone access (opt-in). pu and pu-engine above stay installed whatever
# happens here; a failure exits 3 and says so.

phone_die() {
    printf 'install.sh: phone access not installed: %s\n' "$*" >&2
    printf 'install.sh: pu and pu-engine were installed and keep working.\n' >&2
    exit 3
}

# True when $1 is a directory or file that group or other can write.
others_can_write() {
    [ -n "$(find "$1" -prune \( -perm -0020 -o -perm -0002 \) -print)" ]
}

owned_by_me() {
    [ -n "$(find "$1" -prune -user "$(id -u)" -print)" ]
}

# A directory pu trusts: a real directory owned by this user that nobody else
# can write. A group/other-writable one of ours is tightened, never loosened.
require_private_dir() {
    [ ! -L "$1" ] && [ -d "$1" ] || phone_die "$1 is not a plain directory; refusing to install through it"
    owned_by_me "$1" || phone_die "$1 is not owned by you; refusing to install through it"
    if others_can_write "$1"; then
        chmod go-w "$1" || phone_die "cannot remove group/other write access from $1"
        say "removed group/other write access from $1"
    fi
}

valid_payload_name() {
    printf '%s\n' "$1" | grep -Eqx "purepoint-pointguard-[0-9]+\.[0-9]+\.[0-9]+-$2-linux-gnu"
}

# glibc >= 2.28, the floor of the official Node.js Linux builds (LPH-001).
require_glibc() {
    [ "$(uname -s)" = Linux ] || phone_die "phone access on $(uname -s) comes with the PurePoint app"
    glibc=""
    if command -v getconf >/dev/null 2>&1; then
        glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null | sed -n 's/^glibc \([0-9][0-9]*\.[0-9][0-9]*\).*/\1/p')
    fi
    if [ -z "$glibc" ] && command -v ldd >/dev/null 2>&1; then
        if ldd --version 2>&1 | grep -qi musl; then
            glibc=""
        else
            glibc=$(ldd --version 2>/dev/null | head -n 1 | sed -n 's/.* \([0-9][0-9]*\.[0-9][0-9]*\)$/\1/p')
        fi
    fi
    if [ -z "$glibc" ]; then
        phone_die "glibc ≥ 2.28 required for phone access (this host has no glibc, e.g. Alpine/musl)"
    fi
    major=${glibc%%.*}
    minor=${glibc#*.}
    if [ "$major" -lt 2 ] || { [ "$major" -eq 2 ] && [ "$minor" -lt 28 ]; }; then
        phone_die "glibc ≥ 2.28 required for phone access (this host has glibc $glibc)"
    fi
}

# Every entry must sit under "$2/" with a plain name: no absolute paths, no
# "..", and only regular files and directories (no symlinks, hard links or
# devices), so nothing can point or land outside the staging directory.
check_listing() {
    LC_ALL=C tar -tzf "$1" > "$tmp/phone.list" || phone_die "cannot read $1"
    [ -s "$tmp/phone.list" ] || phone_die "empty archive"
    # $2 was checked against a strict pattern, so it is safe inside a regex.
    if LC_ALL=C grep -v -x -e "$2/\{0,1\}" -e "$2/[A-Za-z0-9._@+=/-]*" "$tmp/phone.list" | grep -q .; then
        phone_die "archive has entries outside $2/ or with unsafe names"
    fi
    if LC_ALL=C grep -E -q -e '^/' -e '//' -e '(^|/)\.\.?(/|$)' "$tmp/phone.list"; then
        phone_die "archive has absolute or relative-escape paths"
    fi
    LC_ALL=C tar -tvzf "$1" > "$tmp/phone.long" || phone_die "cannot read $1"
    if cut -c 1 "$tmp/phone.long" | grep -q -v -e '^-$' -e '^d$'; then
        phone_die "archive contains links or special files; only regular files and directories are allowed"
    fi
}

manifest_field() {
    sed -n "s/^ *\"$1\": *\"\{0,1\}\([^\",]*\)\"\{0,1\},\{0,1\}\$/\1/p" "$2" | head -n 1
}

# A version directory is complete when its manifest and every file pu runs exist.
payload_complete() {
    for file in runtime-manifest.json bin/node bin/point-guard-lock bridge/main.js; do
        [ -f "$1/$file" ] && [ ! -L "$1/$file" ] || return 1
    done
    [ "$(manifest_field puVersion "$1/runtime-manifest.json")" = "$2" ]
}

require_glibc
arch=$(detect_arch)

if [ -n "${PU_PHONE_ARCHIVE:-}" ]; then
    phone_archive="$PU_PHONE_ARCHIVE"
    [ -f "$phone_archive" ] || phone_die "PU_PHONE_ARCHIVE not found: $phone_archive"
    phone_name=$(LC_ALL=C tar -tzf "$phone_archive" | head -n 1 | cut -d / -f 1)
    valid_payload_name "$phone_name" "$arch" || phone_die "PU_PHONE_ARCHIVE is not a $arch phone access payload: $phone_archive"
    if [ -f "$phone_archive.sha256" ]; then
        expected=$(cut -d ' ' -f 1 "$phone_archive.sha256")
        [ "$expected" = "$(sha256_of "$phone_archive")" ] || phone_die "checksum mismatch for $phone_archive"
        say "verified SHA-256 of $phone_archive"
    else
        say "note: no $phone_archive.sha256 beside the local archive; checksum not verified"
    fi
    say "note: provenance is not checked for a local archive; verify it with:"
    say "  gh attestation verify $phone_archive --repo $REPO"
else
    version=$(release_version)
    phone_name="purepoint-pointguard-$version-$arch-linux-gnu"
    valid_payload_name "$phone_name" "$arch" || phone_die "release version must be X.Y.Z, got $version"
    base="https://github.com/$REPO/releases/download/v$version"
    phone_archive="$tmp/$phone_name.tar.gz"
    say ""
    say "downloading $phone_name"
    fetch "$base/$phone_name.tar.gz" "$phone_archive" || phone_die "download failed: $base/$phone_name.tar.gz"
    fetch "$base/$phone_name.tar.gz.sha256" "$tmp/phone.sum" || phone_die "checksum download failed"
    expected=$(cut -d ' ' -f 1 "$tmp/phone.sum")
    printf '%s' "$expected" | grep -Eq '^[0-9a-f]{64}$' || phone_die "malformed checksum file"
    [ "$expected" = "$(sha256_of "$phone_archive")" ] || phone_die "checksum mismatch for $phone_name.tar.gz"
    # Same rule as pu itself: the release signature or build provenance must
    # verify, and any check that runs must pass.
    signature=$(verify_signature "$phone_archive" "$base/$phone_name.tar.gz.minisig")
    [ "$signature" != failed ] || phone_die "signature check failed for $phone_name.tar.gz"
    attestation=$(verify_attestation "$phone_archive")
    [ "$attestation" != failed ] || phone_die "gh attestation verify failed for $phone_name.tar.gz"
    [ "$signature" = verified ] && say "verified release signature of $phone_name.tar.gz"
    [ "$attestation" = verified ] && say "verified build provenance of $phone_name.tar.gz (gh attestation)"
    if [ "$signature" != verified ] && [ "$attestation" != verified ]; then
        if [ "${PU_INSTALL_SKIP_VERIFY:-}" = 1 ]; then
            warn "PU_INSTALL_SKIP_VERIFY=1: installing $phone_name.tar.gz without verifying who built it"
        else
            phone_die "could not verify $phone_name.tar.gz: install minisign or OpenSSL 3, or sign in with the GitHub CLI (gh auth login). To install anyway, set PU_INSTALL_SKIP_VERIFY=1."
        fi
    fi
fi

check_listing "$phone_archive" "$phone_name"

# Stage inside ~/.pu/pointguard so the final step is one rename on one filesystem.
pu_home="$HOME/.pu"
root="$pu_home/pointguard"
mkdir -p "$pu_home"
require_private_dir "$pu_home"
if [ ! -e "$root" ] && [ ! -L "$root" ]; then
    mkdir -m 755 "$root" || phone_die "cannot create $root"
fi
require_private_dir "$root"

phone_stage=$(mktemp -d "$root/.install.XXXXXX") || phone_die "cannot create a staging directory in $root"
(umask 022 && tar -xzf "$phone_archive" -C "$phone_stage") || phone_die "unpacking failed"
payload="$phone_stage/$phone_name"
[ -d "$payload" ] && [ ! -L "$payload" ] || phone_die "archive has no $phone_name/ directory"
# Re-check what landed on disk, then make sure nothing is group/other-writable.
[ -z "$(find "$payload" ! -type f ! -type d -print | head -n 1)" ] || phone_die "archive unpacked links or special files"
chmod -R go-w "$payload"
[ -z "$(find "$payload" \( -perm -0020 -o -perm -0002 \) -print | head -n 1)" ] || phone_die "cannot make the payload private"

manifest="$payload/runtime-manifest.json"
[ -f "$manifest" ] || phone_die "archive has no runtime-manifest.json"
[ "$(manifest_field layout "$manifest")" = linux ] || phone_die "not a Linux phone access payload"
[ "$(manifest_field schemaVersion "$manifest")" = 1 ] || phone_die "unsupported payload schema"
pu_version=$(manifest_field puVersion "$manifest")
printf '%s' "$pu_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || phone_die "payload names no valid pu version"
payload_complete "$payload" "$pu_version" || phone_die "payload is incomplete"
node_version=$(manifest_field nodeVersion "$manifest")
# The bundled Node must run on this host before anything points at it.
ran=$(env -i PATH=/usr/bin:/bin "$payload/bin/node" --version 2>/dev/null) || ran=""
[ "$ran" = "v$node_version" ] || phone_die "the bundled Node.js does not run on this host (glibc ≥ 2.28 required for phone access)"

dest="$root/$pu_version"
if [ -e "$dest" ] || [ -L "$dest" ]; then
    # Never modify an existing version: a running bridge may be executing it.
    if [ ! -L "$dest" ] && payload_complete "$dest" "$pu_version"; then
        say "phone access for pu $pu_version is already installed at $dest; left unchanged"
    else
        phone_die "$dest exists but is not a complete payload; stop the bridge (pu phone stop), remove it, and run this again"
    fi
else
    # GNU mv -T renames onto the exact name and never moves into a directory
    # that appeared meanwhile.
    mv -T "$payload" "$dest" 2>/dev/null || phone_die "could not move the payload into $dest (another install running?)"
    say "installed phone access for pu $pu_version to $dest"
fi

say ""
say "next step: run the phone bridge as a user service:"
say "  $INSTALL_DIR/pu phone install --now"
