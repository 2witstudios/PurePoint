#!/usr/bin/env bash
# Minisign-format Ed25519 signatures made with OpenSSL 3, so the release job
# needs no extra signing tool. Signatures use minisign's prehashed format
# (BLAKE2b-512) and verify with `minisign -Vm` or with scripts/install.sh.
#
#   minisign.sh keygen <private-key.pem>   create a key, print its public key
#   minisign.sh pubkey <private-key.pem>   print the public key of a key
#   minisign.sh sign <private-key.pem> <file>...   write <file>.minisig
#
# The key ID is the first 8 bytes of SHA-256 over the raw public key, so the
# public key is fully determined by the private key.
set -euo pipefail

die() { printf 'minisign.sh: %s\n' "$*" >&2; exit 1; }

raw_public_key() {
  openssl pkey -in "$1" -pubout -outform DER | tail -c 32
}

key_id() {
  raw_public_key "$1" | openssl dgst -sha256 -binary | head -c 8
}

public_key() {
  { printf 'Ed'; key_id "$1"; raw_public_key "$1"; } | openssl base64 -A
  printf '\n'
}

ed25519_sign() {
  openssl pkeyutl -sign -inkey "$1" -rawin -in "$2"
}

sign_file() {
  local key=$1 file=$2 work
  work=$(mktemp -d)
  openssl dgst -blake2b512 -binary "$file" > "$work/hash"
  ed25519_sign "$key" "$work/hash" > "$work/sig"
  local trusted
  trusted=$(printf 'timestamp:%s\tfile:%s\thashed' "$(date +%s)" "$(basename "$file")")
  { cat "$work/sig"; printf '%s' "$trusted"; } > "$work/global-input"
  ed25519_sign "$key" "$work/global-input" > "$work/global"
  {
    printf 'untrusted comment: signature from PurePoint release key\n'
    { printf 'ED'; key_id "$key"; cat "$work/sig"; } | openssl base64 -A
    printf '\ntrusted comment: %s\n' "$trusted"
    openssl base64 -A < "$work/global"
    printf '\n'
  } > "$file.minisig"
  rm -rf "$work"
}

command=${1:-}
case "$command" in
  keygen)
    [ $# -eq 2 ] || die "usage: minisign.sh keygen <private-key.pem>"
    [ ! -e "$2" ] || die "$2 already exists"
    (umask 077 && openssl genpkey -algorithm ed25519 -out "$2")
    public_key "$2"
    ;;
  pubkey)
    [ $# -eq 2 ] || die "usage: minisign.sh pubkey <private-key.pem>"
    public_key "$2"
    ;;
  sign)
    [ $# -ge 3 ] || die "usage: minisign.sh sign <private-key.pem> <file>..."
    key=$2
    shift 2
    for file in "$@"; do
      sign_file "$key" "$file"
    done
    ;;
  *)
    die "usage: minisign.sh keygen|pubkey|sign ..."
    ;;
esac
