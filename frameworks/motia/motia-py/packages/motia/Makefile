.PHONY: test

TEST_IMAGE ?= iiidev/iii:latest
TEST_CONTAINER ?= iii-engine
TEST_CONFIG ?= config-test.yaml
HEALTH_PORT ?= 3199
HEALTH_RETRIES ?= 30

test:
	@docker rm -f $(TEST_CONTAINER) >/dev/null 2>&1 || true
	@docker run -d \
		--name $(TEST_CONTAINER) \
		-p 3199:3199 \
		-p 49199:49199 \
		-p 3112:3112 \
		-v "$(PWD)/$(TEST_CONFIG)":/app/config.yaml:ro \
		-e RUST_LOG=info \
		$(TEST_IMAGE)
	@echo "Waiting for III Engine on port $(HEALTH_PORT)..."
	@for i in $$(seq 1 $(HEALTH_RETRIES)); do \
		if nc -z 127.0.0.1 $(HEALTH_PORT) 2>/dev/null; then \
			echo "III Engine is ready!"; \
			break; \
		fi; \
		echo "  attempt $$i/$(HEALTH_RETRIES)..."; \
		sleep 2; \
		if [ $$i -eq $(HEALTH_RETRIES) ]; then \
			echo "Engine did not become ready. Logs:"; \
			docker logs $(TEST_CONTAINER) || true; \
			docker rm -f $(TEST_CONTAINER) >/dev/null 2>&1 || true; \
			exit 1; \
		fi; \
	done
	@uv run pytest; \
	status=$$?; \
	docker rm -f $(TEST_CONTAINER) >/dev/null 2>&1 || true; \
	exit $$status
