.PHONY: fmt check test smoke mcp release-test publish-pages

fmt:
	cargo fmt --check

check:
	cargo check

test:
	./scripts/test.sh

smoke:
	./scripts/smoke.sh

mcp:
	./scripts/test-mcp.sh

release-test:
	cargo build --release

publish-pages:
	./scripts/publish-pages.sh
