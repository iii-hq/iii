.PHONY: test test-js test-py

test: test-py test-js 

test-js:
	@cd motia-js && pnpm install && pnpm test

test-py:
	@cd motia-py/packages/motia && $(MAKE) test
