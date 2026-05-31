#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE="shush-remote-ssh:dev"
CONTAINER="shush-remote-ssh"
PORT="2222"
KEY_FILE="${ROOT_DIR}/.remote-ssh-key"
PUB_FILE="${KEY_FILE}.pub"
HOME_DIR="${ROOT_DIR}/.remote-ssh-home"
SSH_CONFIG="${HOME_DIR}/.ssh/config"
SERVER_LOG="${ROOT_DIR}/.remote-server.log"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" >/dev/null 2>&1 || true
  fi
  docker rm -f "${CONTAINER}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

if [[ ! -f "${KEY_FILE}" ]]; then
  ssh-keygen -t ed25519 -N "" -f "${KEY_FILE}" -C "shush-remote-dev" >/dev/null
fi

mkdir -p "${HOME_DIR}/.ssh"
chmod 700 "${HOME_DIR}/.ssh"
cat > "${SSH_CONFIG}" <<CFG
Host shush-docker
  HostName 127.0.0.1
  User shush
  Port ${PORT}
  IdentityFile ${KEY_FILE}
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
CFG
chmod 600 "${SSH_CONFIG}"

AUTHORIZED_KEY="$(cat "${PUB_FILE}")"

docker build -t "${IMAGE}" "${ROOT_DIR}/docker/remote-ssh"
docker rm -f "${CONTAINER}" >/dev/null 2>&1 || true
docker run -d --name "${CONTAINER}" -p "${PORT}:22" -e "AUTHORIZED_KEY=${AUTHORIZED_KEY}" "${IMAGE}" >/dev/null

for _ in {1..30}; do
  if ssh -F "${SSH_CONFIG}" shush-docker "echo ok" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

ssh -F "${SSH_CONFIG}" shush-docker "tmux -V >/dev/null"

HOME="${HOME_DIR}" SHUSH_SSH_CONFIG="${SSH_CONFIG}" cargo run -- server >"${SERVER_LOG}" 2>&1 &
SERVER_PID=$!

for _ in {1..30}; do
  if curl -sSf "http://127.0.0.1:8100/api/sessions" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

CREATE_RESPONSE="$(curl -sSf -X POST "http://127.0.0.1:8100/api/sessions" -H "Content-Type: application/json" -d '{"name":"remote-validate","host":"shush-docker"}')"
SESSION_ID="$(python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])' <<<"${CREATE_RESPONSE}")"

ssh -F "${SSH_CONFIG}" shush-docker "tmux -L shush has-session -t remote-validate"
ssh -F "${SSH_CONFIG}" shush-docker "tmux -L shush send-keys -t remote-validate 'echo remote-ok' Enter"
sleep 1

ssh -F "${SSH_CONFIG}" shush-docker "tmux -L shush capture-pane -p -t remote-validate | grep remote-ok"

curl -sSf -X DELETE "http://127.0.0.1:8100/api/sessions/${SESSION_ID}" >/dev/null
if ssh -F "${SSH_CONFIG}" shush-docker "tmux -L shush has-session -t remote-validate" >/dev/null 2>&1; then
  echo "remote session still exists after delete" >&2
  exit 1
fi

echo "Remote SSH validation passed"
