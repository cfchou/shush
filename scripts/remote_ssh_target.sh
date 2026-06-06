#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="shush-remote-ssh:dev"
PORT="2222"
KEY_FILE="${ROOT_DIR}/.remote-ssh-key"
PUB_FILE="${KEY_FILE}.pub"
CONTAINER="${SHUSH_E2E_CONTAINER:-shush-remote-ssh}"
REMOTE_HOST="${SHUSH_E2E_REMOTE_HOST:-shush-docker}"
REMOTE_HOME="${SHUSH_E2E_REMOTE_HOME:-${ROOT_DIR}/.remote-ssh-home}"
SSH_CONFIG="${SHUSH_SSH_CONFIG:-${REMOTE_HOME}/.ssh/config}"

ssh_config_quote() {
  printf '"%s"' "${1//\"/\\\"}"
}

usage() {
  cat <<EOF
Usage: $(basename "$0") [start|stop|restart|cleanup]

Commands:
  start    Build image, start container, and verify SSH/tmux (default)
  stop     Stop and remove the remote SSH container
  restart  Stop container, then start target again
  cleanup  Stop/remove container and delete local generated SSH artifacts

Examples:
  ./scripts/remote_ssh_target.sh
  ./scripts/remote_ssh_target.sh start
  ./scripts/remote_ssh_target.sh stop
  ./scripts/remote_ssh_target.sh restart
  ./scripts/remote_ssh_target.sh cleanup
EOF
}

stop_container() {
  docker rm -f "${CONTAINER}" >/dev/null 2>&1 || true
  echo "Stopped container: ${CONTAINER}"
}

cleanup_all() {
  stop_container
  rm -f "${KEY_FILE}" "${PUB_FILE}" >/dev/null 2>&1 || true
  rm -rf "${REMOTE_HOME}" >/dev/null 2>&1 || true
  echo "Removed generated SSH artifacts: ${KEY_FILE}, ${REMOTE_HOME}"
}

start_target() {
  if [[ ! -f "${KEY_FILE}" ]]; then
    ssh-keygen -t ed25519 -N "" -f "${KEY_FILE}" -C "shush-remote-dev" >/dev/null
  fi

  mkdir -p "${REMOTE_HOME}/.ssh"
  chmod 700 "${REMOTE_HOME}/.ssh"
  local identity_file_quoted
  identity_file_quoted="$(ssh_config_quote "${KEY_FILE}")"
  cat > "${SSH_CONFIG}" <<CFG
Host ${REMOTE_HOST}
  HostName 127.0.0.1
  User shush
  Port ${PORT}
  IdentityFile ${identity_file_quoted}
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
CFG
  chmod 600 "${SSH_CONFIG}"

  AUTHORIZED_KEY="$(cat "${PUB_FILE}")"

  docker build -t "${IMAGE}" "${ROOT_DIR}/docker/remote-ssh"
  docker rm -f "${CONTAINER}" >/dev/null 2>&1 || true
  docker run -d --name "${CONTAINER}" -p "${PORT}:22" -e "AUTHORIZED_KEY=${AUTHORIZED_KEY}" "${IMAGE}" >/dev/null

  for _ in {1..30}; do
    if ssh -F "${SSH_CONFIG}" "${REMOTE_HOST}" "echo ok" >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done

  ssh -F "${SSH_CONFIG}" "${REMOTE_HOST}" "tmux -V >/dev/null"

  cat <<EOF
Remote SSH target ready.

SSH config: ${SSH_CONFIG}
Remote host alias: ${REMOTE_HOST}
Remote home: ${REMOTE_HOME}

Run remote E2E:
  cd frontend
  SHUSH_E2E_REMOTE=1 npm run test:e2e
EOF
}

COMMAND="${1:-start}"

case "${COMMAND}" in
start)
  start_target
  ;;
stop)
  stop_container
  ;;
restart)
  stop_container
  start_target
  ;;
cleanup)
  cleanup_all
  ;;
help|-h|--help)
  usage
  ;;
*)
  echo "Unknown command: ${COMMAND}" >&2
  usage
  exit 1
  ;;
esac
