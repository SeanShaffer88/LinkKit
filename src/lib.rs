//! LinkKit — pure logic core. Platform access lives in
//! `platform`; the C ABI lives in `ffi`. Keep this module
//! OS-independent and tested.
//!
//! What the core does:
//! - **Spec parsing + validation** for the three operations
//!   (`create`, `read`, `set_app_id`): a small JSON object in,
//!   a fully validated spec out. Every rule that can be checked
//!   without a machine — required fields, absolute paths, the
//!   `.lnk` output name, icon-index bounds, AppUserModelID
//!   shape, the shell's description length limit — is checked
//!   here, on every platform, before any COM call exists.
//! - **Read-result shaping**: the fields a `platform` read
//!   returns become the documented result JSON here, so the
//!   exact byte shape of `lk_read`'s output is unit-tested
//!   without a shortcut file or a Windows machine.
//!
//! The Windows work itself (IShellLinkW + IPersistFile +
//! IPropertyStore over dynamically resolved ole32) lives in
//! `platform`; off Windows the operations fail with
//! ERR_NOT_SUPPORTED after validation, never with a fake
//! success.

pub mod ffi;
pub mod json;
pub mod platform;

#[cfg(test)]
mod tests;

pub const VERSION: &str = "0.1.0";
pub const VERSION_MAJOR: u32 = 0;
pub const VERSION_MINOR: u32 = 1;
pub const VERSION_PATCH: u32 = 0;

// Error codes (mirror include/linkkit.h).
pub const LK_OK: i32 = 0;
pub const ERR_INVALID_ARG: i32 = 1;
pub const ERR_NOT_SUPPORTED: i32 = 2;
pub const ERR_NOT_FOUND: i32 = 3;
pub const ERR_ACCESS_DENIED: i32 = 4;
pub const ERR_PLATFORM: i32 = 5;
pub const ERR_INTERNAL: i32 = 6;

pub type LkResult<T> = Result<T, (i32, String)>;

fn invalid<T>(msg: impl Into<String>) -> LkResult<T> {
    Err((ERR_INVALID_ARG, msg.into()))
}

/// Escape a string for inclusion inside JSON double quotes.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Specs — the validated shapes the platform layer consumes.
// ---------------------------------------------------------------------------

/// A validated `lk_create` spec.
#[derive(Clone, Debug, PartialEq)]
pub struct CreateSpec {
    /// Where the .lnk file is written (absolute, `.lnk` suffix).
    pub path: String,
    /// What the shortcut points at (absolute path).
    pub target: String,
    pub args: Option<String>,
    pub working_dir: Option<String>,
    pub icon: Option<String>,
    /// Only meaningful with `icon`; validated to fit an i32 and
    /// to never appear without one.
    pub icon_index: i32,
    pub description: Option<String>,
    /// PKEY_AppUserModel_ID to stamp into the file, if any.
    pub app_id: Option<String>,
}

/// The fields of one shortcut, as read back by the platform
/// layer. String fields the link does not carry come back
/// empty (IShellLinkW's getters cannot distinguish "unset"
/// from "set to empty"); `app_id` is the exception — the
/// property store knows whether the property exists, so it is
/// a real `Option`.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkInfo {
    pub target: String,
    pub args: String,
    pub working_dir: String,
    pub icon: String,
    pub icon_index: i32,
    pub description: String,
    pub app_id: Option<String>,
}

/// The longest description IShellLinkW accepts: INFOTIPSIZE
/// (1024) characters. Longer strings are a spec error here
/// rather than a surprise COM failure at create time.
pub const MAX_DESCRIPTION_CHARS: usize = 1024;

// ---------------------------------------------------------------------------
// Field helpers (the family spec-parsing vocabulary).
// ---------------------------------------------------------------------------

fn req_str(v: &json::Value, field: &str) -> LkResult<String> {
    match v.get(field) {
        Some(json::Value::Str(s)) => Ok(s.clone()),
        Some(other) => invalid(format!(
            "\"{field}\" must be a string, got {}",
            other.type_name()
        )),
        None => invalid(format!("missing required field \"{field}\"")),
    }
}

fn opt_str(v: &json::Value, field: &str) -> LkResult<Option<String>> {
    match v.get(field) {
        None | Some(json::Value::Null) => Ok(None),
        Some(json::Value::Str(s)) => Ok(Some(s.clone())),
        Some(other) => invalid(format!(
            "\"{field}\" must be a string, got {}",
            other.type_name()
        )),
    }
}

fn opt_i64(v: &json::Value, field: &str) -> LkResult<Option<i64>> {
    match v.get(field) {
        None | Some(json::Value::Null) => Ok(None),
        Some(other) => match other.as_i64() {
            Some(n) => Ok(Some(n)),
            None => invalid(format!(
                "\"{field}\" must be an integer, got {}",
                other.type_name()
            )),
        },
    }
}

fn reject_unknown(v: &json::Value, allowed: &[&str]) -> LkResult<()> {
    if let Some(pairs) = v.as_object() {
        for (k, _) in pairs {
            if !allowed.contains(&k.as_str()) {
                return invalid(format!("unknown field \"{k}\" in spec"));
            }
        }
    }
    Ok(())
}

fn parse_spec(spec_json: &str) -> LkResult<json::Value> {
    let v = json::parse(spec_json)
        .map_err(|e| (ERR_INVALID_ARG, format!("spec is not valid JSON: {e}")))?;
    if v.as_object().is_none() {
        return invalid("spec must be a JSON object");
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Validators.
// ---------------------------------------------------------------------------

/// An absolute Windows path: a drive path (`C:\...` or
/// `C:/...`) or a UNC path (`\\server\share\...`).
/// Drive-relative (`C:foo`) and root-relative (`\foo`) forms
/// are rejected — over a C ABI there is no meaningful current
/// directory or current drive for them to lean on, and a
/// shortcut written to a surprising place is worse than an
/// error.
fn validate_absolute_path(what: &str, path: &str) -> LkResult<()> {
    if path.is_empty() {
        return invalid(format!("{what} must not be empty"));
    }
    let b = path.as_bytes();
    let drive_absolute = b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b[2] == b'\\' || b[2] == b'/');
    let unc_absolute = path.starts_with("\\\\");
    if !drive_absolute && !unc_absolute {
        return invalid(format!(
            "{what} must be an absolute path (a drive path like \"C:\\\\Tools\\\\app.exe\" or a UNC path), got \"{path}\""
        ));
    }
    Ok(())
}

/// The file a shortcut is written to (or stamped in): an
/// absolute path whose name ends in `.lnk` (any case). A
/// shortcut not named `.lnk` does not behave as one — Explorer
/// keys off the extension — so writing one is a spec error,
/// not a surprise the caller finds later.
fn validate_link_output_path(what: &str, path: &str) -> LkResult<()> {
    validate_absolute_path(what, path)?;
    let lower = path.to_ascii_lowercase();
    if !lower.ends_with(".lnk") {
        return invalid(format!(
            "{what} must name a shortcut file ending in .lnk, got \"{path}\""
        ));
    }
    Ok(())
}

/// An Application User Model ID: a real, non-empty identifier
/// (same rule as the family's jump-list planner). It is not a
/// path and nothing is sanitized into the registry here, but
/// an empty or control-character-laden id can never match the
/// id an app's windows carry.
fn validate_app_id(raw: &str) -> LkResult<String> {
    if raw.is_empty() {
        return invalid("app_id must not be empty");
    }
    if raw.chars().count() > 1024 {
        return invalid("app_id is longer than 1024 characters");
    }
    if raw.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
        return invalid("app_id contains a control character");
    }
    Ok(raw.to_string())
}

// ---------------------------------------------------------------------------
// Spec parsing.
// ---------------------------------------------------------------------------

const CREATE_FIELDS: &[&str] = &[
    "path",
    "target",
    "args",
    "working_dir",
    "icon",
    "icon_index",
    "description",
    "app_id",
];

/// Parse and validate an `lk_create` spec.
pub fn parse_create_spec(spec_json: &str) -> LkResult<CreateSpec> {
    let v = parse_spec(spec_json)?;
    reject_unknown(&v, CREATE_FIELDS)?;
    let path = req_str(&v, "path")?;
    validate_link_output_path("\"path\"", &path)?;
    let target = req_str(&v, "target")?;
    validate_absolute_path("\"target\"", &target)?;
    let args = opt_str(&v, "args")?;
    let working_dir = opt_str(&v, "working_dir")?;
    if let Some(dir) = &working_dir {
        validate_absolute_path("\"working_dir\"", dir)?;
    }
    let icon = opt_str(&v, "icon")?;
    if let Some(icon) = &icon {
        validate_absolute_path("\"icon\"", icon)?;
    }
    let icon_index = opt_i64(&v, "icon_index")?;
    if icon_index.is_some() && icon.is_none() {
        return invalid(
            "\"icon_index\" requires \"icon\": an icon index with no icon path has no effect",
        );
    }
    let icon_index = match icon_index {
        Some(n) => {
            if n < i32::MIN as i64 || n > i32::MAX as i64 {
                return invalid("\"icon_index\" is out of range");
            }
            n as i32
        }
        None => 0,
    };
    let description = opt_str(&v, "description")?;
    if let Some(d) = &description {
        if d.chars().count() > MAX_DESCRIPTION_CHARS {
            return invalid(format!(
                "\"description\" is longer than {MAX_DESCRIPTION_CHARS} characters: the shell's limit (INFOTIPSIZE) for a shortcut description"
            ));
        }
    }
    let app_id = match opt_str(&v, "app_id")? {
        Some(raw) => Some(validate_app_id(&raw)?),
        None => None,
    };
    Ok(CreateSpec {
        path,
        target,
        args,
        working_dir,
        icon,
        icon_index,
        description,
        app_id,
    })
}

const READ_FIELDS: &[&str] = &["path"];

/// Parse and validate an `lk_read` spec; returns the path.
/// Reading requires an absolute path but not the `.lnk`
/// suffix: what IPersistFile can load is the shell's call,
/// and a read changes nothing.
pub fn parse_read_spec(spec_json: &str) -> LkResult<String> {
    let v = parse_spec(spec_json)?;
    reject_unknown(&v, READ_FIELDS)?;
    let path = req_str(&v, "path")?;
    validate_absolute_path("\"path\"", &path)?;
    Ok(path)
}

const SET_APP_ID_FIELDS: &[&str] = &["path", "app_id"];

/// Parse and validate an `lk_set_app_id` spec; returns
/// `(path, app_id)`.
pub fn parse_set_app_id_spec(spec_json: &str) -> LkResult<(String, String)> {
    let v = parse_spec(spec_json)?;
    reject_unknown(&v, SET_APP_ID_FIELDS)?;
    let path = req_str(&v, "path")?;
    validate_link_output_path("\"path\"", &path)?;
    let app_id = validate_app_id(&req_str(&v, "app_id")?)?;
    Ok((path, app_id))
}

// ---------------------------------------------------------------------------
// Read-result shaping.
// ---------------------------------------------------------------------------

/// Shape one read-back into the documented `lk_read` result
/// JSON, in the header's field order. `app_id` is a JSON null
/// when the shortcut carries no AppUserModelID; every other
/// field is always present (empty string when unset).
pub fn link_info_json(info: &LinkInfo) -> String {
    json::obj(vec![
        ("target", json::s(&info.target)),
        ("args", json::s(&info.args)),
        ("working_dir", json::s(&info.working_dir)),
        ("icon", json::s(&info.icon)),
        ("icon_index", json::Value::I64(info.icon_index as i64)),
        ("description", json::s(&info.description)),
        (
            "app_id",
            match &info.app_id {
                Some(id) => json::s(id),
                None => json::Value::Null,
            },
        ),
    ])
    .to_json()
}

// ---------------------------------------------------------------------------
// Operations — validation first (every platform), then the
// platform layer.
// ---------------------------------------------------------------------------

/// Create a shortcut from a JSON spec (Windows only; the
/// spec is validated on every platform first).
pub fn create_link(spec_json: &str) -> LkResult<()> {
    let spec = parse_create_spec(spec_json)?;
    platform::create_link(&spec)
}

/// Read a shortcut from a JSON spec and return the result
/// JSON (Windows only; the spec is validated on every
/// platform first).
pub fn read_link(spec_json: &str) -> LkResult<String> {
    let path = parse_read_spec(spec_json)?;
    let info = platform::read_link(&path)?;
    Ok(link_info_json(&info))
}

/// Stamp or replace the AppUserModelID of an existing
/// shortcut (Windows only; the spec is validated on every
/// platform first).
pub fn set_app_id(spec_json: &str) -> LkResult<()> {
    let (path, app_id) = parse_set_app_id_spec(spec_json)?;
    platform::set_app_id(&path, &app_id)
}
