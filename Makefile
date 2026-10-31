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

export III_TELEMETRY_ENABLED := false

.PHONY: install install-node install-python install-hooks \
        engine-build engine-test coverage engine-fmt-check \
        engine-up engine-up-bridges engine-down \
        test-sdk-node test-sdk-python test-sdk-rust test-sdk-all \
        lint-python lint-rust lint \
        fmt-check fmt-check-rust fmt-check-all \
        typecheck-node typecheck-python typecheck \
        build-node build-sdk-node build \
        fix fix-lint fix-fmt \
        check ci-engine ci-sdk-node ci-sdk-python ci-sdk-rust \
        ci-local cli-docs

# ── Setup ─────────────────────────────────────────────────────────────────────

install: install-node install-python

install-node:
	pnpm install --frozen-lockfile

install-python:
	cd $(PYTHON_SDK_DIR) && uv sync --extra dev

install-hooks:
	git config core.hooksPath .githooks
	@echo "[hooks] pre-commit installed (core.hooksPath=.githooks)"


# ── Engine ────────────────────────────────────────────────────────────────────

engine-build:
	cargo build -p iii --all-features

# Mirrors the CI test set (Engine Coverage job). cargo test includes doctests.
engine-test:
	cargo test -p iii -p iii-sandbox-contract --all-features

coverage: ## Run the engine test suite under llvm-cov (same as CI), prints a report
	@eval "$$(cargo llvm-cov show-env --export-prefix)" && \
		cargo test -p iii -p iii-sandbox-contract --all-features && \
		cargo llvm-cov report

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
		cargo test -p iii-sdk -p iii-helpers -p iii-observability --all-features

test-sdk-all: test-sdk-node test-sdk-python test-sdk-rust

# ── Lint ──────────────────────────────────────────────────────────────────────

lint-python:
	cd $(PYTHON_SDK_DIR) && uv run ruff check src

lint-rust:
	cargo clippy -p iii-sdk -p iii-helpers -p iii-observability --all-targets --all-features -- -D warnings

lint: lint-python lint-rust

# ── Format Check ──────────────────────────────────────────────────────────────

fmt-check-rust:
	cargo fmt -p iii-sdk -p iii-helpers -p iii-observability -- --check

fmt-check-all: engine-fmt-check fmt-check-rust

# ── Type Check ────────────────────────────────────────────────────────────────

typecheck-node:
	pnpm --filter iii-sdk exec tsc --noEmit

typecheck-python:
	cd $(PYTHON_SDK_DIR) && uv run mypy src

typecheck: typecheck-node typecheck-python

# ── Build ─────────────────────────────────────────────────────────────────────

build-sdk-node:
	pnpm --filter iii-sdk build

build: ## Build everything: engine
	cargo build --release -p iii
	@echo ""
	@echo "Build complete. Binaries:"
	@echo "  engine:   $(CURDIR)/target/release/iii"
	@echo ""
	@echo "Add them to your PATH:"
	@echo '  export PATH="$(CURDIR)/target/release:$$PATH"'

# ── CI Jobs (mirror ci.yml; CI additionally passes --locked to cargo) ──────────────────────────────────────────────────

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

# ── Convenience ───────────────────────────────────────────────────────────────

cli-docs: ## Regenerate docs/next/cli-reference/ from the clap CLI definitions
	./scripts/generate-cli-docs.sh

fix: fix-fmt fix-lint

fix-fmt:
	cargo fmt --all

fix-lint:
	cd $(PYTHON_SDK_DIR) && uv run ruff check --fix --unsafe-fixes src && uv run ruff format src
	cargo clippy -p iii-sdk --all-targets --all-features --fix --allow-dirty --allow-staged -- -D warnings

check: lint fmt-check-all typecheck build-sdk-node

ci-local: ci-engine ci-sdk-node ci-sdk-python ci-sdk-rust
