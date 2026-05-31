#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${AUTHORIZED_KEY:-}" ]]; then
  echo "AUTHORIZED_KEY env var is required" >&2
  exit 1
fi

install -d -m 700 -o shush -g shush /home/shush/.ssh
printf '%s\n' "$AUTHORIZED_KEY" > /home/shush/.ssh/authorized_keys
chown shush:shush /home/shush/.ssh/authorized_keys
chmod 600 /home/shush/.ssh/authorized_keys

exec /usr/sbin/sshd -D -e
