# ── Configuration ─────────────────────────────────────────────────────────────

TEST_ENGINE_CONFIG  := sdk/fixtures/config-test.yaml
BRIDGE_BE_CONFIG    := sdk/fixtures/config-bridge-backend.yaml
BRIDGE_CONFIG       := sdk/fixtures/config-bridge.yaml
ENGINE_BIN          := ./target/debug/iii
START_SCRIPT        := bash scripts/start-iii.sh
STOP_SCRIPT         := bash scripts/stop-iii.sh
III_URL             := ws://localhost:49199
III_HTTP_URL        := http://localhost:3199
PYTHON_SDK_DIR      := sdk/packages/python/iii
MOTIA_PY_DIR        := frameworks/motia/motia-py/packages/motia

export III_TELEMETRY_ENABLED := false

.PHONY: install install-node install-python install-motia-py \
        engine-build engine-test engine-fmt-check \
        engine-up engine-up-bridges engine-down \
        test-sdk-node test-sdk-python test-sdk-rust test-sdk-all \
        test-motia-js test-motia-py \
        lint-python lint-rust lint-console lint \
        fmt-check fmt-check-rust fmt-check-all \
        typecheck-node typecheck-python typecheck-motia-py typecheck \
        build-node build-sdk-node build-motia-js build-console build \
        fix fix-lint fix-fmt \
        check ci-engine ci-sdk-node ci-sdk-python ci-sdk-rust \
        ci-motia-js ci-motia-py ci-console ci-local

# ── Setup ─────────────────────────────────────────────────────────────────────

install: install-node install-python install-motia-py

install-node:
	pnpm install --frozen-lockfile

install-python:
	cd $(PYTHON_SDK_DIR) && uv sync --extra dev

install-motia-py:
	cd $(MOTIA_PY_DIR) && uv sync --extra dev

# ── Engine ────────────────────────────────────────────────────────────────────

engine-build:
	cargo build -p iii --all-features

engine-test:
	cargo test -p iii --all-features

engine-fmt-check:
	cargo fmt --all -- --check

engine-up:
	$(START_SCRIPT) --binary $(ENGINE_BIN) --config $(TEST_ENGINE_CONFIG) --port 49199

engine-up-bridges: engine-up
	$(START_SCRIPT) --binary $(ENGINE_BIN) \
		--config $(BRIDGE_BE_CONFIG) --port 49198 \
		--pid-file /tmp/iii-backend.pid --log-file /tmp/iii-backend.log
	$(START_SCRIPT) --binary $(ENGINE_BIN) \
		--config $(BRIDGE_CONFIG) --port 49197 \
		--pid-file /tmp/iii-bridge.pid --log-file /tmp/iii-bridge.log

engine-down:
	$(STOP_SCRIPT) /tmp/iii-engine.pid /tmp/iii-backend.pid /tmp/iii-bridge.pid

# ── SDK Tests ─────────────────────────────────────────────────────────────────

test-sdk-node:
	III_URL=$(III_URL) III_HTTP_URL=$(III_HTTP_URL) \
		pnpm --filter iii-sdk test:coverage

test-sdk-python:
	cd $(PYTHON_SDK_DIR) && \
		III_URL=$(III_URL) III_HTTP_URL=$(III_HTTP_URL) \
		uv run pytest -q

test-sdk-rust:
	III_URL=$(III_URL) III_HTTP_URL=$(III_HTTP_URL) \
		cargo test -p iii-sdk --all-features

test-sdk-all: test-sdk-node test-sdk-python test-sdk-rust

# ── Framework Tests ───────────────────────────────────────────────────────────

test-motia-js:
	III_URL=$(III_URL) pnpm --filter motia test:ci

test-motia-py:
	cd $(MOTIA_PY_DIR) && \
		III_URL=$(III_URL) III_HTTP_URL=$(III_HTTP_URL) \
		uv run pytest --cov=src --cov-report=term-missing

# ── Lint ──────────────────────────────────────────────────────────────────────

lint-python:
	cd $(PYTHON_SDK_DIR) && uv run ruff check src

lint-motia-py:
	cd $(MOTIA_PY_DIR) && uv run ruff check src

lint-rust:
	cargo clippy -p iii-sdk --all-targets --all-features -- -D warnings

lint-console:
	pnpm --filter console-frontend lint

lint: lint-python lint-motia-py lint-rust lint-console

# ── Format Check ──────────────────────────────────────────────────────────────

fmt-check-rust:
	cargo fmt -p iii-sdk -- --check

fmt-check-all: engine-fmt-check fmt-check-rust

# ── Type Check ────────────────────────────────────────────────────────────────

typecheck-node:
	pnpm --filter iii-sdk exec tsc --noEmit

typecheck-python:
	cd $(PYTHON_SDK_DIR) && uv run mypy src

typecheck-motia-py:
	cd $(MOTIA_PY_DIR) && uv run mypy src

typecheck: typecheck-node typecheck-python typecheck-motia-py

# ── Build ─────────────────────────────────────────────────────────────────────

build-sdk-node:
	pnpm --filter iii-sdk build

build-motia-js: build-sdk-node
	pnpm --filter motia build

build-console:
	pnpm --filter console-frontend build
	cargo build -p iii-console --release

build: build-motia-js build-console

# ── CI Jobs (mirror ci.yml) ──────────────────────────────────────────────────

ci-engine: engine-build engine-test engine-fmt-check

ci-sdk-node: engine-up-bridges
	@trap '$(MAKE) engine-down' EXIT; \
	$(MAKE) typecheck-node build-sdk-node test-sdk-node

ci-sdk-python: engine-up
	@trap '$(MAKE) engine-down' EXIT; \
	$(MAKE) install-python lint-python typecheck-python test-sdk-python

ci-sdk-rust: engine-up
	@trap '$(MAKE) engine-down' EXIT; \
	$(MAKE) fmt-check-rust lint-rust test-sdk-rust

ci-motia-js: engine-up
	@trap '$(MAKE) engine-down' EXIT; \
	$(MAKE) build-motia-js test-motia-js

ci-motia-py: engine-up
	@trap '$(MAKE) engine-down' EXIT; \
	$(MAKE) install-motia-py lint-motia-py typecheck-motia-py test-motia-py

ci-console:
	$(MAKE) lint-console build-console

# ── Convenience ───────────────────────────────────────────────────────────────

fix: fix-fmt fix-lint

fix-fmt:
	cargo fmt --all

fix-lint:
	cd $(PYTHON_SDK_DIR) && uv run ruff check --fix --unsafe-fixes src && uv run ruff format src
	cd $(MOTIA_PY_DIR) && uv run ruff check --fix --unsafe-fixes src && uv run ruff format src
	cargo clippy -p iii-sdk --all-targets --all-features --fix --allow-dirty --allow-staged -- -D warnings
	pnpm --filter console-frontend run lint:fix

check: lint fmt-check-all typecheck build

ci-local: ci-engine ci-sdk-node ci-sdk-python ci-sdk-rust ci-motia-js ci-motia-py ci-console
