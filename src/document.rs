use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::error::{EditError, ParseError};
use crate::parse::{self, Header, Line, LineKind, Property, WHITESPACE, content, is_blank};

/// Which of the two shared files a document holds; they name sections
/// differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// The config file (`~/.aws/config`): `[default]`, `[profile name]`,
    /// `[sso-session name]`, `[services name]`.
    Config,
    /// The credentials file (`~/.aws/credentials`): only `[name]` profiles.
    Credentials,
}

/// The type of a section, given by its header's prefix.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// `[default]` or `[profile name]` in the config file, `[name]` in the
    /// credentials file.
    Profile,
    /// `[sso-session name]`: IAM Identity Center settings shared by profiles.
    SsoSession,
    /// `[services name]`: service-specific settings such as endpoints.
    Services,
    /// Any other `[prefix name]` section in the config file.
    Other(String),
}

/// A section, independent of how its header is spelled in a file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SectionId {
    kind: SectionKind,
    name: String,
}

impl SectionId {
    /// A profile; `default` is the default profile.
    pub fn profile(name: impl Into<String>) -> Self {
        Self::new(SectionKind::Profile, name)
    }

    /// An `[sso-session name]` section.
    pub fn sso_session(name: impl Into<String>) -> Self {
        Self::new(SectionKind::SsoSession, name)
    }

    /// A `[services name]` section.
    pub fn services(name: impl Into<String>) -> Self {
        Self::new(SectionKind::Services, name)
    }

    /// A `[prefix name]` section of a type this crate has no shortcut for.
    pub fn other(prefix: impl Into<String>, name: impl Into<String>) -> Self {
        let prefix = prefix.into();
        let kind = match prefix.as_str() {
            "profile" => SectionKind::Profile,
            "sso-session" => SectionKind::SsoSession,
            "services" => SectionKind::Services,
            _ => SectionKind::Other(prefix),
        };
        Self::new(kind, name)
    }

    fn new(kind: SectionKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
        }
    }

    /// The section's type.
    pub fn kind(&self) -> &SectionKind {
        &self.kind
    }

    /// The section's name: `dev` for `[profile dev]`.
    pub fn name(&self) -> &str {
        &self.name
    }

    fn is_default_profile(&self) -> bool {
        self.kind == SectionKind::Profile && self.name == "default"
    }

    /// The section a header declares, or `None` if the SDKs ignore it.
    fn of_header(header: &Header, file: FileKind) -> Option<Self> {
        if !parse::is_valid_identifier(&header.name) {
            return None;
        }
        match (file, header.prefix.as_deref()) {
            (FileKind::Credentials, None) => Some(Self::profile(&header.name)),
            // Credentials files only hold `[name]` profiles.
            (FileKind::Credentials, Some(_)) => None,
            (FileKind::Config, None) if header.name == "default" => Some(Self::profile("default")),
            // Apart from `[default]`, config file sections need a prefix.
            (FileKind::Config, None) => None,
            (FileKind::Config, Some(prefix)) => Some(Self::other(prefix, &header.name)),
        }
    }

    /// The header that declares this section in `file`.
    fn header(&self, file: FileKind) -> Result<String, EditError> {
        check_name(&self.name)?;
        Ok(match (file, &self.kind) {
            (FileKind::Credentials, SectionKind::Profile) => format!("[{}]", self.name),
            (FileKind::Credentials, _) => return Err(EditError::NotInCredentialsFile),
            (FileKind::Config, SectionKind::Profile) if self.name == "default" => {
                "[default]".to_owned()
            }
            (FileKind::Config, SectionKind::Profile) => format!("[profile {}]", self.name),
            (FileKind::Config, SectionKind::SsoSession) => format!("[sso-session {}]", self.name),
            (FileKind::Config, SectionKind::Services) => format!("[services {}]", self.name),
            (FileKind::Config, SectionKind::Other(prefix)) => {
                check_name(prefix)?;
                format!("[{prefix} {}]", self.name)
            }
        })
    }
}

impl fmt::Display for SectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            SectionKind::Profile => write!(f, "profile {}", self.name),
            SectionKind::SsoSession => write!(f, "sso-session {}", self.name),
            SectionKind::Services => write!(f, "services {}", self.name),
            SectionKind::Other(prefix) => write!(f, "{prefix} {}", self.name),
        }
    }
}

/// A parsed config or credentials file that prints back exactly as it was
/// read, apart from the edits made to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    file: FileKind,
    lines: Vec<Line>,
}

impl Document {
    /// An empty document.
    pub fn new(file: FileKind) -> Self {
        Self {
            file,
            lines: Vec::new(),
        }
    }

    /// Parse `text` with the rules the AWS SDKs use.
    pub fn parse(text: &str, file: FileKind) -> Result<Self, ParseError> {
        Ok(Self {
            file,
            lines: parse::parse_lines(text)?,
        })
    }

    /// Whether this is a config or a credentials file.
    pub fn file_kind(&self) -> FileKind {
        self.file
    }

    /// The profile named `name`, if the file defines it.
    pub fn profile(&self, name: &str) -> Option<Section<'_>> {
        self.section(&SectionId::profile(name))
    }

    /// The `[sso-session name]` section, if the file defines it.
    pub fn sso_session(&self, name: &str) -> Option<Section<'_>> {
        self.section(&SectionId::sso_session(name))
    }

    /// The section `id`, if the file defines it. Sections declared more than
    /// once read as one, with later values winning.
    pub fn section(&self, id: &SectionId) -> Option<Section<'_>> {
        let headers = self.effective_headers(id);
        (!headers.is_empty()).then(|| Section {
            doc: self,
            id: id.clone(),
            headers,
        })
    }

    /// The names of the profiles in the file, in order of first appearance.
    pub fn profile_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for (_, header) in self.headers() {
            let is_profile = SectionId::of_header(header, self.file)
                .is_some_and(|id| id.kind == SectionKind::Profile);
            if is_profile && !names.contains(&header.name.as_str()) {
                names.push(&header.name);
            }
        }
        names
    }

    /// Every section the SDKs read from the file, in order of first appearance.
    pub fn section_ids(&self) -> Vec<SectionId> {
        let mut ids: Vec<SectionId> = Vec::new();
        for (_, header) in self.headers() {
            if let Some(id) = SectionId::of_header(header, self.file) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        ids
    }

    /// Edit the profile named `name`. It is created when first given a value.
    pub fn profile_mut(&mut self, name: &str) -> SectionMut<'_> {
        self.section_mut(SectionId::profile(name))
    }

    /// Edit the `[sso-session name]` section. It is created when first given
    /// a value.
    pub fn sso_session_mut(&mut self, name: &str) -> SectionMut<'_> {
        self.section_mut(SectionId::sso_session(name))
    }

    /// Edit the section `id`. It is created when first given a value.
    pub fn section_mut(&mut self, id: SectionId) -> SectionMut<'_> {
        SectionMut { doc: self, id }
    }

    /// Remove the profile named `name`. Returns whether it existed.
    pub fn remove_profile(&mut self, name: &str) -> bool {
        self.remove_section(&SectionId::profile(name))
    }

    /// Remove every declaration of section `id`: each header and the lines up
    /// to its last property. Comments and blank lines after the last property
    /// stay, since they usually introduce the next section. Returns whether
    /// the section existed.
    pub fn remove_section(&mut self, id: &SectionId) -> bool {
        let headers: Vec<usize> = self
            .headers()
            .filter(|(_, header)| SectionId::of_header(header, self.file).as_ref() == Some(id))
            .map(|(index, _)| index)
            .collect();
        if headers.is_empty() {
            return false;
        }

        let mut rewrite = Rewrite::default();
        for header in headers {
            let end = self.last_entry(header);
            rewrite.remove.extend(header..=end);
            // Keep a single blank line between the neighbours.
            let blank = |index: usize| {
                self.lines
                    .get(index)
                    .is_some_and(|line| line.kind == LineKind::Blank)
            };
            let blank_before = header == 0 || blank(header - 1);
            if blank_before && blank(end + 1) {
                rewrite.remove.insert(end + 1);
            } else if header > 0 && blank(header - 1) && end + 1 == self.lines.len() {
                rewrite.remove.insert(header - 1);
            }
        }
        self.apply(rewrite)
            .expect("removing whole sections leaves a valid document");
        true
    }

    fn headers(&self) -> impl Iterator<Item = (usize, &Header)> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| match &line.kind {
                LineKind::Header(header) => Some((index, header)),
                _ => None,
            })
    }

    /// The header lines declaring `id` that the SDKs read. In the config
    /// file, `[profile default]` takes priority over `[default]`.
    fn effective_headers(&self, id: &SectionId) -> Vec<usize> {
        let matching: Vec<(usize, &Header)> = self
            .headers()
            .filter(|(_, header)| SectionId::of_header(header, self.file).as_ref() == Some(id))
            .collect();
        let prefixed_default = self.file == FileKind::Config
            && id.is_default_profile()
            && matching.iter().any(|(_, header)| header.prefix.is_some());
        matching
            .into_iter()
            .filter(|(_, header)| !prefixed_default || header.prefix.is_some())
            .map(|(index, _)| index)
            .collect()
    }

    /// The lines after a header, up to the next header.
    fn body(&self, header: usize) -> std::ops::Range<usize> {
        let end = self.lines[header + 1..]
            .iter()
            .position(|line| matches!(line.kind, LineKind::Header(_)))
            .map_or(self.lines.len(), |offset| header + 1 + offset);
        header + 1..end
    }

    /// The last property or continuation line of a section, or its header.
    fn last_entry(&self, header: usize) -> usize {
        self.body(header)
            .rev()
            .find(|&index| {
                matches!(
                    self.lines[index].kind,
                    LineKind::Property(_) | LineKind::Continuation
                )
            })
            .unwrap_or(header)
    }

    /// The property lines of the given sections whose key the SDKs accept.
    fn properties<'a, 'h>(
        &'a self,
        headers: &'h [usize],
    ) -> impl Iterator<Item = (usize, &'a Property)> + use<'a, 'h> {
        headers.iter().copied().flat_map(move |header| {
            self.body(header)
                .filter_map(move |index| match &self.lines[index].kind {
                    LineKind::Property(property) if parse::is_valid_identifier(&property.key) => {
                        Some((index, property))
                    }
                    _ => None,
                })
        })
    }

    /// The line of the property that provides `key`'s value: the last one.
    fn find_property(&self, headers: &[usize], key: &str) -> Option<(usize, &Property)> {
        let key = key.to_ascii_lowercase();
        self.properties(headers)
            .filter(|(_, property)| property.key == key)
            .last()
    }

    /// The continuation lines of the property on line `index`.
    fn continuations(&self, index: usize) -> Vec<usize> {
        self.lines[index + 1..]
            .iter()
            .enumerate()
            .take_while(|(_, line)| {
                !matches!(line.kind, LineKind::Property(_) | LineKind::Header(_))
            })
            .filter(|(_, line)| line.kind == LineKind::Continuation)
            .map(|(offset, _)| index + 1 + offset)
            .collect()
    }

    /// The line terminator new lines use: whichever the file already uses.
    fn eol(&self) -> &'static str {
        self.lines
            .iter()
            .find(|line| line.raw.ends_with('\n'))
            .map_or("\n", |line| {
                if line.raw.ends_with("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                }
            })
    }

    /// Lines that add a new section declaring `id` to the end of the file.
    fn new_section(&self, id: &SectionId, entries: &[String]) -> Result<Vec<String>, EditError> {
        let eol = self.eol();
        let mut lines = Vec::new();
        if self
            .lines
            .last()
            .is_some_and(|line| !is_blank(content(&line.raw)))
        {
            lines.push(eol.to_owned());
        }
        lines.push(format!("{}{eol}", id.header(self.file)?));
        lines.extend(entries.iter().map(|entry| format!("{entry}{eol}")));
        Ok(lines)
    }

    /// Rebuild the text with `rewrite` applied and re-parse it, so every
    /// parsed field matches the new text. The document is left unchanged if
    /// the result would not parse.
    fn apply(&mut self, rewrite: Rewrite) -> Result<(), EditError> {
        let eol = self.eol();
        let mut text = String::new();
        let mut push = |line: &str| {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push_str(eol);
            }
            text.push_str(line);
        };
        for (index, line) in self.lines.iter().enumerate() {
            if !rewrite.remove.contains(&index) {
                push(rewrite.replace.get(&index).unwrap_or(&line.raw));
            }
            for new in rewrite.insert_after.get(&index).into_iter().flatten() {
                push(new);
            }
        }
        for new in &rewrite.append {
            push(new);
        }
        self.lines = parse::parse_lines(&text).map_err(EditError::WouldNotParse)?;
        Ok(())
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.lines
            .iter()
            .try_for_each(|line| f.write_str(&line.raw))
    }
}

/// Line edits applied together, by index into the current lines.
#[derive(Default)]
struct Rewrite {
    replace: BTreeMap<usize, String>,
    remove: BTreeSet<usize>,
    insert_after: BTreeMap<usize, Vec<String>>,
    append: Vec<String>,
}

/// A read-only view of a section.
#[derive(Clone, Debug)]
pub struct Section<'a> {
    doc: &'a Document,
    id: SectionId,
    headers: Vec<usize>,
}

impl<'a> Section<'a> {
    /// Which section this is.
    pub fn id(&self) -> &SectionId {
        &self.id
    }

    /// The section's name: `dev` for `[profile dev]`.
    pub fn name(&self) -> &str {
        &self.id.name
    }

    /// The value of `key` (compared case-insensitively). For a property with
    /// sub-properties this is their raw text; see [`Section::sub_properties`].
    pub fn get(&self, key: &str) -> Option<&'a str> {
        self.doc
            .find_property(&self.headers, key)
            .map(|(_, property)| property.value.as_str())
    }

    /// Whether `key` is set (compared case-insensitively).
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Every key with its value, in order of first appearance. Keys set more
    /// than once have their last value.
    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        let mut entries: Vec<(&'a str, &'a str)> = Vec::new();
        for (_, property) in self.doc.properties(&self.headers) {
            match entries.iter_mut().find(|(key, _)| *key == property.key) {
                Some(entry) => entry.1 = &property.value,
                None => entries.push((&property.key, &property.value)),
            }
        }
        entries.into_iter()
    }

    /// The number of distinct keys.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Whether the section has no keys.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The sub-properties of `key`, such as `max_concurrent_requests` under
    /// `s3 =`. `None` if `key` is missing or has a value of its own.
    pub fn sub_properties(&self, key: &str) -> Option<Vec<(&'a str, &'a str)>> {
        let value = self.get(key)?;
        has_sub_properties(value).then(|| parse::sub_properties(value).collect())
    }

    /// The value of the sub-property `sub_key` of `key`.
    pub fn get_sub(&self, key: &str, sub_key: &str) -> Option<&'a str> {
        self.sub_properties(key)?
            .into_iter()
            .rev()
            .find(|(name, _)| *name == sub_key)
            .map(|(_, value)| value)
    }
}

/// A handle for editing a section. Edits keep everything else in the file —
/// comments, blank lines, ordering and spacing — as it was.
#[derive(Debug)]
pub struct SectionMut<'a> {
    doc: &'a mut Document,
    id: SectionId,
}

impl SectionMut<'_> {
    /// Which section this edits.
    pub fn id(&self) -> &SectionId {
        &self.id
    }

    /// A read-only view of the section, if it exists.
    pub fn as_section(&self) -> Option<Section<'_>> {
        self.doc.section(&self.id)
    }

    /// The value of `key`, as [`Section::get`] returns it.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.doc
            .find_property(&self.doc.effective_headers(&self.id), key)
            .map(|(_, property)| property.value.as_str())
    }

    /// Set `key` to `value`.
    ///
    /// An existing value is replaced in place, keeping the line's key
    /// spelling, spacing and trailing comment. A new key goes after the
    /// section's last property, and a new section goes at the end of the file.
    pub fn set(&mut self, key: &str, value: &str) -> Result<&mut Self, EditError> {
        check_name(key)?;
        check_value(value)?;
        let doc = &*self.doc;
        let headers = doc.effective_headers(&self.id);
        let mut rewrite = Rewrite::default();

        if let Some((index, property)) = doc.find_property(&headers, key) {
            let raw = &doc.lines[index].raw;
            rewrite
                .replace
                .insert(index, replace_span(raw, property.value_span.clone(), value));
            rewrite.remove.extend(doc.continuations(index));
        } else if let Some(&last) = headers.last() {
            let line = format!("{key} = {value}{}", doc.eol());
            rewrite
                .insert_after
                .insert(doc.last_entry(last), vec![line]);
        } else {
            rewrite.append = doc.new_section(&self.id, &[format!("{key} = {value}")])?;
        }
        self.doc.apply(rewrite)?;
        Ok(self)
    }

    /// Remove `key`. Returns whether it was set.
    pub fn remove(&mut self, key: &str) -> bool {
        let key = key.to_ascii_lowercase();
        let doc = &*self.doc;
        let headers = doc.effective_headers(&self.id);
        let mut rewrite = Rewrite::default();
        for (index, _) in doc
            .properties(&headers)
            .filter(|(_, property)| property.key == key)
        {
            rewrite.remove.insert(index);
            rewrite.remove.extend(doc.continuations(index));
        }
        if rewrite.remove.is_empty() {
            return false;
        }
        self.doc
            .apply(rewrite)
            .expect("removing whole properties leaves a valid document");
        true
    }

    /// Set the sub-property `sub_key` of `key`, creating `key =` if needed.
    pub fn set_sub(
        &mut self,
        key: &str,
        sub_key: &str,
        value: &str,
    ) -> Result<&mut Self, EditError> {
        check_name(key)?;
        check_name(sub_key)?;
        check_value(value)?;
        let doc = &*self.doc;
        let eol = doc.eol();
        let headers = doc.effective_headers(&self.id);
        let mut rewrite = Rewrite::default();

        if let Some((index, property)) = doc.find_property(&headers, key) {
            if !has_sub_properties(&property.value) {
                return Err(EditError::NotSubProperties(key.to_owned()));
            }
            let lines = doc.continuations(index);
            let existing = lines.iter().rev().find_map(|&line| {
                sub_value_span(&doc.lines[line].raw, sub_key).map(|span| (line, span))
            });
            match existing {
                Some((line, span)) => {
                    rewrite
                        .replace
                        .insert(line, replace_span(&doc.lines[line].raw, span, value));
                }
                None => {
                    let indent = lines.last().map_or("  ", |&line| {
                        let raw = &doc.lines[line].raw;
                        &raw[..raw.len() - raw.trim_start_matches(WHITESPACE).len()]
                    });
                    let new = format!("{indent}{sub_key} = {value}{eol}");
                    rewrite
                        .insert_after
                        .insert(lines.last().copied().unwrap_or(index), vec![new]);
                }
            }
        } else {
            let entries = [format!("{key} ="), format!("  {sub_key} = {value}")];
            match headers.last() {
                Some(&last) => {
                    let lines = entries
                        .iter()
                        .map(|entry| format!("{entry}{eol}"))
                        .collect();
                    rewrite.insert_after.insert(doc.last_entry(last), lines);
                }
                None => rewrite.append = doc.new_section(&self.id, &entries)?,
            }
        }
        self.doc.apply(rewrite)?;
        Ok(self)
    }

    /// Remove the sub-property `sub_key` of `key`, and `key` itself once it
    /// has no sub-properties left. Returns whether the sub-property was set.
    pub fn remove_sub(&mut self, key: &str, sub_key: &str) -> bool {
        let doc = &*self.doc;
        let headers = doc.effective_headers(&self.id);
        let Some((index, property)) = doc.find_property(&headers, key) else {
            return false;
        };
        if !has_sub_properties(&property.value) {
            return false;
        }
        let lines = doc.continuations(index);
        let (matching, rest): (Vec<usize>, Vec<usize>) = lines
            .iter()
            .partition(|&&line| sub_value_span(&doc.lines[line].raw, sub_key).is_some());
        if matching.is_empty() {
            return false;
        }
        let mut rewrite = Rewrite::default();
        rewrite.remove.extend(matching);
        if rest.is_empty() {
            rewrite.remove.insert(index);
        }
        self.doc
            .apply(rewrite)
            .expect("removing sub-property lines leaves a valid document");
        true
    }

    #[cfg(feature = "serde")]
    pub(crate) fn document(&mut self) -> &mut Document {
        self.doc
    }
}

/// Whether a property value holds sub-properties: its first line is empty.
fn has_sub_properties(value: &str) -> bool {
    value.is_empty() || value.starts_with('\n')
}

/// Replace the value at `span` of `raw`, keeping a space after `=`.
fn replace_span(raw: &str, span: std::ops::Range<usize>, value: &str) -> String {
    let before = &raw[..span.start];
    let mut line = String::with_capacity(raw.len() + value.len());
    line.push_str(before);
    if span.is_empty() && !value.is_empty() && !before.ends_with(WHITESPACE) {
        line.push(' ');
    }
    line.push_str(value);
    line.push_str(&raw[span.end..]);
    line
}

/// The span of the value in a sub-property line `  key = value`, if its key
/// is `key`. Comments are part of sub-property values, as in the SDKs.
fn sub_value_span(raw: &str, key: &str) -> Option<std::ops::Range<usize>> {
    let line = content(raw);
    let equals = line.find('=')?;
    if line[..equals].trim_matches(WHITESPACE) != key {
        return None;
    }
    let after = &line[equals + 1..];
    let start = equals + 1 + (after.len() - after.trim_start_matches(WHITESPACE).len());
    let end = line.trim_end_matches(WHITESPACE).len().max(start);
    Some(start..end)
}

fn check_name(name: &str) -> Result<(), EditError> {
    if !name.is_empty() && parse::is_valid_identifier(name) {
        Ok(())
    } else {
        Err(EditError::InvalidName(name.to_owned()))
    }
}

/// Reject values that would read back differently: the format has no quoting
/// or escapes.
fn check_value(value: &str) -> Result<(), EditError> {
    let invalid = |reason| {
        Err(EditError::InvalidValue {
            value: value.to_owned(),
            reason,
        })
    };
    if value.contains(['\n', '\r']) {
        return invalid("values cannot span lines; use sub-properties");
    }
    if value.starts_with(WHITESPACE) || value.ends_with(WHITESPACE) {
        return invalid("leading and trailing whitespace is not kept");
    }
    if value.starts_with(['#', ';']) {
        return invalid("a value cannot start with `#` or `;`");
    }
    let mut previous_whitespace = false;
    for c in value.chars() {
        if matches!(c, '#' | ';') && previous_whitespace {
            return invalid("`#` or `;` after whitespace starts a comment");
        }
        previous_whitespace = c.is_whitespace();
    }
    Ok(())
}
