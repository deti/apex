#!/bin/sh
# APEX installer — downloads the latest release binary for your platform.
# Usage: curl -sSL https://raw.githubusercontent.com/sahajamoth/apex/main/install.sh | sh

set -eu

REPO="sahajamoth/apex"
BINARY="apex"
INSTALL_DIR="${APEX_INSTALL_DIR:-/usr/local/bin}"

main() {
    os=$(uname -s)
    arch=$(uname -m)

    case "${os}" in
        Linux)  target_os="unknown-linux-gnu" ;;
        Darwin) target_os="apple-darwin" ;;
        *)      echo "Error: unsupported OS: ${os}" >&2; exit 1 ;;
    esac

    case "${arch}" in
        x86_64|amd64)  target_arch="x86_64" ;;
        arm64|aarch64) target_arch="aarch64" ;;
        *)             echo "Error: unsupported architecture: ${arch}" >&2; exit 1 ;;
    esac

    target="${target_arch}-${target_os}"

    # Get latest release tag
    if command -v curl >/dev/null 2>&1; then
        fetch="curl -sSL"
        fetch_redirect="curl -sSL -o"
    elif command -v wget >/dev/null 2>&1; then
        fetch="wget -qO-"
        fetch_redirect="wget -qO"
    else
        echo "Error: curl or wget required" >&2
        exit 1
    fi

    latest=$(${fetch} "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

    if [ -z "${latest}" ]; then
        echo "Error: could not determine latest release" >&2
        exit 1
    fi

    url="https://github.com/${REPO}/releases/download/${latest}/${BINARY}-${target}.tar.gz"

    echo "Installing ${BINARY} ${latest} (${target})..."

    tmpdir=$(mktemp -d)
    trap 'rm -rf "${tmpdir}"' EXIT

    ${fetch_redirect} "${tmpdir}/archive.tar.gz" "${url}"
    tar xzf "${tmpdir}/archive.tar.gz" -C "${tmpdir}"

    if [ -w "${INSTALL_DIR}" ]; then
        mv "${tmpdir}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    else
        echo "Installing to ${INSTALL_DIR} (requires sudo)..."
        sudo mv "${tmpdir}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    fi

    chmod +x "${INSTALL_DIR}/${BINARY}"
    echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
    "${INSTALL_DIR}/${BINARY}" --version 2>/dev/null || true
}

main
