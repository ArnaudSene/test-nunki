#!/bin/sh
# The battery for a Rust project: what must be silent before anything
# leaves a slot (SPEC 4.4).
set -eu

cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
# Not `cargo deny check` alone: its `advisories` stage fetches
# its database from github.com, and the coder's allowlist names
# no forge (SPEC 4.1 bis, rule 6). That stage can never pass in
# this container — not for want of a network, but by design — so
# asking for it would hold gate 6 red for a reason no agent can
# repair. Run the advisories in CI, where the forge is reachable.
cargo deny check bans licenses sources
