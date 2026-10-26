.PHONY: dev test lint install uninstall man help current-version release-patch release-minor release-major release-current check-git-clean bump-version git-push crates-publish package

EXECUTABLE_NAME := kav
INSTALL_PATH := /usr/local/bin

# Download directory / file prefix for `make package`, as releases name them.
ifeq ($(shell uname -s),Darwin)
  PACKAGE_PLATFORM := macos
else ifneq ($(filter aarch64 arm64,$(shell uname -m)),)
  PACKAGE_PLATFORM := linux-arm64
else
  PACKAGE_PLATFORM := linux
endif

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

dev: ## Run the application in development mode
	cargo run

test: ## Run tests
	cargo test --locked

lint: ## Run format check and clippy (tests included, as CI does)
	cargo fmt -- --check
	cargo clippy --locked --all-targets -- -D warnings

install: ## Build release binary and install to /usr/local/bin (may require sudo)
	cargo build --release
	sudo cp target/release/$(EXECUTABLE_NAME) $(INSTALL_PATH)/

uninstall: ## Remove binary from /usr/local/bin (may require sudo)
	sudo rm -f $(INSTALL_PATH)/$(EXECUTABLE_NAME)

man: ## Generate and install man page (may require sudo)
	cargo build --release
	sudo mkdir -p /usr/local/share/man/man1
	./target/release/$(EXECUTABLE_NAME) man | sudo tee /usr/local/share/man/man1/$(EXECUTABLE_NAME).1 > /dev/null
	@echo "\033[32mMan page installed at /usr/local/share/man/man1/$(EXECUTABLE_NAME).1\033[0m"

current-version: ## Output current version from Cargo.toml
	@sed -n 's/^version = "\([0-9.]*\)"/\1/p' Cargo.toml | head -n1

# Internal Release Helpers
check-git-clean:
	@if [ -n "$$(git status --porcelain --untracked-files=no)" ]; then \
		echo "\033[31mError: Git working directory is dirty. Please commit or stash changes first.\033[0m"; \
		exit 1; \
	fi

git-push:
	git push origin HEAD
	git push origin v$$(make current-version)

crates-publish:
	cargo publish

bump-version:
	@current=$$(make current-version); \
	major=$$(echo $$current | cut -d. -f1); \
	minor=$$(echo $$current | cut -d. -f2); \
	patch=$$(echo $$current | cut -d. -f3); \
	if [ "$(BUMP)" = "major" ]; then \
		major=$$((major + 1)); minor=0; patch=0; \
	elif [ "$(BUMP)" = "minor" ]; then \
		minor=$$((minor + 1)); patch=0; \
	elif [ "$(BUMP)" = "patch" ]; then \
		patch=$$((patch + 1)); \
	fi; \
	new_ver="$$major.$$minor.$$patch"; \
	echo "Bumping version: $$current -> $$new_ver"; \
	perl -i -pe "s/^version = \"$$current\"/version = \"$$new_ver\"/" Cargo.toml; \
	cargo check --quiet 2>/dev/null; \
	git commit -am "chore: bump version to $$new_ver"; \
	git tag "v$$new_ver"; \
	echo "\033[32mSuccessfully bumped to $$new_ver\033[0m"

# Public Release Commands
release-patch: check-git-clean ## Bump patch version (0.1.0 -> 0.1.1), push, and publish to crates.io
	cargo publish --dry-run
	@$(MAKE) bump-version BUMP=patch
	@$(MAKE) git-push
	@$(MAKE) crates-publish

release-minor: check-git-clean ## Bump minor version (0.1.x -> 0.2.0), push, and publish to crates.io
	cargo publish --dry-run
	@$(MAKE) bump-version BUMP=minor
	@$(MAKE) git-push
	@$(MAKE) crates-publish

release-major: check-git-clean ## Bump major version (0.x.x -> 1.0.0), push, and publish to crates.io
	cargo publish --dry-run
	@$(MAKE) bump-version BUMP=major
	@$(MAKE) git-push
	@$(MAKE) crates-publish

release-current: check-git-clean ## Tag the version already in Cargo.toml (no bump), push, and publish to crates.io
	cargo publish --dry-run
	git tag "v$$(make current-version)"
	@$(MAKE) git-push
	@$(MAKE) crates-publish

package: ## Build this machine's release tarball (kav, man page, licence, uninstaller) into target/dist
	cargo build --release --locked
	scripts/package.sh target/release/$(EXECUTABLE_NAME) $(PACKAGE_PLATFORM) v$$(make current-version) target/dist
	@echo "\033[32mCreated target/dist/$(PACKAGE_PLATFORM)/\033[0m"
