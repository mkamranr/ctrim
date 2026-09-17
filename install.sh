#!/bin/sh
# Install ctrim, the terminal-output token trimmer.
#
#   curl -fsSL https://raw.githubusercontent.com/mkamranr/ctrim/main/install.sh | sh
#
# Environment:
#   CTRIM_VERSION   version to install (default: latest release)
#   CTRIM_BIN_DIR   install directory (default: ~/.local/bin, or /usr/local/bin if writable)
set -eu

REPO="mkamranr/ctrim"

say() { printf 'ctrim: %s\n' "$1" >&2; }
die() { say "$1"; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"; }

need uname
need tar

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1"; }
  fetch_to() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO- "$1"; }
  fetch_to() { wget -qO "$2" "$1"; }
else
  die "need curl or wget"
fi

os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin) case "$arch" in
            arm64|aarch64) target="aarch64-apple-darwin" ;;
            x86_64)        target="x86_64-apple-darwin" ;;
            *) die "unsupported macOS architecture: $arch" ;;
          esac ;;
  Linux)  case "$arch" in
            x86_64|amd64)  target="x86_64-unknown-linux-musl" ;;
            aarch64|arm64) target="aarch64-unknown-linux-musl" ;;
            *) die "unsupported Linux architecture: $arch" ;;
          esac ;;
  *) die "unsupported OS: $os (Windows users: download the .tar.gz from the releases page)" ;;
esac

version="${CTRIM_VERSION:-}"
if [ -z "$version" ]; then
  say "looking up the latest release"
  version="$(fetch "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$version" ] || die "could not determine the latest version; set CTRIM_VERSION"
fi

name="ctrim-${version}-${target}"
url="https://github.com/$REPO/releases/download/v${version}/${name}.tar.gz"

bin_dir="${CTRIM_BIN_DIR:-}"
if [ -z "$bin_dir" ]; then
  if [ -w /usr/local/bin ] 2>/dev/null; then bin_dir=/usr/local/bin; else bin_dir="$HOME/.local/bin"; fi
fi
mkdir -p "$bin_dir"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "downloading $name"
fetch_to "$url" "$tmp/ctrim.tar.gz" || die "download failed: $url"

# Verify the checksum when the release publishes one and a tool exists to check it.
if fetch_to "${url}.sha256" "$tmp/ctrim.tar.gz.sha256" 2>/dev/null; then
  expected="$(cut -d' ' -f1 <"$tmp/ctrim.tar.gz.sha256")"
  if command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$tmp/ctrim.tar.gz" | cut -d' ' -f1)"
  elif command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp/ctrim.tar.gz" | cut -d' ' -f1)"
  else
    actual=""
    say "no sha256 tool found, skipping checksum verification"
  fi
  if [ -n "$actual" ] && [ "$actual" != "$expected" ]; then
    die "checksum mismatch: expected $expected, got $actual"
  fi
fi

tar xzf "$tmp/ctrim.tar.gz" -C "$tmp"
install_src="$tmp/$name/ctrim"
[ -f "$install_src" ] || install_src="$(find "$tmp" -name ctrim -type f | head -n 1)"
[ -f "$install_src" ] || die "archive did not contain a ctrim binary"

chmod +x "$install_src"
mv "$install_src" "$bin_dir/ctrim"
say "installed ctrim $version to $bin_dir/ctrim"

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) say "add it to your PATH:  export PATH=\"$bin_dir:\$PATH\"" ;;
esac

say "try it:  git diff | ctrim"
