//! Reading sections into Rust types and writing them back in place.
#![cfg(feature = "serde")]

use std::collections::BTreeMap;

use aws_shared_config::{Document, FileKind};
use serde::{Deserialize, Serialize};

const CONFIG: &str = "\
# Development account
[profile dev]
region = eu-west-1 # closest to the team
max_attempts = 5
retry_mode = adaptive
use_fips_endpoint = TRUE
s3 =
  max_concurrent_requests = 20
  addressing_style = path
mfa_serial = arn:aws:iam::123456789012:mfa/alice
";

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum RetryMode {
    Standard,
    Adaptive,
}

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct S3 {
    max_concurrent_requests: Option<u32>,
    addressing_style: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct Profile<'a> {
    region: &'a str,
    max_attempts: u8,
    retry_mode: RetryMode,
    use_fips_endpoint: bool,
    s3: S3,
    output: Option<String>,
}

fn config(text: &str) -> Document {
    Document::parse(text, FileKind::Config).unwrap()
}

#[test]
fn deserialize_typed_profile() {
    let doc = config(CONFIG);
    let profile: Profile = doc.profile("dev").unwrap().deserialize().unwrap();
    assert_eq!(
        profile,
        Profile {
            region: "eu-west-1",
            max_attempts: 5,
            retry_mode: RetryMode::Adaptive,
            use_fips_endpoint: true,
            s3: S3 {
                max_concurrent_requests: Some(20),
                addressing_style: Some("path".to_owned()),
            },
            output: None,
        }
    );
}

#[test]
fn deserialize_into_maps() {
    let doc = config(CONFIG);
    let section = doc.profile("dev").unwrap();
    let all: BTreeMap<String, String> = section.deserialize().unwrap();
    assert_eq!(all["region"], "eu-west-1");
    assert_eq!(
        all["s3"],
        "\nmax_concurrent_requests = 20\naddressing_style = path"
    );

    #[derive(Deserialize)]
    struct OnlyS3 {
        s3: BTreeMap<String, String>,
    }
    let s3 = section.deserialize::<OnlyS3>().unwrap().s3;
    assert_eq!(s3["addressing_style"], "path");
}

#[test]
fn deserialize_errors_name_the_key() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Wrong {
        region: u32,
    }
    let doc = config(CONFIG);
    let error = doc
        .profile("dev")
        .unwrap()
        .deserialize::<Wrong>()
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "`region`: expected a non-negative integer, found `eu-west-1`"
    );

    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Missing {
        sso_start_url: String,
    }
    let error = doc
        .profile("dev")
        .unwrap()
        .deserialize::<Missing>()
        .unwrap_err();
    assert_eq!(error.to_string(), "missing field `sso_start_url`");
}

#[derive(Serialize)]
struct Update<'a> {
    region: &'a str,
    max_attempts: Option<u8>,
    retry_mode: RetryMode,
    s3: S3,
}

#[test]
fn serialize_merges_in_place() {
    let mut doc = config(CONFIG);
    doc.profile_mut("dev")
        .serialize(&Update {
            region: "us-west-2",
            max_attempts: None,
            retry_mode: RetryMode::Standard,
            s3: S3 {
                max_concurrent_requests: Some(50),
                addressing_style: None,
            },
        })
        .unwrap();
    // The comments, the untouched keys and their order stay; `None` removes.
    assert_eq!(
        doc.to_string(),
        "\
# Development account
[profile dev]
region = us-west-2 # closest to the team
retry_mode = standard
use_fips_endpoint = TRUE
s3 =
  max_concurrent_requests = 50
mfa_serial = arn:aws:iam::123456789012:mfa/alice
"
    );
}

#[test]
fn serialize_creates_the_section() {
    #[derive(Serialize)]
    struct Credentials<'a> {
        aws_access_key_id: &'a str,
        aws_secret_access_key: &'a str,
        aws_session_token: Option<&'a str>,
    }
    let mut doc =
        Document::parse("[default]\naws_access_key_id = A\n", FileKind::Credentials).unwrap();
    doc.profile_mut("session")
        .serialize(&Credentials {
            aws_access_key_id: "ASIA",
            aws_secret_access_key: "secret",
            aws_session_token: Some("token"),
        })
        .unwrap();
    assert_eq!(
        doc.to_string(),
        "[default]\naws_access_key_id = A\n\n[session]\n\
         aws_access_key_id = ASIA\naws_secret_access_key = secret\naws_session_token = token\n"
    );
}

#[test]
fn serialize_maps() {
    let mut doc = config("[default]\n");
    let mut values = BTreeMap::new();
    values.insert("region", "eu-north-1");
    values.insert("output", "text");
    doc.profile_mut("default").serialize(&values).unwrap();
    assert_eq!(
        doc.to_string(),
        "[default]\noutput = text\nregion = eu-north-1\n"
    );
}

#[test]
fn serialize_failure_changes_nothing() {
    #[derive(Serialize)]
    struct Bad<'a> {
        region: &'a str,
        output: &'a str,
    }
    let mut doc = config(CONFIG);
    let error = doc
        .profile_mut("dev")
        .serialize(&Bad {
            region: "us-west-2",
            output: "json # with a comment",
        })
        .unwrap_err();
    assert!(error.to_string().contains("starts a comment"), "{error}");
    assert_eq!(doc.to_string(), CONFIG, "the region change was rolled back");
}

#[test]
fn serialize_rejects_shapes_the_format_cannot_hold() {
    let mut doc = config("");

    let error = doc
        .profile_mut("dev")
        .serialize(&"just a string")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "a section can only be written from a struct or map"
    );

    #[derive(Serialize)]
    struct WithList {
        regions: Vec<&'static str>,
    }
    let error = doc
        .profile_mut("dev")
        .serialize(&WithList { regions: vec!["a"] })
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "a list cannot be written to a profile file"
    );

    #[derive(Serialize)]
    struct Deep {
        s3: BTreeMap<&'static str, BTreeMap<&'static str, &'static str>>,
    }
    let error = doc
        .profile_mut("dev")
        .serialize(&Deep {
            s3: BTreeMap::from([("a", BTreeMap::from([("b", "c")]))]),
        })
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "a sub-property with sub-properties cannot be written to a profile file"
    );
    assert_eq!(doc.to_string(), "");
}

#[test]
fn round_trip_through_a_struct() {
    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct Session {
        aws_access_key_id: String,
        aws_secret_access_key: String,
        aws_session_token: String,
        expiration: Option<String>,
    }
    let session = Session {
        aws_access_key_id: "ASIA".to_owned(),
        aws_secret_access_key: "secret/with+chars=".to_owned(),
        aws_session_token: "token==".to_owned(),
        expiration: Some("2027-01-15T08:00:00Z".to_owned()),
    };
    let mut doc = Document::new(FileKind::Credentials);
    doc.profile_mut("session").serialize(&session).unwrap();
    let text = doc.to_string();

    let reread = Document::parse(&text, FileKind::Credentials).unwrap();
    assert_eq!(
        reread
            .profile("session")
            .unwrap()
            .deserialize::<Session>()
            .unwrap(),
        session
    );
}
