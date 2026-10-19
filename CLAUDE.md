# CLAUDE.md

Guidance for Claude Code in this repository.

## What this is

`aws-shared-config` is a Rust library that parses and edits the AWS shared `config` and
`credentials` files losslessly: a parsed file prints back byte-for-byte, and edits change only the
lines they target, so comments and formatting survive. Reading follows the rules the AWS SDKs
share; serde support (the default `serde` feature) maps sections to Rust types.

`SPEC.md` is the authority on behaviour: the file format, the reading rules, every editing rule,
and the serde mapping. It is also published as the `spec` module. Change it together with the
code.

## Commands

```sh
scripts/ci.sh                # every check CI runs: fmt, clippy (both feature sets), tests, docs, package
scripts/ci.sh msrv           # the MSRV checks; run with the 1.85 toolchain
scripts/ci.sh release v0.1.0 # the release check: tag, Cargo.toml version and CHANGELOG.md section
cargo test --test conformance  # the AWS SDKs' 65-case parser suite
```

## Layout

- `src/parse.rs`: line classification and the SDK parsing rules (`#`/`;` comments, continuations,
  sub-properties, identifiers). Keep it identical in behaviour to the SDKs.
- `src/document.rs`: `Document` (the lines, each with its raw text), section lookup
  (`SectionId`, `[profile default]` precedence), the `Section` view and `SectionMut` edits.
- `src/de.rs`, `src/ser.rs`: serde. `serialize` merges into a section and is all-or-nothing.
- `src/error.rs`: `ParseError` (messages match the SDKs'), `EditError`, `SerdeError`.
- `tests/conformance.rs`: runs `tests/data/profile-parser-tests.json`, vendored unchanged from
  the AWS SDK for Rust (Apache-2.0, not published with the crate).

## Invariants

- `Document::parse(text)?.to_string() == text` for every valid text.
- Edits rewrite raw lines, then re-parse the whole text (`Document::apply`); an edit whose result
  would not parse fails with `EditError::WouldNotParse` and leaves the document unchanged.
- Never write a value that would read back differently: `check_value` rejects them.
- The crate does no file I/O and has no dependencies besides optional serde.
- `rust-version` stays at or below what aws-mfa-session, its main user, can build with.
