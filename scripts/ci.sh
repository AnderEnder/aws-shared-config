#!/bin/sh
# Runs locally what CI runs. The workflows in .github/workflows/ run every check through this
# script, so a green run here means a green run there, for the same toolchain. Only publishing
# and uploads are theirs alone.
#
#   scripts/ci.sh              the checks run on every push and pull request
#   scripts/ci.sh msrv         the checks that fit the minimum supported Rust version,
#                              rust-version in Cargo.toml; run with that toolchain
#   scripts/ci.sh coverage     the tests under cargo-llvm-cov; writes the report to
#                              target/coverage/ (see coverage below)
#   scripts/ci.sh release TAG  the release check: fails unless TAG is vX.Y.Z for the version in
#                              Cargo.toml and CHANGELOG.md has a section for it, which it writes
#                              to target/release-notes.md
#
# The active toolchain is used.
set -eu

cd "$(dirname "$0")/.."

step() {
	printf '\n==> %s\n' "$*" >&2
}

run() {
	step "$*"
	"$@"
}

checks() {
	run cargo fmt --all --check

	run cargo clippy --locked --all-targets --all-features -- -D warnings
	# Without the default `serde` feature, the serde tests are compiled out.
	run cargo clippy --locked --all-targets --no-default-features -- -D warnings

	# Unit, integration (including the AWS SDKs' parser suite) and doc tests.
	run cargo test --locked
	# The README's examples use serde, so the doctests need the default feature.
	run cargo test --locked --no-default-features --lib --tests

	step "RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features"
	RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features

	run cargo package --locked --allow-dirty --quiet
}

# The minimum supported Rust version: everything builds, and the tests pass.
msrv() {
	run cargo test --locked
	run cargo test --locked --no-default-features --lib --tests
}

# The coverage report, `scripts/ci.sh coverage`: runs the tests under cargo-llvm-cov
# (`cargo install cargo-llvm-cov` and `rustup component add llvm-tools`) and writes to
# $COVERAGE_OUT:
#
#   summary.txt   the table of covered regions, functions and lines per file
#   lcov.info     the report in LCOV format, for Codecov and editors
#   html/         the report as HTML
#
# The report covers the library, src/. The doctests are not run, since cargo-llvm-cov needs
# nightly Rust for them.
COVERAGE_OUT=target/coverage
COVERAGE_IGNORE='/tests/'

coverage() {
	rm -rf "$COVERAGE_OUT"
	mkdir -p "$COVERAGE_OUT"
	run cargo llvm-cov clean --workspace
	run cargo llvm-cov --no-report
	run cargo llvm-cov report --ignore-filename-regex "$COVERAGE_IGNORE" \
		--lcov --output-path "$COVERAGE_OUT/lcov.info"
	run cargo llvm-cov report --ignore-filename-regex "$COVERAGE_IGNORE" \
		--html --output-dir "$COVERAGE_OUT"
	step "cargo llvm-cov report --summary-only > $COVERAGE_OUT/summary.txt"
	cargo llvm-cov report --ignore-filename-regex "$COVERAGE_IGNORE" --summary-only \
		>"$COVERAGE_OUT/summary.txt"
	cat "$COVERAGE_OUT/summary.txt"
}

# The release check, `scripts/ci.sh release TAG`, which .github/workflows/release.yml runs before
# anything is published: fails unless TAG is vX.Y.Z, X.Y.Z is the version in Cargo.toml, and
# CHANGELOG.md has a section for it, a heading `## X.Y.Z` or `## X.Y.Z - DATE`. The section,
# without its heading, goes to $RELEASE_NOTES, the notes of the GitHub release.
RELEASE_NOTES=target/release-notes.md

release() {
	tag=${1:-}
	version=${tag#v}
	if [ "$version" = "$tag" ] ||
		! printf '%s\n' "$version" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'; then
		echo "usage: scripts/ci.sh release TAG, where TAG is vX.Y.Z" >&2
		exit 2
	fi
	# `path+file:///…/aws-shared-config#0.1.0`, or `…#aws-shared-config@0.1.0` in a directory
	# of another name.
	pkgid=$(cargo pkgid)
	manifest=${pkgid##*[#@]}
	if [ "$version" != "$manifest" ]; then
		echo "error: tag $tag does not match the version in Cargo.toml, $manifest" >&2
		exit 1
	fi
	mkdir -p "$(dirname "$RELEASE_NOTES")"
	# The lines after the version's heading up to the next `## ` heading, without the blank lines
	# at either end.
	awk -v version="$version" '
		/^## / {
			if (found) exit
			if ($2 == version && (NF == 2 || $3 == "-")) found = 1
			next
		}
		found && $0 == "" { blank++; next }
		found {
			if (printed) for (i = 0; i < blank; i++) print ""
			blank = 0
			printed = 1
			print
		}
	' CHANGELOG.md >"$RELEASE_NOTES"
	if [ ! -s "$RELEASE_NOTES" ]; then
		echo "error: CHANGELOG.md has no section for $version (a heading \`## $version\`)" >&2
		exit 1
	fi
	echo "tag $tag matches Cargo.toml; the notes of $version are in $RELEASE_NOTES" >&2
}

case "${1:-checks}" in
checks) checks ;;
msrv) msrv ;;
coverage) coverage ;;
release)
	shift
	release "$@"
	;;
*)
	echo "usage: scripts/ci.sh [checks|msrv|coverage|release TAG]" >&2
	exit 2
	;;
esac
