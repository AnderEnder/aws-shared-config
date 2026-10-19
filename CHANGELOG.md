# Changelog

All notable changes to this crate are recorded here.

## 0.1.0 - 2026-10-19

The first release: a lossless parser and editor for the AWS shared `config` and
`credentials` files.

- `Document::parse` reads a file with the rules the AWS SDKs share, and prints
  it back byte-for-byte. All 65 cases of the SDKs' profile parser test suite
  pass.
- Reading: profiles, `sso-session` and `services` sections, case-insensitive
  keys, continuation lines and sub-properties, with `[profile default]`
  overriding `[default]` and later values winning.
- Editing: `set`, `remove`, `set_sub`, `remove_sub` and `remove_section` change
  only the lines they target, keeping comments, spacing, key spelling and line
  endings. Values that would read back differently are rejected, and every edit
  is re-parsed before it is accepted.
- serde (default feature): `Section::deserialize` reads a section into a Rust
  type, and `SectionMut::serialize` writes one back in place, leaving the keys
  and comments it does not mention.
- `SPEC.md`, also the `spec` module: the file format, and how this crate reads,
  edits and (de)serializes it.
