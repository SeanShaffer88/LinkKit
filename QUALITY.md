# LinkKit — Quality Scorecard

Scored against the v0.1.0 gate run on the development VM:
`cargo fmt --check`, clippy (host + `x86_64-pc-windows-msvc`,
`-D warnings`), and the full test suite — 18/18 passing on the
host, gate run repeated with the same result. (The suite holds
a 19th test, the Windows-only COM integration test, which the
host cannot run; it compiles under the MSVC target check.)
Scores are only claimed where that evidence exists; Windows
runtime behaviour is not claimed from Linux evidence.

| Dimension | Score /5 | Evidence |
|---|---|---|
| Spec validation | 5 | Acceptance and rejection suites for all three specs: required/optional fields, absolute-path forms (drive, forward-slash, UNC accepted; relative, drive-relative, root-relative rejected), the `.lnk` output rule (create + set_app_id), icon-index rules (requires icon, full i32 range, fractional/string rejected), AppUserModelID shape, the 1024-character description limit at and over the line, unknown fields, wrong types, duplicate keys |
| Read-result shaping | 5 | Exact-byte golden JSON for a full read; `app_id` null-vs-string semantics asserted both ways; output round-trips through the family parser |
| FFI contract | 4 | Null and bad-spec calls return LK_ERR_INVALID_ARG with the thread-local code set; off-Windows valid specs return LK_ERR_NOT_SUPPORTED, never a fake success; free-string accepts null. Held back one point: the caller-owned string path (lk_read success) is only exercised by the Windows integration test, which has not run yet |
| API & documentation | 4 | Header documents every export with the spec format, error mapping and ownership; README has a runnable C example. No compiled C consumer test in the suite (FFI is exercised from Rust) — one point held back |
| Code health | 5 | Zero dependencies, fmt + clippy clean on both targets at the gate, no unwinding across the FFI boundary, COM vtables/GUIDs cited to the `windows` crate 0.62.2 bindings in code |
| Platform coverage | 2 | Validation, shaping, the FFI contract and the NOT_SUPPORTED path are fully tested on Linux, and the Windows COM module compiles + clippies under the MSVC target — but no Windows machine or CI run has yet linked the DLL or executed a real create/read/stamp. Honestly low until that happens |

**Total: 83/100**

## Waivers / open items
- **Windows COM runtime is Unverified.** The IShellLinkW / IPersistFile / IPropertyStore sequence (create → setters → property-store stamp → Save; read via Load + getters + GetValue; stamp via Load(STGM_READWRITE) → SetValue → Commit → Save) has never run. Every GUID and vtable slot was checked against the `windows` crate 0.62.2 generated bindings before coding (sources cited in `src/platform.rs`), and every HRESULT failure surfaces in the error message in hex, but only a real Windows run — the CI integration test, then a field test that opens the created shortcut and sees a toast filed under the stamped id — proves it.
- A local Windows cross-link could not be attempted: check/clippy do not link, and the development VM's gate is check-only for the MSVC target. CI's `cargo build --release` on windows-latest is the link gate.
- No dimension is scored on Windows runtime evidence.
