#!/bin/sh
# The integrator's battery for a Rust project: its system tests, green, in the
# system profile (SPEC 4.4, gate 6 for the integrator).
#
# `nunki` runs this from the clean copy of HEAD inside the integrator's
# container, where the mission's services are reachable. The coder's battery
# (`prepush.sh`) runs where they are not, and so does CI: a system test must
# therefore never run under a plain `cargo test`, or it goes red in both.
#
# The convention that keeps them apart: every system test carries
#
#     #[ignore = "system test: needs the services"]
#
# A plain `cargo test` skips it and says so; `-- --ignored` below runs those
# tests and only those. Ignore nothing else: an ignored test that is not a
# system test would run here and nowhere else.
#
# `--tests` leaves the doc-tests out: an `ignore` code block in a doc comment
# is usually one that does not compile, and `--ignored` would try it.
#
# Pointing the tests at the services is the integrator's wiring, committed with
# them. Amend this script too if the project's system tests need more than
# this — with its path in the mission's wiring list, or gate 4 refuses it.
set -eu

# Beside the build rather than under /tmp: `target/` is what the copy of HEAD
# is certain to let an execution write.
mkdir -p target
log=target/nunki-system-tests.log

status=0
cargo test --all-features --tests -- --ignored >"$log" 2>&1 || status=$?
cat "$log" >&2
if [ "$status" -ne 0 ]; then
  exit "$status"
fi

# A battery that ran nothing proved nothing (SPEC 4.4): no system test is red,
# not green.
ran=$(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$log" | awk '{ n += $1 } END { print n + 0 }')
if [ "$ran" -eq 0 ]; then
  echo "nunki: no system test ran — mark each one #[ignore = \"system test: needs the services\"]" >&2
  exit 1
fi
