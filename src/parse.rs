//! Line-level parsing that follows the rules the AWS SDKs share for profile
//! files (see `tests/data/profile-parser-tests.json`).
//!
//! Every line keeps its exact text, so a parsed document prints back
//! byte-for-byte; the parsed meaning sits alongside it.

use std::ops::Range;

use crate::error::{ParseError, ParseErrorKind};

/// Only spaces and tabs count as whitespace in profile files.
pub(crate) const WHITESPACE: &[char] = &[' ', '\t'];
const COMMENT: &[char] = &['#', ';'];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Line {
    /// The exact text of the line, including its line terminator, if any.
    pub raw: String,
    pub kind: LineKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LineKind {
    /// Empty, or only spaces and tabs.
    Blank,
    /// Starts with `#` or `;`.
    Comment,
    /// `[name]` or `[prefix name]`.
    Header(Header),
    /// `key = value`, starting in the first column.
    Property(Property),
    /// An indented line continuing the property above it.
    Continuation,
}

/// A section header, split into prefix and name the way the SDKs do:
/// `[profile dev]` has prefix `profile` and name `dev`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub prefix: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Property {
    /// Lowercased: the SDKs compare keys case-insensitively.
    pub key: String,
    /// The value, with any continuation lines appended after `\n`.
    pub value: String,
    /// Byte range of the first line's value within the raw line.
    pub value_span: Range<usize>,
}

/// The text of `raw` without its line terminator. Like `str::lines`, a `\r`
/// is only part of the terminator when it is followed by `\n`.
pub(crate) fn content(raw: &str) -> &str {
    match raw.strip_suffix('\n') {
        Some(line) => line.strip_suffix('\r').unwrap_or(line),
        None => raw,
    }
}

pub(crate) fn is_blank(content: &str) -> bool {
    content.trim_matches(WHITESPACE).is_empty()
}

pub(crate) fn parse_lines(text: &str) -> Result<Vec<Line>, ParseError> {
    let mut lines: Vec<Line> = Vec::new();
    // `None` before the first header. Inside a section, the index of the
    // property that indented lines continue, and whether its first line was
    // empty, which makes the indented lines sub-properties.
    let mut section: Option<Option<(usize, bool)>> = None;

    for (index, raw) in text.split_inclusive('\n').enumerate() {
        let line = content(raw);
        let error = |kind| ParseError::new(index + 1, kind);

        let kind = if is_blank(line) {
            LineKind::Blank
        } else if line.starts_with(COMMENT) {
            LineKind::Comment
        } else if line.starts_with('[') {
            section = Some(None);
            LineKind::Header(parse_header(line).map_err(error)?)
        } else if line.starts_with(WHITESPACE) {
            let Some(current) = section else {
                return Err(error(ParseErrorKind::NoSection));
            };
            let Some((owner, sub_properties)) = current else {
                return Err(error(ParseErrorKind::ContinuationWithoutProperty));
            };
            if sub_properties {
                parse_key_value(line, true).map_err(error)?;
            }
            if let LineKind::Property(property) = &mut lines[owner].kind {
                property.value.push('\n');
                property.value.push_str(line.trim_matches(WHITESPACE));
            }
            LineKind::Continuation
        } else {
            if section.is_none() {
                return Err(error(ParseErrorKind::NoSection));
            }
            let property = parse_key_value(line, false).map_err(error)?;
            section = Some(Some((lines.len(), property.value.is_empty())));
            LineKind::Property(property)
        };

        lines.push(Line {
            raw: raw.to_owned(),
            kind,
        });
    }
    Ok(lines)
}

fn parse_header(line: &str) -> Result<Header, ParseErrorKind> {
    // Comments may follow the closing bracket directly: `[profile dev]# note`.
    let significant = &line[significant_range(line, false)];
    let inner = significant
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or(ParseErrorKind::UnclosedHeader)?
        .trim_matches(WHITESPACE);
    Ok(match inner.split_once(WHITESPACE) {
        Some((prefix, name)) => Header {
            prefix: Some(prefix.trim().to_owned()),
            name: name.trim().to_owned(),
        },
        None => Header {
            prefix: None,
            name: inner.trim().to_owned(),
        },
    })
}

/// Parse `key = value`. In values, `#` and `;` only start a comment after
/// whitespace, so `value#1` keeps its `#`.
fn parse_key_value(line: &str, sub_property: bool) -> Result<Property, ParseErrorKind> {
    let range = significant_range(line, true);
    let significant = &line[range.clone()];
    let equals = significant
        .find('=')
        .ok_or(ParseErrorKind::MissingEquals { sub_property })?;
    let key = significant[..equals].trim_matches(WHITESPACE);
    if key.is_empty() {
        return Err(ParseErrorKind::EmptyKey { sub_property });
    }
    let after_equals = &significant[equals + 1..];
    let value = after_equals.trim_matches(WHITESPACE);
    let start = range.start
        + equals
        + 1
        + (after_equals.len() - after_equals.trim_start_matches(WHITESPACE).len());
    Ok(Property {
        key: key.to_ascii_lowercase(),
        value: value.to_owned(),
        value_span: start..start + value.len(),
    })
}

/// The part of `line` that carries meaning: without surrounding whitespace
/// and without a trailing comment.
fn significant_range(line: &str, comments_need_whitespace: bool) -> Range<usize> {
    let start = line.len() - line.trim_start_matches(WHITESPACE).len();
    let trimmed = line[start..].trim_end_matches(WHITESPACE);

    let mut previous_whitespace = false;
    let mut end = trimmed.len();
    for (index, c) in trimmed.char_indices() {
        if COMMENT.contains(&c) && (previous_whitespace || !comments_need_whitespace) {
            end = index;
            break;
        }
        previous_whitespace = c.is_whitespace();
    }
    start..start + trimmed[..end].trim_end_matches(WHITESPACE).len()
}

/// Split sub-property lines (a property's value after its empty first line)
/// into key-value pairs, as the SDKs do.
pub(crate) fn sub_properties(value: &str) -> impl Iterator<Item = (&str, &str)> {
    value
        .split('\n')
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim_matches(WHITESPACE), value.trim_matches(WHITESPACE)))
}

/// Whether the SDKs accept `name` as a profile, section, or key name.
pub fn is_valid_identifier(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || "_-/.%@:+".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<LineKind> {
        parse_lines(text)
            .unwrap()
            .into_iter()
            .map(|line| line.kind)
            .collect()
    }

    fn property(line: &str) -> Property {
        parse_key_value(line, false).unwrap()
    }

    #[test]
    fn test_raw_lines_round_trip() {
        let text = "# c\r\n[default]\r\nregion = eu-west-1 # c\n\n  \t\n[profile a] ; x";
        let lines = parse_lines(text).unwrap();
        let printed: String = lines.iter().map(|line| line.raw.as_str()).collect();
        assert_eq!(printed, text);
        assert_eq!(lines.len(), 6);
    }

    #[test]
    fn test_line_kinds() {
        let header = |prefix: Option<&str>, name: &str| {
            LineKind::Header(Header {
                prefix: prefix.map(str::to_owned),
                name: name.to_owned(),
            })
        };
        let kinds = kinds("; c\n[ profile \t dev ] # c\n\na = 1\n  b\n# c\n[default]");
        assert_eq!(kinds[0], LineKind::Comment);
        assert_eq!(kinds[1], header(Some("profile"), "dev"));
        assert_eq!(kinds[2], LineKind::Blank);
        assert!(matches!(&kinds[3], LineKind::Property(p) if p.value == "1\nb"));
        assert_eq!(kinds[4], LineKind::Continuation);
        assert_eq!(kinds[5], LineKind::Comment);
        assert_eq!(kinds[6], header(None, "default"));
    }

    #[test]
    fn test_value_spans() {
        let line = "Key  =  some value  # comment";
        let parsed = property(line);
        assert_eq!(parsed.key, "key");
        assert_eq!(parsed.value, "some value");
        assert_eq!(&line[parsed.value_span], "some value");

        let empty = property("key =   ; comment");
        assert_eq!(empty.value, "");
        assert_eq!(empty.value_span, 5..5, "right after `=`");
    }

    #[test]
    fn test_comments_in_values_need_whitespace() {
        assert_eq!(property("a = b#c").value, "b#c");
        assert_eq!(property("a = b #c").value, "b");
        assert_eq!(property("a = b\t;c").value, "b");
        // Right after `=` there is no whitespace, so this is a value.
        assert_eq!(property("a =#b").value, "#b");
    }

    #[test]
    fn test_crlf_and_bare_cr() {
        assert_eq!(content("a = b\r\n"), "a = b");
        assert_eq!(content("a = b\n"), "a = b");
        assert_eq!(content("a = b\r"), "a = b\r");
    }

    #[test]
    fn test_sub_properties() {
        let pairs: Vec<_> =
            sub_properties("\nmax_concurrent_requests = 10\n\tmax_queue_size=1000\n").collect();
        assert_eq!(
            pairs,
            [
                ("max_concurrent_requests", "10"),
                ("max_queue_size", "1000")
            ]
        );
    }

    #[test]
    fn test_is_valid_identifier() {
        assert!(is_valid_identifier("some-thing:long/the_one%only.foo@bar+"));
        assert!(!is_valid_identifier("foo bar"));
        assert!(!is_valid_identifier("foo!"));
        assert!(!is_valid_identifier("prod]"));
    }

    #[test]
    fn test_errors_report_line_numbers() {
        let error = parse_lines("[default]\n\nkey value").unwrap_err();
        assert_eq!(error.line(), 3);
        assert_eq!(
            *error.kind(),
            ParseErrorKind::MissingEquals {
                sub_property: false
            }
        );
    }
}
