#!/usr/bin/env bash
# CI: install nfpm, which makes the deb and the rpm, from its GitHub release, after checking the
# download against the SHA-256 kept here (the one nfpm publishes for that file).
#
# Usage: packaging/linux/install-nfpm.sh amd64|arm64
set -euo pipefail

VERSION=2.43.0
case "${1:?usage: install-nfpm.sh amd64|arm64}" in
  amd64) SUM=4a4402f8f9ea87c669d6a62b333f8e268439974754dd886a01731ecaf9fa6d66 ;;
  arm64) SUM=2529b6b3fbd107b57064158a183de6a9801f78d5232980cf46419564491fa5c2 ;;
  *) echo "unsupported architecture $1" >&2; exit 2 ;;
esac

DEB="$(mktemp -d)/nfpm.deb"
curl --proto '=https' --tlsv1.2 -fsSL -o "$DEB" "https://github.com/goreleaser/nfpm/releases/download/v$VERSION/nfpm_${VERSION}_$1.deb"
echo "$SUM  $DEB" | sha256sum -c -
sudo dpkg -i "$DEB"
nfpm --version
