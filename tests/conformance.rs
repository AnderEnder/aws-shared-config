//! Runs the profile parser test suite the AWS SDKs share (vendored from the
//! AWS SDK for Rust; see `tests/data/README.md`), and checks that every input
//! prints back unchanged.

use std::collections::BTreeMap;

use aws_shared_config::{Document, FileKind, SectionKind};
use serde::Deserialize;

type Properties = BTreeMap<String, String>;

#[derive(Deserialize)]
struct Suite {
    tests: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    input: Input,
    output: Output,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    config_file: Option<String>,
    credentials_file: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Output {
    config: Option<Expected>,
    error_containing: Option<String>,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
struct Expected {
    profiles: BTreeMap<String, Properties>,
    #[serde(default)]
    sso_sessions: BTreeMap<String, Properties>,
}

/// Read both files the way the SDKs merge them: profiles from the config
/// file, then the credentials file, whose values win.
fn load(input: &Input) -> Result<Expected, String> {
    let mut result = Expected::default();
    let files = [
        (&input.config_file, FileKind::Config),
        (&input.credentials_file, FileKind::Credentials),
    ];
    for (text, kind) in files {
        let Some(text) = text else { continue };
        let doc = Document::parse(text, kind).map_err(|error| error.to_string())?;
        assert_eq!(doc.to_string(), *text, "printing must reproduce the input");

        for id in doc.section_ids() {
            let target = match id.kind() {
                SectionKind::Profile => &mut result.profiles,
                SectionKind::SsoSession => &mut result.sso_sessions,
                _ => continue,
            };
            let properties = target.entry(id.name().to_owned()).or_default();
            for (key, value) in doc.section(&id).unwrap().iter() {
                properties.insert(key.to_owned(), value.to_owned());
            }
        }
    }
    Ok(result)
}

#[test]
fn sdk_profile_parser_suite() {
    let suite: Suite =
        serde_json::from_str(include_str!("data/profile-parser-tests.json")).unwrap();
    assert_eq!(suite.tests.len(), 65);

    let mut failures = Vec::new();
    for case in &suite.tests {
        let actual = load(&case.input);
        let passed = match (&case.output.config, &case.output.error_containing, &actual) {
            (Some(expected), _, Ok(actual)) => expected == actual,
            (_, Some(message), Err(error)) => error.contains(message.as_str()),
            _ => false,
        };
        if !passed {
            failures.push(format!(
                "{}\n  input:    {:?} / {:?}\n  expected: {:?} / {:?}\n  actual:   {:?}",
                case.name,
                case.input.config_file,
                case.input.credentials_file,
                case.output.config,
                case.output.error_containing,
                actual
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
