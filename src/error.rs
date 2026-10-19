use std::error::Error;
use std::fmt;

/// The text is not a valid profile file; the AWS SDKs would reject it too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    line: usize,
    kind: ParseErrorKind,
}

impl ParseError {
    pub(crate) fn new(line: usize, kind: ParseErrorKind) -> Self {
        Self { line, kind }
    }

    /// The 1-based line number of the offending line.
    pub fn line(&self) -> usize {
        self.line
    }

    /// What is wrong with the line.
    pub fn kind(&self) -> &ParseErrorKind {
        &self.kind
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.kind)
    }
}

impl Error for ParseError {}

/// What is wrong with a line. The messages match the AWS SDKs' own.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// A property or continuation line before the first section header.
    NoSection,
    /// An indented line that does not follow a property.
    ContinuationWithoutProperty,
    /// A section header without a closing `]`.
    UnclosedHeader,
    /// A property line without `=`.
    MissingEquals {
        /// Whether the line is an indented sub-property.
        sub_property: bool,
    },
    /// A property line with nothing before `=`.
    EmptyKey {
        /// Whether the line is an indented sub-property.
        sub_property: bool,
    },
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = |sub_property: bool| {
            if sub_property {
                "sub-property"
            } else {
                "property"
            }
        };
        match self {
            ParseErrorKind::NoSection => f.write_str("Expected a profile definition"),
            ParseErrorKind::ContinuationWithoutProperty => {
                f.write_str("Expected a property definition, found continuation")
            }
            ParseErrorKind::UnclosedHeader => f.write_str("Profile definition must end with ']'"),
            ParseErrorKind::MissingEquals { sub_property } => {
                write!(f, "Expected an '=' sign defining a {}", what(*sub_property))
            }
            ParseErrorKind::EmptyKey {
                sub_property: false,
            } => f.write_str("Property did not have a name"),
            ParseErrorKind::EmptyKey { sub_property: true } => {
                f.write_str("Sub-property did not have a name")
            }
        }
    }
}

/// An edit that cannot be expressed in a profile file.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EditError {
    /// Section and key names may only use ASCII letters, digits and `_-/.%@:+`.
    InvalidName(String),
    /// The value cannot be written so that it reads back unchanged.
    InvalidValue {
        /// The rejected value.
        value: String,
        /// Why it cannot be written.
        reason: &'static str,
    },
    /// Sub-properties can only be added under a property without a value.
    NotSubProperties(String),
    /// Credentials files may only contain profiles.
    NotInCredentialsFile,
    /// The edited text would not parse. Valid input never causes this.
    WouldNotParse(ParseError),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::InvalidName(name) => write!(
                f,
                "invalid name `{name}`: use only ASCII letters, digits and _-/.%@:+"
            ),
            EditError::InvalidValue { value, reason } => {
                write!(f, "cannot write the value `{value}`: {reason}")
            }
            EditError::NotSubProperties(key) => {
                write!(f, "`{key}` has a value, so it cannot hold sub-properties")
            }
            EditError::NotInCredentialsFile => {
                f.write_str("credentials files can only contain profiles")
            }
            EditError::WouldNotParse(error) => write!(f, "the edit would not parse: {error}"),
        }
    }
}

impl Error for EditError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            EditError::WouldNotParse(error) => Some(error),
            _ => None,
        }
    }
}

/// Converting a section to or from a Rust type failed.
#[cfg(feature = "serde")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerdeError(String);

#[cfg(feature = "serde")]
impl fmt::Display for SerdeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(feature = "serde")]
impl Error for SerdeError {}

#[cfg(feature = "serde")]
impl serde::de::Error for SerdeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

#[cfg(feature = "serde")]
impl serde::ser::Error for SerdeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

#[cfg(feature = "serde")]
impl From<EditError> for SerdeError {
    fn from(error: EditError) -> Self {
        Self(error.to_string())
    }
}
