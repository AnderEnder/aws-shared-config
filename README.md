# aws-shared-config

A lossless parser and editor for the AWS shared `config` and `credentials`
files (`~/.aws/config`, `~/.aws/credentials`): read and change profiles
without losing comments or formatting.

- **Lossless.** A parsed file prints back byte-for-byte. Edits change only the
  lines they must; comments, blank lines, ordering, key spelling, spacing and
  line endings elsewhere stay as they were.
- **Same rules as the AWS SDKs.** Comments, continuation lines,
  sub-properties, `[profile name]` prefixes, `[profile default]` precedence and
  name validation all follow the parser the AWS SDKs share. The crate passes
  all 65 cases of the SDKs' own conformance suite.
- **Safe edits.** Values that would read back differently, such as one
  containing ` #`, are rejected instead of being written incorrectly. Every edit
  is re-parsed before it is accepted.
- **serde.** Read a profile into a struct; write a struct into a profile, in
  place, keeping the comments and the keys the struct does not mention.

The full format specification and every editing rule are in
[`SPEC.md`](https://github.com/AnderEnder/aws-shared-config/blob/main/SPEC.md),
also published as the [`spec`] module.

## Reading

```rust
use aws_shared_config::{Document, FileKind};

let text = "\
[default]
region = us-east-1 # closest to the office

[profile dev]
region = eu-west-1
s3 =
  max_concurrent_requests = 20
";
let doc = Document::parse(text, FileKind::Config)?;

let dev = doc.profile("dev").unwrap();
assert_eq!(dev.get("region"), Some("eu-west-1"));
assert_eq!(dev.get_sub("s3", "max_concurrent_requests"), Some("20"));
assert_eq!(doc.profile("default").unwrap().get("Region"), Some("us-east-1"));
assert_eq!(doc.profile_names(), ["default", "dev"]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The config file names profiles `[profile name]` (or `[default]`), and the
credentials file names them `[name]`. A [`Document`] knows which file it holds,
so profiles are always looked up by plain name.

## Editing

```rust
use aws_shared_config::{Document, FileKind};

let text = concat!(
    "# Long-term keys\n",
    "[default]\n",
    "aws_access_key_id = AKIAIOSFODNN7EXAMPLE   # rotated 2026-09\n",
    "aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\n",
);
let mut doc = Document::parse(text, FileKind::Credentials)?;

doc.profile_mut("default").set("aws_access_key_id", "AKIAI44QH8DHBEXAMPLE")?;
doc.profile_mut("session")
    .set("aws_access_key_id", "ASIAEXAMPLE")?
    .set("aws_session_token", "IQoJb3JpZ2luX2VjEXAMPLE")?;

assert_eq!(doc.to_string(), concat!(
    "# Long-term keys\n",
    "[default]\n",
    "aws_access_key_id = AKIAI44QH8DHBEXAMPLE   # rotated 2026-09\n",
    "aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\n",
    "\n",
    "[session]\n",
    "aws_access_key_id = ASIAEXAMPLE\n",
    "aws_session_token = IQoJb3JpZ2luX2VjEXAMPLE\n",
));
# Ok::<(), Box<dyn std::error::Error>>(())
```

`set` replaces a value where it is, keeping the line's comment. A new key
goes after the section's last property, and a new section goes at the end of
the file. `remove`, `set_sub`, `remove_sub` and `Document::remove_section`
complete the set. A section is only created once it is given a value.

The crate does no file I/O: read the file, parse, edit, and write the result
back however suits the program, ideally atomically (write a temporary file
next to it, then rename).

## serde

With the default `serde` feature, a section reads into any
`Deserialize` type and is updated from any `Serialize` type:

```rust
use aws_shared_config::{Document, FileKind};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
struct Session {
    aws_access_key_id: String,
    aws_secret_access_key: String,
    aws_session_token: String,
    expiration: Option<String>,
}

let text = concat!(
    "[session]\n",
    "# Written by my-tool; do not edit.\n",
    "aws_access_key_id = ASIAOLD\n",
    "aws_secret_access_key = old\n",
    "aws_session_token = old\n",
    "expiration = 2026-10-18T15:00:00Z\n",
    "region = eu-west-1\n",
);
let mut doc = Document::parse(text, FileKind::Credentials)?;

let old: Session = doc.profile("session").unwrap().deserialize()?;
assert_eq!(old.expiration.as_deref(), Some("2026-10-18T15:00:00Z"));

doc.profile_mut("session").serialize(&Session {
    aws_access_key_id: "ASIANEW".into(),
    aws_secret_access_key: "new".into(),
    aws_session_token: "new".into(),
    expiration: None, // `None` removes the key
})?;

// The comment and `region`, which `Session` does not mention, stay.
assert_eq!(doc.to_string(), concat!(
    "[session]\n",
    "# Written by my-tool; do not edit.\n",
    "aws_access_key_id = ASIANEW\n",
    "aws_secret_access_key = new\n",
    "aws_session_token = new\n",
    "region = eu-west-1\n",
));
# Ok::<(), Box<dyn std::error::Error>>(())
```

Values are text: numbers and booleans are parsed when a field asks for them,
unit enum variants are written by name, and a struct or map field maps to
sub-properties. Lists have no representation in the format and are rejected.

## Minimum supported Rust version

Rust 1.85.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

The repository also holds the AWS SDKs' profile parser test suite under `tests/data/`. It is
covered by the Apache License, Version 2.0, [tests/data/LICENSE-APACHE](tests/data/LICENSE-APACHE),
and is not part of the published package.

[`spec`]: https://docs.rs/aws-shared-config/latest/aws_shared_config/spec/index.html
[`Document`]: https://docs.rs/aws-shared-config/latest/aws_shared_config/struct.Document.html
