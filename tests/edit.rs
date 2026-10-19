//! Edits change only what they must: comments, blank lines, ordering, key
//! spelling, spacing and line endings elsewhere stay as they were.

use aws_shared_config::{Document, EditError, FileKind, SectionId};

fn config(text: &str) -> Document {
    Document::parse(text, FileKind::Config).unwrap()
}

fn credentials(text: &str) -> Document {
    Document::parse(text, FileKind::Credentials).unwrap()
}

#[test]
fn set_replaces_value_in_place() {
    let mut doc = config("[profile dev]\nRegion=us-east-1   # main region\noutput = json\n");
    doc.profile_mut("dev").set("region", "eu-west-1").unwrap();
    assert_eq!(
        doc.to_string(),
        "[profile dev]\nRegion=eu-west-1   # main region\noutput = json\n"
    );
}

#[test]
fn set_fills_empty_value() {
    let mut doc = config("[default]\nregion =\noutput = ; pick later\n");
    doc.profile_mut("default")
        .set("region", "eu-west-1")
        .unwrap()
        .set("output", "json")
        .unwrap();
    assert_eq!(
        doc.to_string(),
        "[default]\nregion = eu-west-1\noutput = json ; pick later\n"
    );
}

#[test]
fn set_adds_key_after_last_property() {
    // The comment and blank line introduce the next section, so they stay
    // in front of it.
    let mut doc = config("[profile dev]\nregion = us-east-1\n\n# production\n[profile prod]\n");
    doc.profile_mut("dev").set("output", "json").unwrap();
    assert_eq!(
        doc.to_string(),
        "[profile dev]\nregion = us-east-1\noutput = json\n\n# production\n[profile prod]\n"
    );
}

#[test]
fn set_adds_key_to_empty_section() {
    let mut doc = config("[profile dev]\n# nothing yet\n");
    doc.profile_mut("dev").set("region", "eu-west-1").unwrap();
    assert_eq!(
        doc.to_string(),
        "[profile dev]\nregion = eu-west-1\n# nothing yet\n"
    );
}

#[test]
fn set_creates_section_at_end() {
    let mut doc = config("# AWS config\n[default]\nregion = us-east-1\n");
    doc.profile_mut("dev").set("region", "eu-west-1").unwrap();
    doc.sso_session_mut("corp")
        .set("sso_region", "us-east-1")
        .unwrap();
    assert_eq!(
        doc.to_string(),
        "# AWS config\n[default]\nregion = us-east-1\n\n\
         [profile dev]\nregion = eu-west-1\n\n\
         [sso-session corp]\nsso_region = us-east-1\n"
    );
}

#[test]
fn set_creates_section_in_empty_and_unterminated_files() {
    let mut empty = credentials("");
    empty
        .profile_mut("dev")
        .set("aws_access_key_id", "AKIA")
        .unwrap();
    assert_eq!(empty.to_string(), "[dev]\naws_access_key_id = AKIA\n");

    let mut unterminated = credentials("[default]\naws_access_key_id = A");
    unterminated
        .profile_mut("dev")
        .set("aws_access_key_id", "B")
        .unwrap();
    assert_eq!(
        unterminated.to_string(),
        "[default]\naws_access_key_id = A\n\n[dev]\naws_access_key_id = B\n"
    );
}

#[test]
fn headers_follow_the_file_kind() {
    let mut config = config("");
    config.profile_mut("default").set("region", "a").unwrap();
    config.profile_mut("dev").set("region", "b").unwrap();
    assert!(config.to_string().contains("[default]\n"));
    assert!(config.to_string().contains("[profile dev]\n"));

    let mut credentials = credentials("");
    credentials
        .profile_mut("dev")
        .set("aws_access_key_id", "A")
        .unwrap();
    assert!(credentials.to_string().starts_with("[dev]\n"));
    assert_eq!(
        credentials
            .sso_session_mut("corp")
            .set("sso_region", "x")
            .unwrap_err(),
        EditError::NotInCredentialsFile
    );
}

#[test]
fn keeps_crlf_line_endings() {
    let mut doc = credentials("[default]\r\naws_access_key_id = OLD\r\n");
    doc.profile_mut("default")
        .set("aws_access_key_id", "NEW")
        .unwrap()
        .set("aws_secret_access_key", "SECRET")
        .unwrap();
    doc.profile_mut("dev")
        .set("aws_access_key_id", "DEV")
        .unwrap();
    assert_eq!(
        doc.to_string(),
        "[default]\r\naws_access_key_id = NEW\r\naws_secret_access_key = SECRET\r\n\r\n\
         [dev]\r\naws_access_key_id = DEV\r\n"
    );
}

#[test]
fn set_replaces_multi_line_value_but_keeps_comments_between() {
    let mut doc = config("[default]\nlist = a\n  b\n# note\n  c\nnext = 1\n");
    doc.profile_mut("default").set("list", "single").unwrap();
    assert_eq!(
        doc.to_string(),
        "[default]\nlist = single\n# note\nnext = 1\n"
    );
}

#[test]
fn set_changes_the_value_that_wins() {
    // Keys and sections may repeat; the last one wins, so that one changes.
    let mut doc = credentials("[a]\nk = 1\nk = 2\n\n[a]\nk = 3\n");
    doc.profile_mut("a").set("k", "4").unwrap();
    assert_eq!(doc.to_string(), "[a]\nk = 1\nk = 2\n\n[a]\nk = 4\n");
    assert_eq!(doc.profile("a").unwrap().get("k"), Some("4"));
}

#[test]
fn prefixed_default_wins_over_plain_default() {
    let mut doc = config("[default]\nregion = ignored\n\n[profile default]\nregion = used\n");
    assert_eq!(doc.profile("default").unwrap().get("region"), Some("used"));
    doc.profile_mut("default").set("output", "json").unwrap();
    assert_eq!(
        doc.to_string(),
        "[default]\nregion = ignored\n\n[profile default]\nregion = used\noutput = json\n"
    );
}

#[test]
fn similar_names_do_not_collide() {
    let mut doc = credentials("[prod]\nk = PROD\n\n[production]\nk = PRODUCTION\n");
    doc.profile_mut("prod").set("k", "NEW").unwrap();
    assert_eq!(
        doc.to_string(),
        "[prod]\nk = NEW\n\n[production]\nk = PRODUCTION\n"
    );
}

#[test]
fn section_names_in_comments_and_values_are_not_headers() {
    let text = "# rotate [session] keys\n[default]\nnote = see [session]\n";
    let mut doc = credentials(text);
    assert!(doc.profile("session").is_none());
    doc.profile_mut("session").set("k", "v").unwrap();
    assert_eq!(doc.to_string(), format!("{text}\n[session]\nk = v\n"));
}

#[test]
fn values_are_literal() {
    // Regression: an earlier regex-based writer expanded `$1` and `${name}`.
    let mut doc = credentials("[s]\nk = old\n");
    doc.profile_mut("s").set("k", "se$1cret${name}").unwrap();
    assert_eq!(doc.profile("s").unwrap().get("k"), Some("se$1cret${name}"));
}

#[test]
fn unrepresentable_values_are_rejected() {
    let mut doc = config("[default]\nregion = us-east-1\n");
    let before = doc.clone();
    for value in [
        "two\nlines",
        "cr\r",
        " leading",
        "trailing\t",
        "#starts",
        ";starts",
        "has #comment",
        "has\t;comment",
    ] {
        let error = doc.profile_mut("default").set("region", value).unwrap_err();
        assert!(
            matches!(error, EditError::InvalidValue { .. }),
            "{value:?}: {error}"
        );
    }
    // `#` and `;` inside a word are fine.
    doc.profile_mut("default").set("tag", "a#b;c").unwrap();
    assert_eq!(doc.profile("default").unwrap().get("tag"), Some("a#b;c"));
    assert_ne!(doc, before);
}

#[test]
fn invalid_names_are_rejected() {
    let mut doc = credentials("");
    for name in ["", "has space", "bad]", "a=b"] {
        assert_eq!(
            doc.profile_mut("ok").set(name, "v").unwrap_err(),
            EditError::InvalidName(name.to_owned())
        );
        assert_eq!(
            doc.profile_mut(name).set("key", "v").unwrap_err(),
            EditError::InvalidName(name.to_owned())
        );
    }
    assert_eq!(doc.to_string(), "");
}

#[test]
fn remove_key_and_its_continuations() {
    let mut doc = config(
        "[default]\ns3 =\n  max_concurrent_requests = 10\nregion = x # keep?\noutput = json\n",
    );
    assert!(doc.profile_mut("default").remove("S3"));
    assert!(doc.profile_mut("default").remove("region"));
    assert!(!doc.profile_mut("default").remove("missing"));
    assert_eq!(doc.to_string(), "[default]\noutput = json\n");
}

#[test]
fn remove_from_missing_section_creates_nothing() {
    let mut doc = config("[default]\n");
    assert!(!doc.profile_mut("dev").remove("region"));
    assert_eq!(doc.to_string(), "[default]\n");
}

#[test]
fn remove_section_keeps_neighbours_tidy() {
    let text = "[a]\nk = 1\n\n[b]\nk = 2\n# about c\n\n[c]\nk = 3\n";

    let mut middle = credentials(text);
    assert!(middle.remove_profile("b"));
    assert_eq!(
        middle.to_string(),
        "[a]\nk = 1\n\n# about c\n\n[c]\nk = 3\n"
    );

    let mut first = credentials(text);
    assert!(first.remove_profile("a"));
    assert_eq!(first.to_string(), "[b]\nk = 2\n# about c\n\n[c]\nk = 3\n");

    let mut last = credentials(text);
    assert!(last.remove_profile("c"));
    assert_eq!(last.to_string(), "[a]\nk = 1\n\n[b]\nk = 2\n# about c\n");

    assert!(!last.remove_profile("missing"));
}

#[test]
fn remove_section_removes_every_declaration() {
    let mut doc = config("[default]\na = 1\n\n[profile default]\nb = 2\n\n[profile dev]\nc = 3\n");
    assert!(doc.remove_section(&SectionId::profile("default")));
    assert_eq!(doc.to_string(), "[profile dev]\nc = 3\n");
}

#[test]
fn sub_properties_round_trip() {
    let text = "[profile dev]\ns3 =\n    max_concurrent_requests = 10\n    max_queue_size=1000\n";
    let mut doc = config(text);
    let profile = doc.profile("dev").unwrap();
    assert_eq!(
        profile.sub_properties("s3").unwrap(),
        [
            ("max_concurrent_requests", "10"),
            ("max_queue_size", "1000")
        ]
    );
    assert_eq!(profile.get_sub("s3", "max_queue_size"), Some("1000"));

    doc.profile_mut("dev")
        .set_sub("s3", "max_queue_size", "2000")
        .unwrap()
        .set_sub("s3", "multipart_threshold", "64MB")
        .unwrap();
    assert_eq!(
        doc.to_string(),
        "[profile dev]\ns3 =\n    max_concurrent_requests = 10\n    max_queue_size=2000\n    \
         multipart_threshold = 64MB\n"
    );
}

#[test]
fn set_sub_creates_the_property_and_section() {
    let mut doc = config("[profile dev]\nservices = local\n");
    doc.section_mut(SectionId::services("local"))
        .set_sub("dynamodb", "endpoint_url", "http://localhost:8000")
        .unwrap();
    doc.profile_mut("dev")
        .set_sub("s3", "addressing_style", "path")
        .unwrap();
    assert_eq!(
        doc.to_string(),
        "[profile dev]\nservices = local\ns3 =\n  addressing_style = path\n\n\
         [services local]\ndynamodb =\n  endpoint_url = http://localhost:8000\n"
    );
    let services = doc.section(&SectionId::services("local")).unwrap();
    assert_eq!(
        services.get_sub("dynamodb", "endpoint_url"),
        Some("http://localhost:8000")
    );
}

#[test]
fn set_sub_needs_a_property_without_value() {
    let mut doc = config("[default]\nregion = us-east-1\n");
    assert_eq!(
        doc.profile_mut("default")
            .set_sub("region", "x", "y")
            .unwrap_err(),
        EditError::NotSubProperties("region".to_owned())
    );
}

#[test]
fn remove_sub_drops_the_empty_property() {
    let mut doc = config("[default]\ns3 =\n  a = 1\n  b = 2\nregion = x\n");
    assert!(doc.profile_mut("default").remove_sub("s3", "a"));
    assert_eq!(doc.to_string(), "[default]\ns3 =\n  b = 2\nregion = x\n");
    assert!(doc.profile_mut("default").remove_sub("s3", "b"));
    assert_eq!(doc.to_string(), "[default]\nregion = x\n");
    assert!(!doc.profile_mut("default").remove_sub("s3", "b"));
    assert!(!doc.profile_mut("default").remove_sub("region", "x"));
}

#[test]
fn reading_follows_the_sdks() {
    let doc = config(
        "[profile dev]\n\
         Region = eu-west-1 # comment\n\
         tag = a#b\n\
         list = one\n  two\n\
         in valid = ignored\n\
         [dev]\nignored = true\n",
    );
    let dev = doc.profile("dev").unwrap();
    assert_eq!(dev.get("region"), Some("eu-west-1"));
    assert_eq!(dev.get("REGION"), Some("eu-west-1"));
    assert_eq!(dev.get("tag"), Some("a#b"));
    assert_eq!(dev.get("list"), Some("one\ntwo"));
    assert_eq!(dev.get("in valid"), None);
    assert_eq!(
        dev.get("ignored"),
        None,
        "[dev] needs a `profile` prefix in the config file"
    );
    assert_eq!(
        dev.iter().collect::<Vec<_>>(),
        [
            ("region", "eu-west-1"),
            ("tag", "a#b"),
            ("list", "one\ntwo")
        ]
    );
    assert_eq!(dev.len(), 3);
    assert_eq!(doc.profile_names(), ["dev"]);
}

#[test]
fn parse_errors_have_line_numbers() {
    let error =
        Document::parse("[default]\nregion = x\n[profile dev\n", FileKind::Config).unwrap_err();
    assert_eq!(error.line(), 3);
    assert_eq!(
        error.to_string(),
        "line 3: Profile definition must end with ']'"
    );
}
