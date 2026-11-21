.PHONY: build test integration-test fmt fmt-check lint deny clean deploy-local deploy-testnet setup setup-hooks create-stream
.PHONY: build test integration-test fmt fmt-check lint deny clean deploy-local deploy-testnet setup setup-hooks stream-status load-test-withdraw

build:
	stellar contract build

test:
	cargo build -p paystream-stream --target wasm32v1-none --release
	cargo test

coverage:
	cargo llvm-cov --workspace --summary-only

mutation-test:
	cargo mutants -p paystream-stream

integration-test:
	docker run -d --rm --name paystream-sandbox -p 8000:8000 stellar/quickstart:latest --local --enable-soroban-rpc
	./tests/integration/run.sh; status=$$?; docker stop paystream-sandbox >/dev/null; exit $$status

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

lint:
	cargo clippy --all-targets -- -D warnings

check:
	cargo check --all

deny:
	cargo deny check

clean:
	cargo clean

deploy-local:
	./scripts/deploy-local.sh

deploy-testnet:
	@set -a; . deploy/testnet.env; set +a; ./scripts/deploy-testnet.sh

deploy-mainnet:
	@if [ ! -f deploy/mainnet.env ]; then \
		echo "Missing deploy/mainnet.env. Copy deploy/mainnet.env.example and fill in values."; \
		exit 1; \
	fi
	@set -a; . deploy/mainnet.env; set +a; ./scripts/deploy-testnet.sh

# Verify the deployed WASM hash matches the locally-built artifact.
# Usage: make verify-deploy CONTRACT_ID=<ID> [WASM=target/wasm32v1-none/release/paystream_stream.wasm] [NETWORK=testnet]
verify-deploy:
	@if [ -z "$(CONTRACT_ID)" ]; then \
		echo "Usage: make verify-deploy CONTRACT_ID=<ID> [WASM=<path>] [NETWORK=testnet]"; \
		exit 1; \
	fi
	@wasm="$(if $(WASM),$(WASM),target/wasm32v1-none/release/paystream_stream.wasm)"; \
	if [ ! -f "$$wasm" ]; then \
		echo "Local WASM artifact not found: $$wasm"; \
		exit 1; \
	fi; \
	network="$(if $(NETWORK),$(NETWORK),testnet)"; \
	local_hash=$$(sha256sum "$$wasm" | awk '{print $$1}'); \
	deployed_hash=$$(stellar contract info --id "$(CONTRACT_ID)" --network "$$network" 2>/dev/null | grep -i 'wasm' | grep -oE '[0-9a-fA-F]{64}' | head -n1); \
	if [ -z "$$deployed_hash" ]; then \
		echo "Could not extract deployed WASM hash for $(CONTRACT_ID) on $$network"; \
		exit 1; \
	fi; \
	if [ "$$local_hash" = "$$deployed_hash" ]; then \
		result="✅ MATCH"; \
		status=0; \
	else \
		result="❌ MISMATCH"; \
		status=1; \
	fi; \
	echo "Contract: $(CONTRACT_ID) ($$network)"; \
	echo "Local SHA256:    $$local_hash"; \
	echo "Deployed WASM:   $$deployed_hash"; \
	echo "Result: $$result"; \
	if [ -n "$$GITHUB_STEP_SUMMARY" ]; then \
		{ \
			echo "### Contract checksum verification"; \
			echo ""; \
			echo "| Field | Value |"; \
			echo "| --- | --- |"; \
			echo "| Contract ID | \`$(CONTRACT_ID)\` |"; \
			echo "| Network | $$network |"; \
			echo "| Local SHA256 | \`$$local_hash\` |"; \
			echo "| Deployed WASM | \`$$deployed_hash\` |"; \
			echo "| Result | $$result |"; \
		} >> "$$GITHUB_STEP_SUMMARY"; \
	fi; \
	exit $$status

# Export an employer's streams as CSV for payroll reconciliation.
# Add JSON=1 to emit JSON instead of CSV, NETWORK=local to query another network.
export-streams:
	./scripts/export-streams.sh "$(CONTRACT_ID)" "$(EMPLOYER)" $(if $(NETWORK),--network $(NETWORK),) $(if $(JSON),--json,)

# Apply .github/labels.yml to the repository (add --dry-run to preview).
sync-labels:
	./scripts/sync-labels.sh

setup:
	@echo "Setting up development environment..."
	@echo "Checking for Rust installation..."
	@which rustc > /dev/null 2>&1 || (echo "Installing Rust via rustup..." && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y)
	@. "$$HOME/.cargo/env" && which rustc > /dev/null 2>&1 && echo "✓ Rust installed" || (echo "✗ Rust installation failed. Please run: source $$HOME/.cargo/env" && exit 1)
	@echo "Adding wasm32-unknown-unknown target..."
	@. "$$HOME/.cargo/env" && rustup target add wasm32-unknown-unknown && echo "✓ wasm32-unknown-unknown target added" || (echo "✗ Failed to add wasm32-unknown-unknown target" && exit 1)
	@echo "Installing Stellar CLI (version 27.0.0)..."
	@. "$$HOME/.cargo/env" && cargo install --locked stellar-cli --version 27.0.0 && echo "✓ Stellar CLI installed" || (echo "✗ Stellar CLI installation failed" && exit 1)
	@echo ""
	@echo "Setup complete! You can now run 'make test' to verify your installation."
	@echo "Note: You may need to run 'source $$HOME/.cargo/env' or restart your terminal for Rust to be available in new shell sessions."

setup-hooks:
	@echo "Installing pre-commit hook..."
	@cp scripts/pre-commit-hook.sh .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@echo "✓ Pre-commit hook installed successfully."
	@echo "The hook will run 'cargo fmt --check' and 'cargo clippy' before each commit."
	@echo "To skip the hook, use: git commit --no-verify"

create-stream:
	./scripts/create-stream-wizard.sh

# Drive N concurrent withdraws against a sandbox to measure throughput and contention.
# Usage: make load-test-withdraw [N=<employees>] [CONCURRENCY=<parallel withdraws>]
load-test-withdraw:
	./scripts/load-test-withdraw.sh $(if $(N),N=$(N),) $(if $(CONCURRENCY),CONCURRENCY=$(CONCURRENCY),)

stream-status:
	@if [ -z "$(CONTRACT_ID)" ] || [ -z "$(EMPLOYER)" ]; then \
		echo "Usage: make stream-status CONTRACT_ID=<ID> EMPLOYER=<ADDRESS> [NETWORK=testnet]"; \
		exit 1; \
	fi
	@./scripts/stream-status.sh "$(CONTRACT_ID)" "$(EMPLOYER)" "$(NETWORK)"
