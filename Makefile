.PHONY: prepare check test build update

CARGO_FLAGS := --release --locked

prepare:
	cargo +nightly fmt
	cargo clippy $(CARGO_FLAGS) --fix --all-targets --allow-dirty -- -D warnings
	cargo check $(CARGO_FLAGS)

check:
	cargo +nightly fmt --check
	cargo clippy $(CARGO_FLAGS) --all-targets -- -D warnings
	cargo check $(CARGO_FLAGS) --bin macmon
	cargo check $(CARGO_FLAGS) --lib --no-default-features

test:
	cargo test $(CARGO_FLAGS)
	sh scripts/test-upstream-sync.sh
	$(MAKE) test-dashboard-control

build:
	cargo build $(CARGO_FLAGS)
	ls -lh target/release/$(shell basename $(CURDIR))

update:
	cargo upgrade -i

.PHONY: loc bench remote upstream-sync test-upstream-sync test-dashboard test-dashboard-control

test-dashboard-control:
	sh scripts/test-dashboard-control.sh

test-dashboard:
	node scripts/test-dashboard.mjs

upstream-sync:
	@sh scripts/upstream-sync.sh

test-upstream-sync:
	@sh scripts/test-upstream-sync.sh

loc: # production lines, tests excluded; REF=origin/main compares with a git ref
	@python3 scripts/loc.py $(REF)

bench: # compare startup time
	cargo build $(CARGO_FLAGS)
	hyperfine --warmup 3 --min-runs 60 \
		'macmon pipe -s 1 -i 100' \
		'./target/release/macmon pipe -s 1 -i 100'

remote:
	@test -n "$(host)" || (echo "Usage: make remote host=user@host" >&2; exit 1)
	@rsync -az Cargo.toml Cargo.lock Makefile src "$(host):macmon/"
	@ssh "$(host)" 'cd ~/macmon && cargo build $(CARGO_FLAGS) && ./target/release/macmon debug'
	@ssh "$(host)" 'cd ~/macmon && ./target/release/macmon pipe -s 1 -i 100 > /dev/null'
