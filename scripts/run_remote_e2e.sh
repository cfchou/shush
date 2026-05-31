#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_SCRIPT="${ROOT_DIR}/scripts/remote_ssh_target.sh"

cleanup() {
  "${TARGET_SCRIPT}" stop >/dev/null 2>&1 || true
}
trap cleanup EXIT

"${TARGET_SCRIPT}" start

pushd "${ROOT_DIR}/frontend" >/dev/null
SHUSH_E2E_REMOTE=1 npm run test:e2e
popd >/dev/null
