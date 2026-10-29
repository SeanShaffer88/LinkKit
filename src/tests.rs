//! Core tests: spec parsing/validation and read-result
//! shaping. All of this is platform-independent — the COM
//! layer is exercised separately (the Windows-only FFI
//! integration test in `ffi.rs`).

use super::*;

fn create_err(spec: &str) -> (i32, String) {
    parse_create_spec(spec).unwrap_err()
}

#[test]
fn create_spec_minimal_defaults() {
    let spec =
        parse_create_spec(r#"{"path":"C:\\Tools\\MyApp.lnk","target":"C:\\Tools\\myapp.exe"}"#)
            .unwrap();
    assert_eq!(spec.path, "C:\\Tools\\MyApp.lnk");
    assert_eq!(spec.target, "C:\\Tools\\myapp.exe");
    assert_eq!(spec.args, None);
    assert_eq!(spec.working_dir, None);
    assert_eq!(spec.icon, None);
    assert_eq!(spec.icon_index, 0);
    assert_eq!(spec.description, None);
    assert_eq!(spec.app_id, None);
}

#[test]
fn create_spec_full() {
    let spec = parse_create_spec(concat!(
        "{\"path\":\"D:\\\\Apps\\\\Thing.lnk\",",
        "\"target\":\"D:\\\\Apps\\\\thing.exe\",",
        "\"args\":\"--mode fast\",\"working_dir\":\"D:\\\\Apps\",",
        "\"icon\":\"D:\\\\Apps\\\\thing.exe\",\"icon_index\":-3,",
        "\"description\":\"The Thing\",\"app_id\":\"Acme.Thing\"}"
    ))
    .unwrap();
    assert_eq!(spec.args.as_deref(), Some("--mode fast"));
    assert_eq!(spec.working_dir.as_deref(), Some("D:\\Apps"));
    assert_eq!(spec.icon.as_deref(), Some("D:\\Apps\\thing.exe"));
    assert_eq!(spec.icon_index, -3); // negative index = a resource id; kept
    assert_eq!(spec.description.as_deref(), Some("The Thing"));
    assert_eq!(spec.app_id.as_deref(), Some("Acme.Thing"));
}

#[test]
fn create_spec_accepts_path_forms() {
    // Forward-slash drive paths and UNC paths are absolute.
    assert!(
        parse_create_spec(r#"{"path":"C:/Tools/MyApp.lnk","target":"C:/Tools/myapp.exe"}"#).is_ok()
    );
    assert!(parse_create_spec(
        r#"{"path":"\\\\server\\share\\MyApp.lnk","target":"\\\\server\\share\\myapp.exe"}"#
    )
    .is_ok());
    // The .lnk suffix check is case-insensitive.
    assert!(parse_create_spec(
        r#"{"path":"C:\\Tools\\MyApp.LNK","target":"C:\\Tools\\myapp.exe"}"#
    )
    .is_ok());
}

#[test]
fn create_spec_rejects_bad_paths() {
    // Missing required fields.
    assert_eq!(create_err(r#"{"target":"C:\\a.exe"}"#).0, ERR_INVALID_ARG);
    assert_eq!(create_err(r#"{"path":"C:\\a.lnk"}"#).0, ERR_INVALID_ARG);
    // Empty target.
    assert!(create_err(r#"{"path":"C:\\a.lnk","target":""}"#)
        .1
        .contains("must not be empty"));
    // Relative, drive-relative and root-relative forms are
    // not absolute paths, for any path field.
    for bad in [
        r#"{"path":"Tools\\a.lnk","target":"C:\\a.exe"}"#,
        r#"{"path":"C:a.lnk","target":"C:\\a.exe"}"#,
        r#"{"path":"\\a.lnk","target":"C:\\a.exe"}"#,
        r#"{"path":"C:\\a.lnk","target":"a.exe"}"#,
        r#"{"path":"C:\\a.lnk","target":"C:a.exe"}"#,
        r#"{"path":"C:\\a.lnk","target":"C:\\a.exe","working_dir":"rel"}"#,
        r#"{"path":"C:\\a.lnk","target":"C:\\a.exe","icon":"rel.ico"}"#,
    ] {
        assert_eq!(create_err(bad).0, ERR_INVALID_ARG, "spec: {bad}");
    }
    // The output must be named .lnk.
    let (code, msg) = create_err(r#"{"path":"C:\\a.txt","target":"C:\\a.exe"}"#);
    assert_eq!(code, ERR_INVALID_ARG);
    assert!(msg.contains(".lnk"), "msg: {msg}");
}

#[test]
fn create_spec_icon_index_rules() {
    // An index with no icon is meaningless.
    let (_, msg) = create_err(r#"{"path":"C:\\a.lnk","target":"C:\\a.exe","icon_index":1}"#);
    assert!(msg.contains("requires \"icon\""), "msg: {msg}");
    // Bounds: the full i32 range is accepted (negative values
    // are resource ids), anything wider is not.
    let ok = parse_create_spec(concat!(
        "{\"path\":\"C:\\\\a.lnk\",\"target\":\"C:\\\\a.exe\",",
        "\"icon\":\"C:\\\\a.exe\",\"icon_index\":2147483647}"
    ))
    .unwrap();
    assert_eq!(ok.icon_index, i32::MAX);
    assert_eq!(
        create_err(concat!(
            "{\"path\":\"C:\\\\a.lnk\",\"target\":\"C:\\\\a.exe\",",
            "\"icon\":\"C:\\\\a.exe\",\"icon_index\":2147483648}"
        ))
        .0,
        ERR_INVALID_ARG
    );
    // A fractional or string index is a type error, not a
    // silent truncation.
    assert_eq!(
        create_err(
            r#"{"path":"C:\\a.lnk","target":"C:\\a.exe","icon":"C:\\a.exe","icon_index":1.5}"#
        )
        .0,
        ERR_INVALID_ARG
    );
    assert_eq!(
        create_err(
            r#"{"path":"C:\\a.lnk","target":"C:\\a.exe","icon":"C:\\a.exe","icon_index":"1"}"#
        )
        .0,
        ERR_INVALID_ARG
    );
}

#[test]
fn create_spec_app_id_and_description_rules() {
    // app_id: empty, over-long and control-char forms fail.
    for bad_app in [
        r#"{"path":"C:\\a.lnk","target":"C:\\a.exe","app_id":""}"#,
        "{\"path\":\"C:\\\\a.lnk\",\"target\":\"C:\\\\a.exe\",\"app_id\":\"a\\u0007b\"}",
    ] {
        assert_eq!(create_err(bad_app).0, ERR_INVALID_ARG, "spec: {bad_app}");
    }
    let long_app = format!(
        "{{\"path\":\"C:\\\\a.lnk\",\"target\":\"C:\\\\a.exe\",\"app_id\":\"{}\"}}",
        "x".repeat(1025)
    );
    assert_eq!(create_err(&long_app).0, ERR_INVALID_ARG);
    // Description: the shell's INFOTIPSIZE limit is a spec
    // error here, not a COM surprise later. 1024 is fine.
    let at_limit = format!(
        "{{\"path\":\"C:\\\\a.lnk\",\"target\":\"C:\\\\a.exe\",\"description\":\"{}\"}}",
        "d".repeat(MAX_DESCRIPTION_CHARS)
    );
    assert!(parse_create_spec(&at_limit).is_ok());
    let over_limit = format!(
        "{{\"path\":\"C:\\\\a.lnk\",\"target\":\"C:\\\\a.exe\",\"description\":\"{}\"}}",
        "d".repeat(MAX_DESCRIPTION_CHARS + 1)
    );
    let (_, msg) = create_err(&over_limit);
    assert!(msg.contains("1024"), "msg: {msg}");
}

#[test]
fn create_spec_rejects_shape_errors() {
    // Unknown fields are rejected.
    let (_, msg) = create_err(r#"{"path":"C:\\a.lnk","target":"C:\\a.exe","bogus":1}"#);
    assert!(msg.contains("unknown field"), "msg: {msg}");
    // Wrong types are rejected.
    assert_eq!(
        create_err(r#"{"path":3,"target":"C:\\a.exe"}"#).0,
        ERR_INVALID_ARG
    );
    assert_eq!(
        create_err(r#"{"path":"C:\\a.lnk","target":"C:\\a.exe","args":true}"#).0,
        ERR_INVALID_ARG
    );
    // Non-object and non-JSON specs are rejected.
    assert_eq!(create_err(r#"["C:\\a.lnk"]"#).0, ERR_INVALID_ARG);
    assert_eq!(create_err("not json").0, ERR_INVALID_ARG);
    // Duplicate keys in one object are a parse error (the
    // family JSON rule), not last-wins.
    assert_eq!(
        create_err(r#"{"path":"C:\\a.lnk","path":"C:\\b.lnk","target":"C:\\a.exe"}"#).0,
        ERR_INVALID_ARG
    );
    // An explicit null optional is the same as omitting it.
    let spec =
        parse_create_spec(r#"{"path":"C:\\a.lnk","target":"C:\\a.exe","args":null,"app_id":null}"#)
            .unwrap();
    assert_eq!(spec.args, None);
    assert_eq!(spec.app_id, None);
}

#[test]
fn read_spec_rules() {
    assert_eq!(
        parse_read_spec(r#"{"path":"C:\\Tools\\MyApp.lnk"}"#).unwrap(),
        "C:\\Tools\\MyApp.lnk"
    );
    // Reading does not demand the .lnk suffix...
    assert!(parse_read_spec(r#"{"path":"C:\\Tools\\shortcut"}"#).is_ok());
    // ...but it does demand an absolute path and nothing else.
    assert_eq!(
        parse_read_spec(r#"{"path":"rel.lnk"}"#).unwrap_err().0,
        ERR_INVALID_ARG
    );
    assert_eq!(parse_read_spec(r#"{}"#).unwrap_err().0, ERR_INVALID_ARG);
    assert_eq!(
        parse_read_spec(r#"{"path":"C:\\a.lnk","app_id":"X"}"#)
            .unwrap_err()
            .0,
        ERR_INVALID_ARG
    );
}

#[test]
fn set_app_id_spec_rules() {
    let (path, app_id) =
        parse_set_app_id_spec(r#"{"path":"C:\\Tools\\MyApp.lnk","app_id":"Acme.MyApp"}"#).unwrap();
    assert_eq!(path, "C:\\Tools\\MyApp.lnk");
    assert_eq!(app_id, "Acme.MyApp");
    // Missing app_id, empty app_id, and a non-.lnk path all
    // fail before any file is touched.
    assert_eq!(
        parse_set_app_id_spec(r#"{"path":"C:\\a.lnk"}"#)
            .unwrap_err()
            .0,
        ERR_INVALID_ARG
    );
    assert_eq!(
        parse_set_app_id_spec(r#"{"path":"C:\\a.lnk","app_id":""}"#)
            .unwrap_err()
            .0,
        ERR_INVALID_ARG
    );
    assert_eq!(
        parse_set_app_id_spec(r#"{"path":"C:\\a.txt","app_id":"X"}"#)
            .unwrap_err()
            .0,
        ERR_INVALID_ARG
    );
    assert_eq!(
        parse_set_app_id_spec(r#"{"path":"C:\\a.lnk","app_id":"X","extra":1}"#)
            .unwrap_err()
            .0,
        ERR_INVALID_ARG
    );
}

// ---------------------------------------------------------------------------
// Read-result shaping.
// ---------------------------------------------------------------------------

fn sample_info() -> LinkInfo {
    LinkInfo {
        target: "C:\\Tools\\myapp.exe".to_string(),
        args: "--flag \"quoted\"".to_string(),
        working_dir: "C:\\Tools".to_string(),
        icon: "C:\\Tools\\myapp.exe".to_string(),
        icon_index: -3,
        description: "My App".to_string(),
        app_id: Some("Acme.MyApp".to_string()),
    }
}

#[test]
fn read_result_shape_full() {
    let out = link_info_json(&sample_info());
    assert_eq!(
        out,
        concat!(
            "{\"target\":\"C:\\\\Tools\\\\myapp.exe\",",
            "\"args\":\"--flag \\\"quoted\\\"\",",
            "\"working_dir\":\"C:\\\\Tools\",",
            "\"icon\":\"C:\\\\Tools\\\\myapp.exe\",",
            "\"icon_index\":-3,",
            "\"description\":\"My App\",",
            "\"app_id\":\"Acme.MyApp\"}"
        )
    );
    // And the shape round-trips through the family parser.
    let v = json::parse(&out).unwrap();
    assert_eq!(v.get("icon_index").unwrap().as_i64(), Some(-3));
}

#[test]
fn read_result_app_id_null_when_unset() {
    let mut info = sample_info();
    info.app_id = None;
    let out = link_info_json(&info);
    let v = json::parse(&out).unwrap();
    assert_eq!(v.get("app_id").unwrap(), &json::Value::Null);
    // Every other field is still present (empty string is the
    // "unset" signal there — only app_id can be null).
    let mut empty = sample_info();
    empty.args = String::new();
    empty.app_id = None;
    let out = link_info_json(&empty);
    let v = json::parse(&out).unwrap();
    assert_eq!(v.get("args").unwrap().as_str(), Some(""));
}

#[test]
fn operations_validate_before_platform() {
    // On every platform a bad spec is an argument error and a
    // good spec reaches the platform layer (NOT_SUPPORTED off
    // Windows, the real work on Windows).
    assert_eq!(create_link("{}").unwrap_err().0, ERR_INVALID_ARG);
    assert_eq!(read_link("{}").unwrap_err().0, ERR_INVALID_ARG);
    assert_eq!(set_app_id("{}").unwrap_err().0, ERR_INVALID_ARG);
    #[cfg(not(windows))]
    {
        assert_eq!(
            create_link(r#"{"path":"C:\\a.lnk","target":"C:\\a.exe"}"#)
                .unwrap_err()
                .0,
            ERR_NOT_SUPPORTED
        );
        assert_eq!(
            read_link(r#"{"path":"C:\\a.lnk"}"#).unwrap_err().0,
            ERR_NOT_SUPPORTED
        );
        assert_eq!(
            set_app_id(r#"{"path":"C:\\a.lnk","app_id":"X"}"#)
                .unwrap_err()
                .0,
            ERR_NOT_SUPPORTED
        );
    }
}
