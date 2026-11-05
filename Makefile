# GNU Make 3.81+ (including the make shipped with macOS).
.DEFAULT_GOAL := help
SHELL := /bin/sh
.SUFFIXES:

NPM ?= npm
CARGO ?= cargo
PYTHON ?= python3
TARGET ?=
BUNDLES ?=
PROFILE ?= release
FRONTEND ?= build
SIGNED ?= false
RELEASE_TAG ?=
export TARGET BUNDLES PROFILE FRONTEND SIGNED RELEASE_TAG

.PHONY: help doctor setup dev dev-web build build-debug build-native build-signed \
 build-frontend build-assets build-review build-engine lint lint-metadata typecheck \
 format format-check clippy check test test-review test-engine test-native \
 test-updater test-installer test-performance test-tooling test-e2e protocol \
 release-check release-notes release-artifacts release-manifest notices clean

help: ## Show commands and build options (default)
	@awk 'BEGIN { print "mdiff development commands\n" } /^[a-z][a-z-]*:.*## / { split($$0, a, "## "); sub(/:.*/, "", a[1]); printf "  %-20s %s\n", a[1], a[2] }' Makefile
	@printf '\nBuild options: TARGET=<triple> BUNDLES=app|deb,appimage|none PROFILE=debug|release\n'
	@printf 'Use FRONTEND=prebuilt to reuse dist/. Release notes: RELEASE_TAG=vX.Y.Z\n'
	@printf 'Publishing stays in the tag-triggered GitHub release workflow.\n'

doctor: ## Check required local tools
	@set -eu; for tool in node npm cargo rustc python3 git shellcheck; do command -v "$$tool" >/dev/null || { printf 'Missing tool: %s\n' "$$tool" >&2; exit 1; }; done
	node --version
	$(CARGO) --version
	$(PYTHON) --version
	@printf 'Required versions and OS libraries: see README.md (Build).\n'

setup: ## Install both locked JavaScript dependency trees
	$(NPM) run setup

dev: ## Run the native app with live reload
	$(NPM) run tauri -- dev

dev-web: ## Run only the frontend development server
	$(NPM) run dev

build: ## Build desktop packages for the current host or TARGET
	@set -eu; set --; \
	case "$$PROFILE" in release) ;; debug) set -- "$$@" --debug ;; *) echo 'PROFILE must be debug or release' >&2; exit 1 ;; esac; \
	case "$$FRONTEND" in build) before='npm run build' ;; prebuilt) test -f dist/index.html || { echo 'Missing dist/: run make build-frontend' >&2; exit 1; }; before='' ;; *) echo 'FRONTEND must be build or prebuilt' >&2; exit 1 ;; esac; \
	case "$$SIGNED" in false) ;; true) test -n "$${TAURI_SIGNING_PRIVATE_KEY:-}" || { echo 'Missing TAURI_SIGNING_PRIVATE_KEY' >&2; exit 1; }; \
	$(PYTHON) -c 'import json; assert json.load(open("src-tauri/tauri.conf.json"))["plugins"]["updater"]["pubkey"].strip(), "Missing updater public key"' ;; *) echo 'SIGNED must be true or false' >&2; exit 1 ;; esac; \
	if [ -n "$$TARGET" ]; then set -- "$$@" --target "$$TARGET"; fi; \
	case "$$BUNDLES" in '') ;; none) set -- "$$@" --no-bundle ;; *) set -- "$$@" --bundles "$$BUNDLES" ;; esac; \
	config="{\"build\":{\"beforeBuildCommand\":\"$$before\"},\"bundle\":{\"createUpdaterArtifacts\":$$SIGNED}}"; \
	$(NPM) run tauri -- build "$$@" --config "$$config" -- --locked

build-debug: ## Build debug desktop packages
	$(MAKE) build PROFILE=debug

build-native: ## Compile native code using already-built frontend assets (CI)
	$(MAKE) build FRONTEND=prebuilt BUNDLES=none

build-signed: ## Package signed updater artifacts using signing environment variables
	$(MAKE) build SIGNED=true PROFILE=release

build-frontend: ## Typecheck and build the desktop frontend
	$(NPM) run build:desktop

build-assets: ## Build frontend assets after typechecking (CI)
	$(NPM) exec -- vite build

build-review: ## Build the embedded review assets needed by engine tests
	$(NPM) run build:review

build-engine: build-review ## Build the standalone engine used by browser integration tests
	$(CARGO) build --locked --release --manifest-path vendor/diffle/Cargo.toml

lint: ## Lint the review frontend
	$(NPM) run lint --prefix vendor/diffle

lint-metadata: ## Check installer syntax, shell lint, versions, and release notes
	bash -n install.sh
	shellcheck install.sh
	@set -eu; set --; if [ -n "$$RELEASE_TAG" ]; then set -- --tag "$$RELEASE_TAG"; fi; $(PYTHON) scripts/release-notes.py "$$@"

typecheck: ## Typecheck desktop, review, tooling, and browser tests
	$(NPM) exec -- tsc --noEmit
	$(NPM) run typecheck --prefix vendor/diffle

format: ## Format desktop, review, and Rust sources
	$(NPM) run format
	$(NPM) run format --prefix vendor/diffle
	$(CARGO) fmt --manifest-path src-tauri/Cargo.toml
	$(CARGO) fmt --manifest-path vendor/diffle/Cargo.toml

format-check: ## Check formatting without changing files
	$(NPM) exec -- prettier --check src package.json tsconfig.json src-tauri/tauri.conf.json
	$(NPM) run format:check --prefix vendor/diffle
	$(CARGO) fmt --manifest-path src-tauri/Cargo.toml -- --check
	$(CARGO) fmt --manifest-path vendor/diffle/Cargo.toml -- --check

clippy: ## Check Rust code and tests with warnings treated as errors
	$(CARGO) clippy --locked --manifest-path vendor/diffle/Cargo.toml --no-default-features --all-targets -- -D warnings
	$(CARGO) clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

check: ## Run lint, typechecks, metadata checks, Clippy, tests, then build assets
	$(MAKE) lint typecheck lint-metadata
	$(MAKE) clippy
	$(MAKE) test
	$(MAKE) build-assets

test: test-updater test-review test-engine test-native test-installer ## Run all local unit and integration suites (no browser/container required)

test-review: ## Run frontend unit and desktop performance tests
	$(NPM) run test:review

test-engine: build-review ## Run the complete Rust engine suite
	$(CARGO) test --locked --manifest-path vendor/diffle/Cargo.toml

test-native: ## Run native desktop integration and performance tests
	$(CARGO) test --locked --manifest-path src-tauri/Cargo.toml

test-updater: ## Test updater behavior and release manifests
	$(NPM) run test:updater
	$(PYTHON) -m unittest discover -s tests -p 'test_update_manifest.py'

test-installer: ## Test installation, upgrade, and rollback in temporary directories
	$(PYTHON) tests/test_installer.py

test-performance: ## Run focused frontend, engine, and native performance regressions
	$(NPM) run test:performance
	$(CARGO) test --locked --manifest-path vendor/diffle/Cargo.toml --no-default-features --test desktop_engine
	$(CARGO) test --locked --manifest-path src-tauri/Cargo.toml desktop_performance

test-tooling: ## Run the optional upstream tooling suite
	$(NPM) run test:tooling --prefix vendor/diffle

test-e2e: build-engine ## Run browser integration tests (requires Docker)
	$(NPM) run test:e2e --prefix vendor/diffle

protocol: ## Regenerate the shared Rust/TypeScript protocol schema
	cd vendor/diffle && $(NPM) exec -- vitest run test/scripts/protocol-schema.test.ts -u

release-check: ## Run the complete check pipeline against RELEASE_TAG (required)
	@test -n "$$RELEASE_TAG" || { echo 'Usage: make release-check RELEASE_TAG=vX.Y.Z' >&2; exit 1; }
	$(MAKE) check

release-notes: ## Write release-notes.txt from the changelog
	@set -eu; set --; if [ -n "$$RELEASE_TAG" ]; then set -- --tag "$$RELEASE_TAG"; fi; $(PYTHON) scripts/release-notes.py "$$@" --output release-notes.txt

notices: ## Refresh LICENCE.md from downloaded dependencies for TARGET
	@test -n "$$TARGET" || { echo 'TARGET is required' >&2; exit 1; }
	$(CARGO) fetch --locked --manifest-path src-tauri/Cargo.toml --target "$$TARGET"
	$(PYTHON) scripts/native-notices.py

release-artifacts: ## Collect signed release packages for TARGET
	@test -n "$$TARGET" || { echo 'TARGET is required' >&2; exit 1; }
	$(PYTHON) scripts/release-artifacts.py "$$TARGET"

release-manifest: release-notes ## Generate latest.json after collecting all signed platform artifacts
	$(PYTHON) scripts/update-manifest.py

clean: ## Remove generated builds, reports, and release artifacts; keep dependencies
	$(CARGO) clean --manifest-path src-tauri/Cargo.toml
	$(CARGO) clean --manifest-path vendor/diffle/Cargo.toml
	rm -rf dist vendor/diffle/crates/diffle/client release-artifacts release-notes.txt
	rm -rf vendor/diffle/test-results vendor/diffle/playwright-report vendor/diffle/coverage
