# LinkKit — Parity (spec vs implementation)

Evidence: the Rust test suite in `src/tests.rs`, `src/ffi.rs`
and `src/json.rs` (18 tests passing on the host at the v0.1.0
gate, plus one Windows-only integration test that compiles
under the `x86_64-pc-windows-msvc` target check) plus the
header contract in `include/linkkit.h`. "Verified" means a
named test exercises the behaviour. The Windows COM backend
compiles under the MSVC target at the gate but has never
executed — every row that needs a real shortcut file or a
real shell is marked accordingly, not claimed.

| Spec feature | Status | Evidence |
|---|---|---|
| Create spec: required `path` + `target`, optional args / working_dir / icon / icon_index / description / app_id | Verified | `create_spec_minimal_defaults`, `create_spec_full` |
| Path validation: absolute only (drive, forward-slash drive and UNC accepted; relative, drive-relative, root-relative rejected) for path, target, working_dir, icon | Verified | `create_spec_accepts_path_forms`, `create_spec_rejects_bad_paths` |
| Output name: create and set_app_id paths must end in `.lnk` (any case); read paths need not | Verified | `create_spec_rejects_bad_paths`, `create_spec_accepts_path_forms`, `read_spec_rules`, `set_app_id_spec_rules` |
| Icon index: requires `icon`, defaults to 0, full i32 range kept (negative = resource id), wider/fractional/string rejected | Verified | `create_spec_icon_index_rules`, `create_spec_full` |
| AppUserModelID validation: non-empty, ≤1024 chars, no control characters (all three specs that carry one) | Verified | `create_spec_app_id_and_description_rules`, `set_app_id_spec_rules` |
| Description limit: 1024 characters (INFOTIPSIZE) accepted, 1025 rejected | Verified | `create_spec_app_id_and_description_rules` |
| Spec shape: unknown fields, wrong types, non-object specs rejected; explicit null optional = omitted | Verified | `create_spec_rejects_shape_errors`, `read_spec_rules`, `set_app_id_spec_rules` |
| JSON engine: duplicate keys are a parse error; escapes/surrogates/depth cap | Verified | `create_spec_rejects_shape_errors` (duplicate key) + the `src/json.rs` suite |
| Read-result shaping: documented field order, exact escaping, icon_index as a number | Verified | `read_result_shape_full` |
| Read-result null semantics: `app_id` is JSON null when unset, every other field always present | Verified | `read_result_app_id_null_when_unset` |
| Validation precedes platform: bad specs are LK_ERR_INVALID_ARG on every platform; valid specs off Windows are LK_ERR_NOT_SUPPORTED | Verified | `operations_validate_before_platform` |
| FFI: null/bad-spec errors, last-error code, free-string null no-op | Verified | `ffi_null_and_bad_spec` |
| FFI off Windows: create/read/set_app_id return LK_ERR_NOT_SUPPORTED | Verified | `ffi_platform_contract_off_windows` |
| Windows create: IShellLinkW setters → IPropertyStore stamp (when app_id given) → IPersistFile::Save | Implemented, Unverified | `src/platform.rs` `com::create_link` compiles + clippies for `x86_64-pc-windows-msvc`; GUIDs and vtable slots checked against the `windows` crate 0.62.2 bindings (sources cited in code). Exercised only by the Windows-only test below, which has never run |
| Windows read: IPersistFile::Load + IShellLinkW getters (target via SLGP_RAWPATH) + IPropertyStore::GetValue for the AppUserModelID, PropVariantClear after copying | Implemented, Unverified | `src/platform.rs` `com::read_link`; same as above |
| Windows set_app_id: Load(STGM_READWRITE) → SetValue(PKEY_AppUserModel_ID) → Commit → Save, other fields preserved | Implemented, Unverified | `src/platform.rs` `com::set_app_id`; same as above |
| Windows integration: create (no app_id) → read back all fields with `app_id: null` → stamp → read back the id with other fields intact → missing file reads as LK_ERR_NOT_FOUND | Implemented, Unverified | `ffi_create_read_stamp_round_trip_on_windows` in `src/ffi.rs` — written for Windows CI; has never run anywhere |

## Known gaps (also in README "Limitations")
- Hotkey and show-command state are not in the v0.1 spec surface (ROADMAP item 2).
- String fields other than `app_id` cannot distinguish unset from empty in a read — an IShellLinkW getter limitation, stated in the header rather than papered over.
- The create spec's folder must already exist; LinkKit does not create directories.
