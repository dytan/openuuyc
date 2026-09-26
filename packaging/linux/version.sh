#!/usr/bin/env bash
# Resolve OpenUUYC version from Cargo.toml (first package version = line).
# Usage: VERSION="$(./packaging/linux/version.sh)"
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
sed -n 's/^version = "\(.*\)"/\1/p' "${ROOT}/Cargo.toml" | head -1
