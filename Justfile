# Display help information about available commands
help:
	@echo "Preconfirmation Gateway - Justfile Commands"
	@echo "==========================================="
	@echo ""
	@echo "QUICK START"
	@echo "  just build-all dev      - Build all Docker images with 'dev' tag"
	@echo "  just setup-and-run      - Setup simulation and run all services"
	@echo ""
	@echo "DOCKER COMPOSE WORKFLOW"
	@echo "  just up [SERVICE] [ENV_FILE]  - Start services (optionally with env file)"
	@echo "  just down                     - Stop all services"
	@echo "  just build [SERVICE]          - Build Docker images via docker-compose"
	@echo "  just logs SERVICE             - View logs for a service"
	@echo ""
	@echo "BUILD DOCKER IMAGES (VERSION = Docker image tag, e.g., 'dev', 'v1.0.0', 'latest')"
	@echo "  just build-gateway VERSION       - Build gateway Docker image"
	@echo "  just build-proposer VERSION      - Build proposer Docker image"
	@echo "  just build-relay VERSION         - Build relay Docker image"
	@echo "  just build-spammer VERSION       - Build spammer Docker image"
	@echo "  just build-signer VERSION        - Build signer Docker image"
	@echo "  just build-beacon-mock VERSION   - Build beacon-mock Docker image"
	@echo "  just build-all VERSION           - Build all Docker images"
	@echo "  Example: just build-all dev      - Builds all images tagged as 'dev'"
	@echo ""
	@echo "SIMULATION SETUP"
	@echo "  just setup-simulation            - Generate .simulation.env with JWTs"
	@echo ""
	@echo "LOCAL BINARY EXECUTION (No Docker)"
	@echo "  just run-local-signer            - Run local signer module"
	@echo "  just run-local-gateway           - Run local gateway module"
	@echo "  just run-local-proposer          - Run local proposer daemon"
	@echo "  just run-local-relay             - Run local relay"
	@echo "  just run-local-spammer           - Run local spammer"
	@echo "  just run-local-beacon-mock       - Run local mock beacon node"
	@echo ""
	@echo "DOCKERIZED EXECUTION (requires building images first with VERSION tag)"
	@echo "  just run-docker-signer [ENV_FILE] [VERSION]      - Run dockerized signer"
	@echo "  just run-docker-beacon-mock [ENV_FILE] [VERSION] - Run dockerized beacon mock"
	@echo "  just run-docker-gateway [ENV_FILE] [VERSION]     - Run dockerized gateway"
	@echo "  just run-docker-proposer [ENV_FILE] [VERSION]    - Run dockerized proposer"
	@echo "  just run-docker-relay [ENV_FILE] [VERSION]       - Run dockerized relay"
	@echo "  just run-docker-spammer [ENV_FILE] [VERSION]     - Run dockerized spammer"
	@echo "  just run-docker-all [ENV_FILE] [VERSION]         - Run all dockerized services"
	@echo "  just setup-and-run [ENV_FILE] [VERSION]          - Setup and run all services"
	@echo "  Default ENV_FILE: .simulation.env, Default VERSION: dev"
	@echo ""
	@echo "PROPOSER OPERATIONS"
	@echo "  just proposer-register URC KEYSTORE [PWD]           - Register proposer BLS keys"
	@echo "  just proposer-opt-in URC ROOT SLASHER COMMITTER KEYSTORE [PWD] - Opt-in"
	@echo "  just proposer-opt-out URC ROOT SLASHER KEYSTORE [PWD]          - Opt-out"
	@echo ""
	@echo "EXAMPLES"
	@echo "  # Build and run locally (no Docker)"
	@echo "  just setup-simulation && just run-local-signer"
	@echo ""
	@echo "  # Build Docker images and run in containers"
	@echo "  just build-all dev && just setup-and-run"
	@echo ""
	@echo "  # Use production tag"
	@echo "  just build-all v1.0.0 && just setup-and-run .simulation.env v1.0.0"
	@echo ""
	@echo "NOTES"
	@echo "  - VERSION is the Docker image tag (e.g., dev, v1.0.0, latest)"
	@echo "  - ENV_FILE defaults to .simulation.env if not specified"
	@echo "  - Images are tagged as: preconfirmation-gateway/<service>:<VERSION>"
	@echo ""

# Simple Docker Compose workflow

up SERVICE="" ENV_FILE="" VERSION="dev":
	#!/usr/bin/env bash
	set -e
	if [ -n "{{ENV_FILE}}" ]; then \
		if [ ! -f "{{ENV_FILE}}" ]; then \
			echo "Error: Environment file {{ENV_FILE}} not found"; \
			exit 1; \
		fi; \
		set -a; \
		source "{{ENV_FILE}}"; \
		set +a; \
		echo "Loaded environment from {{ENV_FILE}}"; \
	fi
	export VERSION={{VERSION}}
	if [ -z "{{SERVICE}}" ]; then \
		docker compose up -d; \
	else \
		docker compose up -d {{SERVICE}}; \
	fi

build SERVICE="":
	@if [ -z "{{SERVICE}}" ]; then \
	  docker compose build; \
	else \
	  docker compose build {{SERVICE}}; \
	fi

down:
	docker compose down

logs SERVICE:
	docker compose logs -f {{SERVICE}}

# ===============================
# Commit-boost style builders
# ===============================

_create-docker-builder:
	docker buildx create --name multiarch-builder --driver docker-container --use > /dev/null 2>&1 || true

# Detect the first supported platform shorthand like linux_amd64 or linux_arm64
_platform:
	@docker buildx inspect --bootstrap | sed -n 's/^ *Platforms: *//p' | cut -d',' -f1 | tr -d ' ' | tr '/' '_'

# Build binary artifact for a workspace bin into ./build/<version>/<platform>

_docker-build-binary version bin: _create-docker-builder
	PLATFORM=`just _platform`; \
	docker buildx build --rm --platform=local \
	  -f provisioning/build.Dockerfile \
	  --output "build/{{version}}/$PLATFORM" \
	  --target output \
	  --build-arg TARGET_CRATE=preconfirmation-gateway \
	  --build-arg BINARY_NAME={{bin}} .

# Build runtime image for a bin using prebuilt artifacts
_docker-build-image version bin: _create-docker-builder
	PLATFORM=`just _platform`; \
	docker buildx build --rm --load \
	  -f provisioning/{{bin}}.Dockerfile \
	  --build-arg BINARIES_PATH=build/{{version}} \
	  --build-arg PLATFORM=$PLATFORM \
	  -t preconfirmation-gateway/{{bin}}:{{version}} .

# Public tasks per service (gateway, relay, proposer, spammer)

build-gateway version: (_docker-build-binary version "gateway") (_docker-build-image version "gateway")
build-relay version:   (_docker-build-binary version "relay")   (_docker-build-image version "relay")
build-proposer version:(_docker-build-binary version "proposer")(_docker-build-image version "proposer")
build-spammer version: (_docker-build-binary version "spammer") (_docker-build-image version "spammer")
build-signer version:  (_docker-build-binary version "local-signer-module") (_docker-build-image version "signer")
build-beacon-mock version: (_docker-build-binary version "beacon-mock") (_docker-build-image version "beacon-mock")

build-all version: _create-docker-builder
	#!/usr/bin/env bash
	set -e
	echo "🚀 Building all binaries in a single optimized Docker build..."
	PLATFORM=`just _platform`
	# Build ALL binaries in one Docker build (MUCH faster than 6 separate builds)
	docker buildx build --rm --platform=local \
	  -f provisioning/build.Dockerfile \
	  --output "build/{{version}}/$PLATFORM" \
	  --target output \
	  --build-arg TARGET_CRATE=preconfirmation-gateway \
	  --build-arg BINARIES="gateway relay proposer spammer local-signer-module beacon-mock" \
	  .
	echo "✅ All binaries built successfully!"
	echo "📦 Creating runtime Docker images..."
	# Now create individual runtime images from the pre-built binaries
	just _docker-build-image {{version}} gateway
	just _docker-build-image {{version}} relay
	just _docker-build-image {{version}} proposer
	just _docker-build-image {{version}} spammer
	just _docker-build-image {{version}} signer
	just _docker-build-image {{version}} beacon-mock
	echo "✅ All Docker images created successfully!"

# ===============================
# Local binary execution (without Docker)
# ===============================

# Generate .simulation.env with JWTs and configuration
setup-simulation:
	cargo run --bin simulation-setup

# Run local signer module
run-local-signer:
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export RUST_LOG=info
	cargo run --bin local-signer-module

# Run local gateway module
run-local-gateway:
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/gateway.config.toml
	export CB_MODULE_ID=gateway-module
	export CB_SIGNER_JWT=$GATEWAY_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	cargo run --bin gateway

# Run local proposer daemon
run-local-proposer:
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_SIGNER_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=debug
	cargo run --bin proposer

# Run local proposer daemon with debug logging
run-local-proposer-debug:
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=debug
	echo "=== Environment Check ==="
	echo "CB_CONFIG: $CB_CONFIG"
	echo "CB_MODULE_ID: $CB_MODULE_ID"
	echo "CB_SIGNER_URL: $CB_SIGNER_URL"
	echo "CB_MODULE_JWT: ${CB_MODULE_JWT:0:10}..."
	echo "========================="
	cargo run --bin proposer -- run

# Register proposer BLS keys with URC contract
proposer-register urc_address keystore password="": 
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	if [ -z "{{password}}" ]; then \
		cargo run --bin proposer -- register --urc-address {{urc_address}} --keystore {{keystore}}; \
	else \
		cargo run --bin proposer -- register --urc-address {{urc_address}} --keystore {{keystore}} --password {{password}}; \
	fi

# Dry run registration (sign but don't send transaction)
proposer-register-dry-run urc_address keystore password="":
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	if [ -z "{{password}}" ]; then \
		cargo run --bin proposer -- register --urc-address {{urc_address}} --keystore {{keystore}} --dry-run; \
	else \
		cargo run --bin proposer -- register --urc-address {{urc_address}} --keystore {{keystore}} --password {{password}} --dry-run; \
	fi

# Opt-in to a slasher with a specific committer
proposer-opt-in urc_address registration_root slasher committer keystore password="":
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	if [ -z "{{password}}" ]; then \
		cargo run --bin proposer -- opt-in-to-slasher --urc-address {{urc_address}} --registration-root {{registration_root}} --slasher {{slasher}} --committer {{committer}} --keystore {{keystore}}; \
	else \
		cargo run --bin proposer -- opt-in-to-slasher --urc-address {{urc_address}} --registration-root {{registration_root}} --slasher {{slasher}} --committer {{committer}} --keystore {{keystore}} --password {{password}}; \
	fi

# Opt-out of a slasher
proposer-opt-out urc_address registration_root slasher keystore password="":
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	if [ -z "{{password}}" ]; then \
		cargo run --bin proposer -- opt-out-of-slasher --urc-address {{urc_address}} --registration-root {{registration_root}} --slasher {{slasher}} --keystore {{keystore}}; \
	else \
		cargo run --bin proposer -- opt-out-of-slasher --urc-address {{urc_address}} --registration-root {{registration_root}} --slasher {{slasher}} --keystore {{keystore}} --password {{password}}; \
	fi

# Get slasher commitment info
proposer-get-commitment urc_address registration_root slasher:
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	cargo run --bin proposer -- get-slasher-commitment --urc-address {{urc_address}} --registration-root {{registration_root}} --slasher {{slasher}}

# Add collateral to existing registration
proposer-add-collateral urc_address registration_root amount_wei keystore password="":
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	if [ -z "{{password}}" ]; then \
		cargo run --bin proposer -- add-collateral --urc-address {{urc_address}} --registration-root {{registration_root}} --amount-wei {{amount_wei}} --keystore {{keystore}}; \
	else \
		cargo run --bin proposer -- add-collateral --urc-address {{urc_address}} --registration-root {{registration_root}} --amount-wei {{amount_wei}} --keystore {{keystore}} --password {{password}}; \
	fi

# Unregister from URC
proposer-unregister urc_address registration_root keystore password="":
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	if [ -z "{{password}}" ]; then \
		cargo run --bin proposer -- unregister --urc-address {{urc_address}} --registration-root {{registration_root}} --keystore {{keystore}}; \
	else \
		cargo run --bin proposer -- unregister --urc-address {{urc_address}} --registration-root {{registration_root}} --keystore {{keystore}} --password {{password}}; \
	fi

# Claim collateral after unregistration delay
proposer-claim-collateral urc_address registration_root keystore password="":
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	if [ -z "{{password}}" ]; then \
		cargo run --bin proposer -- claim-collateral --urc-address {{urc_address}} --registration-root {{registration_root}} --keystore {{keystore}}; \
	else \
		cargo run --bin proposer -- claim-collateral --urc-address {{urc_address}} --registration-root {{registration_root}} --keystore {{keystore}} --password {{password}}; \
	fi

# Claim slashed collateral after slash window
proposer-claim-slashed urc_address registration_root keystore password="":
	#!/usr/bin/env bash
	set -a
	source .simulation.env
	set +a
	export CB_CONFIG=config/proposer.config.toml
	export CB_MODULE_ID=proposer-module
	export CB_MODULE_JWT=$PROPOSER_JWT
	export CB_SIGNER_URL=$CB_SIGNER_URL
	export RUST_LOG=info
	if [ -z "{{password}}" ]; then \
		cargo run --bin proposer -- claim-slashed-collateral --urc-address {{urc_address}} --registration-root {{registration_root}} --keystore {{keystore}}; \
	else \
		cargo run --bin proposer -- claim-slashed-collateral --urc-address {{urc_address}} --registration-root {{registration_root}} --keystore {{keystore}} --password {{password}}; \
	fi

# Run local relay
run-local-relay:
	export RUST_LOG=info
	cargo run --bin relay config/relay.config.toml

# Run local spammer
run-local-spammer:
	export RUST_LOG=info
	cargo run --bin spammer config/spammer.config.toml

# Run local mock beacon node (reads config from simulation.config.toml)
run-local-beacon-mock:
	#!/usr/bin/env bash
	export RUST_LOG=info
	# Extract beacon_api_url and proposer_bls_key from simulation.config.toml
	BEACON_URL=$(grep '^beacon_api_url' config/simulation.config.toml | cut -d'"' -f2)
	PROPOSER_KEY=$(grep '^proposer_bls_key' config/simulation.config.toml | cut -d'"' -f2)
	echo "Starting beacon mock with URL: $BEACON_URL and Proposer Key: $PROPOSER_KEY"
	cargo run --bin beacon-mock -- --url "$BEACON_URL" "$PROPOSER_KEY"

# Generate ECDSA proxy key for gateway
generate-proxy-key-gateway-ecdsa:
	#!/usr/bin/env bash
	set -a
	source .simulation.env 2>/dev/null || true
	set +a
	echo "Generating ECDSA proxy key for gateway..."
	RESPONSE=$(curl -s -w "\n%{http_code}" -X POST $CB_SIGNER_URL/signer/v1/generate_proxy_key \
		-H "Content-Type: application/json" \
		-H "Authorization: Bearer $GATEWAY_PROXY_ECDSA_JWT" \
		-d "{\"pubkey\":\"$GATEWAY_DEFAULT_BLS_KEY\",\"scheme\":\"ecdsa\"}")
	HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
	BODY=$(echo "$RESPONSE" | sed '$d')
	if [ "$HTTP_CODE" -eq 200 ]; then \
		echo "$BODY" | jq; \
	else \
		echo "Error: HTTP $HTTP_CODE"; \
		echo "$BODY"; \
		exit 1; \
	fi

# Generate BLS proxy key for gateway
generate-proxy-key-gateway-bls:
	#!/usr/bin/env bash
	set -a
	source .simulation.env 2>/dev/null || true
	set +a
	echo "Generating BLS proxy key for gateway..."
	RESPONSE=$(curl -s -w "\n%{http_code}" -X POST $CB_SIGNER_URL/signer/v1/generate_proxy_key \
		-H "Content-Type: application/json" \
		-H "Authorization: Bearer $GATEWAY_PROXY_BLS_JWT" \
		-d "{\"pubkey\":\"$GATEWAY_DEFAULT_BLS_KEY\",\"scheme\":\"bls\"}")
	HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
	BODY=$(echo "$RESPONSE" | sed '$d')
	if [ "$HTTP_CODE" -eq 200 ]; then \
		echo "$BODY" | jq; \
	else \
		echo "Error: HTTP $HTTP_CODE"; \
		echo "$BODY"; \
		exit 1; \
	fi

# Generate ECDSA proxy key for proposer
generate-proxy-key-proposer-ecdsa:
	#!/usr/bin/env bash
	set -a
	source .simulation.env 2>/dev/null || true
	set +a
	echo "Generating ECDSA proxy key for proposer..."
	RESPONSE=$(curl -s -w "\n%{http_code}" -X POST $CB_SIGNER_URL/signer/v1/generate_proxy_key \
		-H "Content-Type: application/json" \
		-H "Authorization: Bearer $PROPOSER_PROXY_ECDSA_JWT" \
		-d "{\"pubkey\":\"$PROPOSER_DEFAULT_BLS_KEY\",\"scheme\":\"ecdsa\"}")
	HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
	BODY=$(echo "$RESPONSE" | sed '$d')
	if [ "$HTTP_CODE" -eq 200 ]; then \
		echo "$BODY" | jq; \
	else \
		echo "Error: HTTP $HTTP_CODE"; \
		echo "$BODY"; \
		exit 1; \
	fi

# Generate BLS proxy key for proposer
generate-proxy-key-proposer-bls:
	#!/usr/bin/env bash
	set -a
	source .simulation.env 2>/dev/null || true
	set +a
	echo "Generating BLS proxy key for proposer..."
	RESPONSE=$(curl -s -w "\n%{http_code}" -X POST $CB_SIGNER_URL/signer/v1/generate_proxy_key \
		-H "Content-Type: application/json" \
		-H "Authorization: Bearer $PROPOSER_PROXY_BLS_JWT" \
		-d "{\"pubkey\":\"$PROPOSER_DEFAULT_BLS_KEY\",\"scheme\":\"bls\"}")
	HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
	BODY=$(echo "$RESPONSE" | sed '$d')
	if [ "$HTTP_CODE" -eq 200 ]; then \
		echo "$BODY" | jq; \
	else \
		echo "Error: HTTP $HTTP_CODE"; \
		echo "$BODY"; \
		exit 1; \
	fi

# ===============================
# Docker execution (with simulation environment)
# ===============================

# Run dockerized signer module
run-docker-signer ENV_FILE=".simulation.env" VERSION="dev":
	#!/usr/bin/env bash
	set -e
	if [ ! -f "{{ENV_FILE}}" ]; then \
		echo "Error: Environment file {{ENV_FILE}} not found"; \
		echo "Run 'just setup-simulation' first to generate .simulation.env"; \
		exit 1; \
	fi
	set -a
	source "{{ENV_FILE}}"
	set +a
	export VERSION={{VERSION}}
	echo "Starting dockerized signer (version: {{VERSION}}) with environment from {{ENV_FILE}}"
	docker compose up -d signer

# Run dockerized beacon mock
run-docker-beacon-mock ENV_FILE=".simulation.env" VERSION="dev":
	#!/usr/bin/env bash
	set -e
	if [ ! -f "{{ENV_FILE}}" ]; then \
		echo "Error: Environment file {{ENV_FILE}} not found"; \
		echo "Run 'just setup-simulation' first to generate .simulation.env"; \
		exit 1; \
	fi
	set -a
	source "{{ENV_FILE}}"
	set +a
	export VERSION={{VERSION}}
	# Extract beacon_api_url from simulation.config.toml
	export BEACON_API_URL=$(grep '^beacon_api_url' config/simulation.config.toml | cut -d'"' -f2)
	echo "Starting dockerized beacon mock (version: {{VERSION}}) with environment from {{ENV_FILE}}"
	echo "Beacon API URL: $BEACON_API_URL"
	echo "Proposer BLS Key: $PROPOSER_DEFAULT_BLS_KEY"
	docker compose up -d beacon-mock

# Run dockerized gateway module
run-docker-gateway ENV_FILE=".simulation.env" VERSION="dev":
	#!/usr/bin/env bash
	set -e
	if [ ! -f "{{ENV_FILE}}" ]; then \
		echo "Error: Environment file {{ENV_FILE}} not found"; \
		echo "Run 'just setup-simulation' first to generate .simulation.env"; \
		exit 1; \
	fi
	set -a
	source "{{ENV_FILE}}"
	set +a
	export VERSION={{VERSION}}
	echo "Starting dockerized gateway (version: {{VERSION}}) with environment from {{ENV_FILE}}"
	docker compose up -d gateway

# Run dockerized proposer daemon
run-docker-proposer ENV_FILE=".simulation.env" VERSION="dev":
	#!/usr/bin/env bash
	set -e
	if [ ! -f "{{ENV_FILE}}" ]; then \
		echo "Error: Environment file {{ENV_FILE}} not found"; \
		echo "Run 'just setup-simulation' first to generate .simulation.env"; \
		exit 1; \
	fi
	set -a
	source "{{ENV_FILE}}"
	set +a
	export VERSION={{VERSION}}
	echo "Starting dockerized proposer (version: {{VERSION}}) with environment from {{ENV_FILE}}"
	docker compose up -d proposer

# Run dockerized relay
run-docker-relay ENV_FILE=".simulation.env" VERSION="dev":
	#!/usr/bin/env bash
	set -e
	if [ ! -f "{{ENV_FILE}}" ]; then \
		echo "Error: Environment file {{ENV_FILE}} not found"; \
		echo "Run 'just setup-simulation' first to generate .simulation.env"; \
		exit 1; \
	fi
	set -a
	source "{{ENV_FILE}}"
	set +a
	export VERSION={{VERSION}}
	echo "Starting dockerized relay (version: {{VERSION}}) with environment from {{ENV_FILE}}"
	docker compose up -d relay

# Run dockerized spammer
run-docker-spammer ENV_FILE=".simulation.env" VERSION="dev":
	#!/usr/bin/env bash
	set -e
	if [ ! -f "{{ENV_FILE}}" ]; then \
		echo "Error: Environment file {{ENV_FILE}} not found"; \
		echo "Run 'just setup-simulation' first to generate .simulation.env"; \
		exit 1; \
	fi
	set -a
	source "{{ENV_FILE}}"
	set +a
	export VERSION={{VERSION}}
	echo "Starting dockerized spammer (version: {{VERSION}}) with environment from {{ENV_FILE}}"
	docker compose up -d spammer

# Run all dockerized services
run-docker-all ENV_FILE=".simulation.env" VERSION="dev":
	#!/usr/bin/env bash
	set -e
	if [ ! -f "{{ENV_FILE}}" ]; then \
		echo "Error: Environment file {{ENV_FILE}} not found"; \
		echo "Run 'just setup-simulation' first to generate .simulation.env"; \
		exit 1; \
	fi
	set -a
	source "{{ENV_FILE}}"
	set +a
	export VERSION={{VERSION}}
	echo "Starting all dockerized services (version: {{VERSION}}) with environment from {{ENV_FILE}}"
	docker compose up -d

# Setup simulation environment and run all dockerized services
setup-and-run ENV_FILE=".simulation.env" VERSION="dev":
	#!/usr/bin/env bash
	set -e
	echo "Setting up simulation environment..."
	just setup-simulation
	echo "Starting all dockerized services..."
	just run-docker-all "{{ENV_FILE}}" "{{VERSION}}"
