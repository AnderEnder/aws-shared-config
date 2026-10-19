# The AWS shared config and credentials file format

This document specifies the format of the AWS shared `config` and
`credentials` files as the AWS SDKs read them, and how `aws-shared-config`
models, reads, edits and (de)serializes them.

The AWS documentation describes the format informally. The precise rules
below come from the parser the AWS SDKs share. That parser is defined by a
cross-SDK test suite (`profile-parser-tests.json`, shipped with the SDKs) and
implemented in the AWS SDK for Rust by the `aws-runtime` crate.
`aws-shared-config` passes every case of that suite; see
`tests/conformance.rs`. Where the AWS CLI (botocore) reads files differently,
[section 9](#9-differences-in-the-aws-cli-and-boto3) says so.

**Contents**

1. [Files](#1-files)
2. [Lines](#2-lines)
3. [Section headers](#3-section-headers)
4. [Properties](#4-properties)
5. [Continuation lines](#5-continuation-lines)
6. [Sub-properties](#6-sub-properties)
7. [Combining the two files](#7-combining-the-two-files)
8. [Errors](#8-errors)
9. [Differences in the AWS CLI and boto3](#9-differences-in-the-aws-cli-and-boto3)
10. [The document model](#10-the-document-model)
11. [Reading](#11-reading)
12. [Editing](#12-editing)
13. [serde](#13-serde)
14. [Limitations](#14-limitations)
15. [Sources](#15-sources)

---

## 1. Files

There are two files. Both hold *profiles*: named sets of settings, one of
which an SDK or tool uses at a time.

| File        | Default location (Linux, macOS) | Default location (Windows)        | Override variable             |
|-------------|---------------------------------|-----------------------------------|-------------------------------|
| config      | `~/.aws/config`                 | `%UserProfile%\.aws\config`       | `AWS_CONFIG_FILE`             |
| credentials | `~/.aws/credentials`            | `%UserProfile%\.aws\credentials`  | `AWS_SHARED_CREDENTIALS_FILE` |

- The SDKs expand a leading `~` in the override variables to the home
  directory.
- A missing file is treated as empty.
- The files are UTF-8 text. AWS documents them as ASCII. Non-ASCII text is
  allowed in values and comments, but not in names (section 3.2).
- The profile in use is selected with `AWS_PROFILE`, or `default` when it is
  unset.

The credentials file is meant for secrets (`aws_access_key_id`,
`aws_secret_access_key`, `aws_session_token`) so that it can have stricter
permissions; everything else belongs in the config file. Either file may hold
any setting.

## 2. Lines

The format is line-based. Each line is classified on its own, by its first
character, and the classification never looks ahead.

### 2.1 Line terminators

Lines end at `\n`. A `\r` directly before the `\n` belongs to the terminator,
so `\r\n` files read the same as `\n` files. A `\r` anywhere else is an
ordinary character. The last line needs no terminator.

### 2.2 Whitespace

Only space (U+0020) and tab (U+0009) are whitespace. Trimming removes only
these two characters, and only they make a line blank or indented. Other
characters, such as a non-breaking space or a stray `\r`, are text.

One detail: inside a property value, a `#` or `;` starts a comment when the
character before it is whitespace in the Unicode sense (section 4.1). For
everything else, only space and tab count.

### 2.3 Line kinds

| Kind                 | Recognised by                    | Meaning                                                  |
|----------------------|----------------------------------|----------------------------------------------------------|
| Blank                | empty, or only spaces and tabs   | Ignored.                                                 |
| Comment              | first character is `#` or `;`    | Ignored.                                                 |
| Section header       | first character is `[`           | Starts a section (section 3).                            |
| Continuation         | first character is space or tab  | Continues the property above it (sections 5 and 6).      |
| Property             | any other first character        | A `key = value` setting (section 4).                     |

The checks apply in this order, so an indented line holding only spaces and
tabs is blank, not a continuation.

Consequences:

- A comment must start in the first column. An indented `# note` is a
  continuation line, not a comment (sections 5 and 6), and so is an indented
  header such as `  [profile dev]`.
- Blank and comment lines may appear anywhere, including between a property
  and its continuation lines, and do not end a property.

## 3. Section headers

```ini
[default]
[profile dev]                 # comment
[sso-session my-sso];comment
[services local-dynamodb]
```

### 3.1 Syntax

1. Surrounding whitespace is removed.
2. A comment is removed: everything from the first `#` or `;` to the end of
   the line. In headers, unlike values, the comment character need not follow
   whitespace, so `[profile dev]# note` is valid.
3. What remains must start with `[` and end with `]`. Otherwise the line is an
   error (*Profile definition must end with ']'*). Text after the `]` that is
   not a comment, as in `[profile dev] extra`, is therefore an error.
4. The text between the brackets is trimmed and split at its first space or
   tab into a *prefix* and a *name*, each trimmed again. Without whitespace
   there is no prefix, and the whole text is the name.

| Header                  | Prefix        | Name      |
|-------------------------|---------------|-----------|
| `[default]`             | none          | `default` |
| `[profile dev]`         | `profile`     | `dev`     |
| `[ profile   dev ]`     | `profile`     | `dev`     |
| `[sso-session corp]`    | `sso-session` | `corp`    |
| `[sso-sessioncorp]`     | none          | `sso-sessioncorp` |

A section runs from its header to the next header, or to the end of the file.

### 3.2 Names

Section names (and property keys, section 4) are *identifiers*: non-empty
strings of ASCII letters, digits and the characters `_ - / . % @ : +`.

AWS's documentation mentions only letters, digits, hyphens and underscores;
the SDKs accept the whole set. A section whose name is not an identifier
(`[profile foo!bar]`) is ignored, with a warning, not an error. Its
properties are ignored too, but are still checked for syntax errors.

### 3.3 Section types

The prefix gives the section's type. Which types are allowed depends on the
file.

| Header                 | In the config file                      | In the credentials file |
|------------------------|-----------------------------------------|-------------------------|
| `[default]`            | the `default` profile                   | the `default` profile   |
| `[name]`               | **ignored** (must be `[profile name]`)  | the profile `name`      |
| `[profile name]`       | the profile `name`                      | **ignored**             |
| `[profile default]`    | the `default` profile (see 3.4)         | **ignored**             |
| `[sso-session name]`   | IAM Identity Center session `name`      | **ignored**             |
| `[services name]`      | service-specific settings `name`        | **ignored**             |
| `[other name]`         | a section of type `other` (kept, unused by the SDKs today) | **ignored** |

The prefix must be followed by whitespace, so `[sso-sessioncorp]` is not an
`sso-session` section. In the credentials file it is a profile named
`sso-sessioncorp`; in the config file it is ignored.

`[sso-session name]` sections hold `sso_start_url`, `sso_region` and
`sso_registration_scopes`; a profile refers to one with
`sso_session = name`. `[services name]` sections hold one sub-property block
per service (section 6); a profile refers to one with `services = name`.

### 3.4 `[profile default]` and `[default]`

In the config file the default profile may be written either way. If the file
contains any `[profile default]` header, every `[default]` section *in that
file* is ignored. The order of the sections does not matter.

### 3.5 Repeated sections

A section may be declared more than once, as long as the type and name are the
same (`[profile dev]` and `[ profile  dev ]` count as the same). The SDKs merge
the declarations in order, so a key set in a later declaration overrides the
same key from an earlier one.

## 4. Properties

```ini
region = eu-west-1
output=json
Max_Attempts = 3        # comment
role_session_name = me#1
```

### 4.1 Syntax

1. Surrounding whitespace is removed.
2. A trailing comment is removed. A `#` or `;` starts a comment only when the
   character before it is whitespace. So `value # note` and `value ;note`
   end at `value`, while `me#1` and `a;b` are values. A comment character
   directly after `=` (`key =#x`) is part of the value, since `=` is not
   whitespace.
3. The remaining text is split at its *first* `=`. A line without `=` is an
   error (*Expected an '=' sign defining a property*).
4. The key and the value are trimmed. An empty key is an error (*Property did
   not have a name*). An empty value is allowed.

### 4.2 Keys

- Keys are case-insensitive. The SDKs lowercase them (ASCII only), so
  `Region` and `region` are the same key.
- A key that is not an identifier (section 3.2), such as `in valid = x`, is
  ignored with a warning, not an error.

### 4.3 Values

- A value is the raw text after `=`, trimmed. There is no quoting, escaping,
  or variable expansion. Quotes are literal characters.
- Any `=` after the first belongs to the value: `a = b=c` has the value `b=c`.
- A value cannot start or end with whitespace, and cannot contain a `#` or `;`
  that follows whitespace. The format has no way to write such values on a
  property line.

### 4.4 Placement and repetition

- A property before the first section header is an error (*Expected a profile
  definition*).
- If a key appears more than once in a section, including across repeated
  declarations of the section, the last value wins.

## 5. Continuation lines

```ini
[profile dev]
description = first line
    second line   # this is part of the value
```

A line starting with a space or tab continues the value of the closest
property above it in the same section. Blank and comment lines may come in
between.

- The continuation line is trimmed and appended to the value after a `\n`.
  The value above is `first line\nsecond line   # this is part of the value`,
  with the inner whitespace kept.
- Comments are **not** removed from continuation lines. Everything after the
  indentation is value text.
- A continuation line before any section header is an error (*Expected a
  profile definition*). One directly inside a section, before any property, is
  also an error (*Expected a property definition, found continuation*).

## 6. Sub-properties

```ini
[profile dev]
s3 =
  max_concurrent_requests = 20
  addressing_style = path

[services local]
dynamodb =
  endpoint_url = http://localhost:8000
```

A property whose value is empty (after removing any comment) opens a
*sub-property block*. Its continuation lines are sub-properties.

- Each sub-property line must be `key = value`. It is checked like a property
  line (section 4.1), so a sub-property without `=` is an error (*Expected an
  '=' sign defining a sub-property*), and so is one without a name
  (*Sub-property did not have a name*). An indented `# note` inside a block
  is therefore an error: after trimming, its `#` follows nothing, so it is not
  a comment, and the line has no `=`.
- Blank lines inside the block are ignored. Comment lines (column 0) are
  ignored and do not end the block.
- The parent property's value is the raw text: `\nmax_concurrent_requests =
  20\naddressing_style = path`. The sub-properties come from splitting that
  text into lines, splitting each line at its first `=`, and trimming both
  sides.
- Sub-property keys are **not** lowercased, and are not checked against the
  identifier rules: an invalid name is kept as it is.
- Comments are not removed from sub-property values, as with any continuation
  line. `  a = 1 # note` has the value `1 # note`.
- Blocks do not nest: a sub-property cannot have sub-properties.

AWS documents 2- and 4-space indentation in its examples. Any amount of
spaces or tabs works.

## 7. Combining the two files

An SDK loads the config file, then the credentials file:

- Profiles with the same name are merged, key by key. On a conflict the
  credentials file wins.
- The `[profile default]` rule (section 3.4) applies within the config file
  only. A `[default]` section in the credentials file is always merged.
- Only profiles come from the credentials file. `sso-session`, `services` and
  other section types are read from the config file only.

`aws-shared-config` keeps the two files as separate documents; a program that
needs the merged view combines them as above.

## 8. Errors

A file with any of these errors is rejected as a whole, by the SDKs and by
`Document::parse`. Everything not listed here, such as invalid names or
ignored sections, is accepted with a warning.

| Situation                                                     | Message                                              |
|---------------------------------------------------------------|------------------------------------------------------|
| Property or continuation line before the first header         | Expected a profile definition                        |
| Continuation line after a header but before any property      | Expected a property definition, found continuation   |
| Header without a closing `]` (after removing any comment)     | Profile definition must end with ']'                 |
| Property line without `=`                                     | Expected an '=' sign defining a property             |
| Property line with nothing before `=`                         | Property did not have a name                         |
| Sub-property line without `=`                                 | Expected an '=' sign defining a sub-property         |
| Sub-property line with nothing before `=`                     | Sub-property did not have a name                     |

[`ParseError`] carries the 1-based line number and a [`ParseErrorKind`]; its
message is `line N: <message>`, using the SDKs' wording.

## 9. Differences in the AWS CLI and boto3

The AWS CLI and boto3 read these files with botocore. botocore uses Python's
`configparser.RawConfigParser` with its default settings, and so differs from
the SDKs:

| Input                                 | AWS SDKs (and this crate)                    | AWS CLI, boto3 (botocore)                  |
|---------------------------------------|----------------------------------------------|--------------------------------------------|
| `region = us-east-1 # note`           | value `us-east-1`                            | value `us-east-1 # note`                   |
| `region: us-east-1`                   | error: no `=`                                | value `us-east-1`                          |
| a section declared twice              | merged, later values win                     | error (`DuplicateSectionError`)            |
| a key set twice in a section          | last value wins                              | error (`DuplicateOptionError`)             |
| `a = 1` then an indented `  # note`   | `# note` continues `a`: value `1\n# note`    | comment, ignored                           |

To write files every tool reads the same way:

- write `key = value`, with `=`, never `:`;
- put comments on their own lines, starting in the first column, never after
  a value;
- declare each section once and set each key once.

`aws-shared-config` follows these rules for every line it writes. It never adds
comments, and it keeps existing ones where they are.

## 10. The document model

[`Document::parse`] reads a file into a [`Document`]: the list of its lines,
each keeping its exact text (line terminator included) together with its
parsed meaning. `Document`'s `Display` prints the lines back unchanged, so:

```text
Document::parse(text, kind)?.to_string() == text    // for every valid text
```

A [`FileKind`] (`Config` or `Credentials`) goes with every document. It
decides which headers declare which sections (section 3.3), and how new
headers are written.

Sections are identified by [`SectionId`]: a [`SectionKind`] (`Profile`,
`SsoSession`, `Services` or `Other(prefix)`) and a name. The header spelling
does not matter: `SectionId::profile("dev")` finds `[profile dev]` in a config
file and `[dev]` in a credentials file, and `SectionId::profile("default")`
finds `[default]` or `[profile default]` with the precedence of section 3.4.

## 11. Reading

| API | Returns |
|-----|---------|
| `Document::profile(name)`, `sso_session(name)`, `section(&id)` | a [`Section`] view, or `None` if the file does not declare it |
| `Document::profile_names()` | profile names, in order of first appearance |
| `Document::section_ids()` | every section the SDKs read, in order of first appearance |
| `Section::get(key)` | the value of `key` (case-insensitive), or `None` |
| `Section::iter()` | every valid key with its value, in order of first appearance |
| `Section::sub_properties(key)` | the sub-properties of `key`, or `None` if `key` is missing or has a value of its own |
| `Section::get_sub(key, sub_key)` | one sub-property value (sub-keys are case-sensitive) |
| `Section::deserialize::<T>()` | the section as a `T` (section 13.1) |

Reading follows the SDKs exactly:

- repeated declarations of a section read as one, and the last value of a
  repeated key wins;
- ignored sections (section 3.3) and invalid keys (section 4.2) are invisible;
- a multi-line value reads as its lines joined with `\n`;
- a property with sub-properties reads, through `get`, as the raw block text
  starting with `\n`, exactly as the SDKs store it.

## 12. Editing

`Document::profile_mut(name)`, `sso_session_mut(name)` and
`section_mut(id)` return a [`SectionMut`]. The section does not need to exist;
it is created by the first edit that gives it a value.

### 12.1 Guarantees

- **Minimal change.** An edit rewrites only the lines it targets. All other
  bytes stay as they were: comments, blank lines, ordering, indentation, key
  spelling, spacing around `=`, and line terminators.
- **Validity.** After each edit the new text is parsed again. If it would not
  parse, the edit fails with [`EditError::WouldNotParse`] and the document is
  unchanged. Input that passes the checks in 12.7 never causes this.
- **Faithful values.** A value that would read back differently is rejected
  (12.7) rather than written.

### 12.2 `set(key, value)`

- **Existing key.** The value of the line that currently wins (the last
  occurrence, section 4.4) is replaced *in place*: the key keeps its original
  spelling, the spacing around `=` and the trailing comment are kept, and so
  is the line terminator. If the old value was empty, a space is put after
  `=`. Any continuation or sub-property lines of the old value are removed;
  comment and blank lines between them stay. Earlier occurrences of the key,
  which do not take effect, are left alone.
- **New key.** A line `key = value` goes directly after the last property (or
  continuation) line of the section's last declaration. If the section has no
  properties, it goes directly after the header. Comment and blank lines at
  the end of a section usually introduce the next section, so they stay below
  the new line.
- **New section.** The section is added at the end of the file: a blank line
  (unless the file is empty or already ends with a blank line), the header,
  then `key = value`. Headers are written as in section 3.3: `[default]`,
  `[profile name]`, `[sso-session name]` and `[services name]` in the config
  file, `[name]` in the credentials file. A credentials file can hold only
  profiles ([`EditError::NotInCredentialsFile`]).
- `set` returns the `SectionMut`, so calls chain:
  `doc.profile_mut("dev").set("region", "x")?.set("output", "json")?`.

### 12.3 `remove(key)`

Removes every line that sets `key` (case-insensitively) in the section's
active declarations, together with their continuation and sub-property lines.
Returns whether anything was removed. It never creates a section.

### 12.4 `set_sub(key, sub_key, value)` and `remove_sub(key, sub_key)`

- `set_sub` sets one sub-property of `key`. An existing sub-property line is
  replaced in place, keeping its indentation and spacing. A new sub-property
  line goes after the block's last line, indented like that line (two spaces
  if the block is empty). If `key` does not exist, `key =` and the
  sub-property are added as a new property (12.2). If `key` has a value of its
  own, the edit fails with [`EditError::NotSubProperties`].
- `remove_sub` removes the sub-property's lines, and the `key =` line too once
  the block is empty. It returns whether the sub-property existed.
- Sub-property keys are matched exactly (case-sensitively), as in the SDKs.

### 12.5 `Document::remove_section(&id)` and `remove_profile(name)`

Removes every declaration of the section, including a `[default]` that a
`[profile default]` overrides. Each header goes, together with its lines up
to the section's last property or continuation line. Comment and blank lines
after that stay, since they usually belong to the next section. One blank line
is removed when needed, so the neighbouring sections stay separated by a
single blank line. Returns whether the section existed.

### 12.6 Line terminators

New lines use the terminator of the first terminated line in the file: `\r\n`
if that line ends with `\r\n`, otherwise `\n`, which is also the default for
an empty file. If the file's last line has no terminator and a line is added
after it, that line gets one.

### 12.7 What is rejected

| Check | Rule | Error |
|-------|------|-------|
| Section names, keys, sub-property keys | must be identifiers (section 3.2) | [`EditError::InvalidName`] |
| Values | no `\n` or `\r` | [`EditError::InvalidValue`]: *values cannot span lines; use sub-properties* |
| Values | no leading or trailing space or tab | *leading and trailing whitespace is not kept* |
| Values | must not start with `#` or `;` | *a value cannot start with `#` or `;`* |
| Values | no `#` or `;` directly after whitespace | *`#` or `;` after whitespace starts a comment* |

`#` and `;` inside a word, as in `a#b;c`, are fine. An empty value is
allowed.

## 13. serde

The default `serde` feature adds `Section::deserialize` and
`SectionMut::serialize`. Disable default features to build without serde.

### 13.1 Reading: `Section::deserialize::<T>()`

The section is presented to serde as a map from keys to values. `T` is
usually a struct with one field per key, but any map type works too.

| Rust type | Accepted text |
|-----------|---------------|
| `String`, `&str` (borrowed from the document), `Cow<str>` | any value |
| `bool` | `true` or `false`, in any letter case |
| integers, floats | text the type's `FromStr` accepts |
| `char` | a single character |
| `Option<T>` | `Some` when the key is present, `None` when it is missing |
| unit enum variants | the variant name, e.g. `retry_mode = standard`, using `#[serde(rename_all = "lowercase")]` as needed |
| a struct or map field | the key's sub-properties (section 6) |
| `()` | an empty value |
| lists, tuples | not supported (the format has no lists) |

- Keys are lowercase (section 4.2), so field names must be lowercase too, or
  renamed with `#[serde(rename = "...")]`.
- Keys that `T` does not name are ignored, unless `T` has
  `#[serde(deny_unknown_fields)]`.
- A missing required field fails with serde's *missing field `name`*. A value
  that does not parse fails with a message naming the key, e.g.
  ``` `region`: expected a non-negative integer, found `eu-west-1` ```.

### 13.2 Writing: `SectionMut::serialize(&value)`

The value must serialize as a struct or map. Each field becomes an edit, and
the edits are *merged* into the section:

| Field value | Edit |
|-------------|------|
| string, number, `bool`, `char` | `set(key, text)`: replaced in place, keeping the comment (12.2) |
| `None` | `remove(key)` |
| unit enum variant | `set(key, variant name)` |
| newtype struct | as its inner value |
| struct or map | its fields become `set_sub`, or `remove_sub` for `None` |
| field skipped by `#[serde(skip_serializing_if = ...)]` | nothing: the key is left as it is |
| list, tuple, non-unit enum variant, nested sub-properties | rejected |

- Keys the value does not mention are left alone, as are all comments and
  blank lines. To delete a key, give it a field with the value `None`.
- Fields are applied in serialization order, so new keys are added in that
  order.
- The update is all-or-nothing: if any field is rejected (12.7, or a shape the
  format cannot hold), the document is left unchanged.
- Numbers and booleans are written with `Display` (`true`, `5`, `0.5`).

## 14. Limitations

- **No file I/O.** Reading, `~` expansion, and atomic writing are left to the
  caller, as is merging the two files (section 7).
- **Comments are not data.** They are kept and moved with their lines, but
  cannot be read or added through the API.
- **No moving or renaming.** Sections and keys can be added and removed, not
  reordered or renamed.
- **One level of sub-properties**, as in the format itself.
- **Strict parsing.** Files the SDKs reject are rejected too (section 8), even
  ones the AWS CLI accepts (section 9).

## 15. Sources

- AWS SDKs and Tools Reference Guide, *Shared config and credentials files*:
  <https://docs.aws.amazon.com/sdkref/latest/guide/file-format.html>
- AWS SDK for Rust (smithy-rs), `aws-runtime` crate:
  `src/env_config/parse.rs` (line parsing), `src/env_config/normalize.rs`
  (section types, names, merging), and `test-data/profile-parser-tests.json`
  (the shared test suite): <https://github.com/smithy-lang/smithy-rs>
- botocore, `botocore/configloader.py` (`raw_config_parse`, `_parse_nested`):
  <https://github.com/boto/botocore>
- Python, `configparser` defaults:
  <https://docs.python.org/3/library/configparser.html>

[`Document`]: crate::Document
[`Document::parse`]: crate::Document::parse
[`FileKind`]: crate::FileKind
[`Section`]: crate::Section
[`SectionId`]: crate::SectionId
[`SectionKind`]: crate::SectionKind
[`SectionMut`]: crate::SectionMut
[`ParseError`]: crate::ParseError
[`ParseErrorKind`]: crate::ParseErrorKind
[`EditError::InvalidName`]: crate::EditError::InvalidName
[`EditError::InvalidValue`]: crate::EditError::InvalidValue
[`EditError::NotSubProperties`]: crate::EditError::NotSubProperties
[`EditError::NotInCredentialsFile`]: crate::EditError::NotInCredentialsFile
[`EditError::WouldNotParse`]: crate::EditError::WouldNotParse
