.PHONY: install build test e2e dev

PORT ?= 8100
ROOT_DIR := $(CURDIR)

REMOTE_TARGET_CMD := ./scripts/remote_ssh_target.sh
SHUSH_E2E_CONTAINER ?= shush-remote-ssh
SHUSH_E2E_REMOTE_HOST ?= shush-docker
SHUSH_E2E_REMOTE_HOME ?= $(ROOT_DIR)/.remote-ssh-home
SHUSH_SSH_CONFIG ?= $(SHUSH_E2E_REMOTE_HOME)/.ssh/config

# Container as remote for testing
REMOTE_ENVS := \
	SHUSH_E2E_REMOTE_HOST="$(SHUSH_E2E_REMOTE_HOST)" \
	SHUSH_E2E_REMOTE_HOME="$(SHUSH_E2E_REMOTE_HOME)" \
	SHUSH_SSH_CONFIG="$(SHUSH_SSH_CONFIG)"

# E2E tests using container as remote
E2E_ENVS := \
	SHUSH_E2E_REMOTE=1 \
	SHUSH_E2E_ASSERT_STREAM=1 \
	$(REMOTE_ENVS)

# Server env for dev
SERVER_ENVS := \
	SHUSH_SSH_CONFIG="$(SHUSH_SSH_CONFIG)"

install:
	cd frontend && npm install

build: install
	cd frontend && npm run build

test: build
	cargo test --verbose
	cd frontend && npm run test

e2e: test
	@set -e; \
	cleanup() { \
		status=$$1; \
		trap - EXIT INT TERM; \
		$(REMOTE_TARGET_CMD) stop >/dev/null 2>&1 || true; \
		exit $$status; \
	}; \
	trap 'cleanup $$?' EXIT; \
	trap 'cleanup 130' INT; \
	trap 'cleanup 143' TERM; \
	$(REMOTE_ENVS) $(REMOTE_TARGET_CMD) restart; \
	cd frontend && $(E2E_ENVS) npm run test:e2e

dev: build
	@set -e; \
	cleanup() { \
		status=$$1; \
		trap - EXIT INT TERM; \
		$(REMOTE_TARGET_CMD) stop >/dev/null 2>&1 || true; \
		exit $$status; \
	}; \
	trap 'cleanup $$?' EXIT; \
	trap 'cleanup 130' INT; \
	trap 'cleanup 143' TERM; \
	$(REMOTE_ENVS) $(REMOTE_TARGET_CMD) restart; \
	$(SERVER_ENVS) cargo run -- server --port $(PORT)
