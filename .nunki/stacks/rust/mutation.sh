#!/bin/sh
# The mutation campaign for a Rust project (SPEC 4.4, gate 7).
#
# `nunki` calls this as `mutation.sh <campaign-id> <path>...`, from the clean
# copy of HEAD inside the slot's container. The id comes first so the campaign
# is identifiable from its own command line; this script does not need it.
#
# It prints **one JSON object per line** on stdout, one per surviving mutant:
#   {"id":"…","file":"…","line":12,"description":"…"}
# `nunki` ignores anything that is not one, so progress may go to stdout freely —
# though this script keeps the tool's own chatter on stderr.
#
# `--in-place` is not a detail: cargo-mutants only reuses a build cache in
# place, and the copy this runs in has its own, warmed once per slot and kept.
# Without it every campaign recompiles from cold, and SPEC section 7 counts
# that hour.
set -eu

campaign="$1"
shift

# Only Rust sources are worth mutating; the touched list holds whatever the
# branch touched.
files=""
for path in "$@"; do
  case "$path" in
    *.rs) files="$files --file $path" ;;
  esac
done
if [ -z "$files" ]; then
  exit 0
fi

out="target/mutants-$campaign"
# The parent has to exist: `--output` creates its own directory and not the
# path above it, and a clean copy of HEAD that has never been built has no
# `target/` at all ("create output parent directory", measured).
mkdir -p "$out"

# A campaign that finds survivors exits non-zero — 2, measured on
# cargo-mutants 27.1.0 — and that is a result, not a failure: `nunki` reads the
# survivors rather than the status.
#
# `--exclude-re "replace main -> "` drops one survivor nobody could ever
# answer. cargo-mutants replaces a whole function body with
# `Default::default()` whenever the return type allows it, and
# `fn main() -> ExitCode` always allows it — but no unit test calls `main`,
# so that mutant cannot be killed by any test the coder is able to write.
# Left in, it costs a lot for nothing: measured on 2026-09-13, a coder spent
# a run extracting `main`'s body into a testable function, and the mutant
# simply reappeared on the thin wrapper that was left.
#
# Measured the same day, on a crate with a binary and a library: 19 mutants
# without it, 18 with it. It removes `src/main.rs`'s whole body and keeps
# `src/lib.rs`'s `replace run -> ExitCode` — the same shape, in the function
# `main` delegates to, and that one a test can and must kill. The exclusion
# is on the mutation, never on the file: a `main.rs` carrying real code still
# owes every mutant in it.
# shellcheck disable=SC2086
cargo mutants --in-place --no-shuffle --exclude-re "replace main -> " --output "$out" $files >&2 || true

# `--output DIR` writes into `DIR/mutants.out/`, not into `DIR` (measured on
# 27.1.0). Reading the wrong path was the whole campaign silently failing.
missed="$out/mutants.out/missed.txt"
if [ ! -f "$missed" ]; then
  echo "nunki: the campaign left no $missed" >&2
  exit 1
fi

# One mutant per line, as `file:line:col: what it replaced`:
#   src/lib.rs:2:7: replace > with == in keep
#
# **The whole line is the identifier**, and nothing shorter will do. Measured
# on 27.1.0: a single position carries several mutants — `> ==`, `> <` and
# `> >=` are all at `src/lib.rs:2:7` — so `file:line` and even
# `file:line:col` hand the coder survivors it cannot tell apart, in a file
# whose whole purpose is answering them one by one. The tool's own name for a
# mutant is that line, so that is the name nunki uses.
while IFS= read -r mutant; do
  [ -n "$mutant" ] || continue
  file=${mutant%%:*}
  rest=${mutant#*:}
  line=${rest%%:*}
  rest=${rest#*:}
  what=${rest#*: }
  escape() { printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'; }
  printf '{"id":"%s","file":"%s","line":%s,"description":"%s"}\n' \
    "$(escape "$mutant")" "$file" "$line" "$(escape "$what")"
done < "$missed"
