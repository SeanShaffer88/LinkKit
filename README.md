# LinkKit

## TL;DR

LinkKit makes Windows shortcut (`.lnk`) files for you — and, more importantly, it can stamp a shortcut with your program's **AppUserModelID**, the identity Windows uses to file your taskbar button, jump list, and toast notifications under your app. Doing that by hand means raw COM: three interfaces, positional vtables, a property store, and a save, all exactly right or silently wrong. LinkKit reduces it to one call with a small JSON spec: `lk_create` writes a new shortcut (target, arguments, working folder, icon, description, and the app id if you give one), `lk_read` reads a shortcut back as JSON, and `lk_set_app_id` stamps or replaces the AppUserModelID on a shortcut that already exists — the exact errand an unpackaged app runs on its Start Menu shortcut so Windows will deliver its notifications. Specs are validated completely before any COM call happens, on every platform; the work itself is Windows-only and says so honestly elsewhere.

```c
#include "linkkit.h"
#include <stdio.h>

int main(void) {
    /* 1. Create a Start Menu shortcut carrying the app identity. */
    int rc = lk_create(
        "{\"path\":\"C:\\\\Users\\\\me\\\\AppData\\\\Roaming\\\\Microsoft\\\\"
        "Windows\\\\Start Menu\\\\Programs\\\\MyApp.lnk\","
        "\"target\":\"C:\\\\Tools\\\\myapp.exe\","
        "\"description\":\"My App\","
        "\"app_id\":\"MyCompany.MyApp\"}");
    if (rc != LK_OK) { printf("error: %s\n", lk_last_error()); return 1; }

    /* 2. Read it back. */
    char *info = lk_read("{\"path\":\"C:\\\\Users\\\\me\\\\AppData\\\\Roaming\\\\"
                         "Microsoft\\\\Windows\\\\Start Menu\\\\Programs\\\\MyApp.lnk\"}");
    printf("%s\n", info);
    lk_free_string(info);

    /* 3. Re-stamp an existing shortcut after an identity change. */
    rc = lk_set_app_id("{\"path\":\"C:\\\\Users\\\\me\\\\AppData\\\\Roaming\\\\"
                       "Microsoft\\\\Windows\\\\Start Menu\\\\Programs\\\\MyApp.lnk\","
                       "\"app_id\":\"MyCompany.MyApp2\"}");
    return rc;
}
```

## Key features

- **The AppUserModelID stamp, as one call.** `lk_set_app_id` loads an existing shortcut, sets `PKEY_AppUserModel_ID` ({9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, pid 5) through its `IPropertyStore`, commits, and saves — every other field of the shortcut preserved. `lk_create` can stamp at creation time via the spec's `app_id`.
- **Full create.** Target, arguments, working directory, icon + icon index, and description via `IShellLinkW`, persisted with `IPersistFile::Save`.
- **Honest read-back.** `lk_read` returns every field as JSON. A string field the shortcut does not carry reads as `""`, except `app_id`, which is a real JSON `null` when the property is absent — the property store knows the difference even when the link getters don't. The target is reported as stored, without environment-string expansion.
- **Validation before COM, on every platform.** Required fields, unknown fields, absolute paths for every path field (relative, drive-relative and root-relative forms are rejected), the `.lnk` output name, `icon_index` only with an `icon` and within the i32 range, the shell's 1024-character description limit, and the AppUserModelID shape are all checked in the pure core — a bad spec is the same argument error on Linux, in CI, and on the user's machine.
- **Errors that say what happened.** A missing shortcut is `LK_ERR_NOT_FOUND`, a permission refusal is `LK_ERR_ACCESS_DENIED`, and every COM failure carries its HRESULT in hex in the message.
- **Zero dependencies.** Hand-rolled JSON (duplicate keys in one object are a parse error, by design), hand-declared COM vtables checked against the `windows` crate's generated bindings, ole32 resolved at runtime — no new import libraries, small C ABI.

## The API in one screen

| Call | What it does |
|---|---|
| `lk_create(spec_json)` | Create a shortcut file; returns an `LK_*` code |
| `lk_read(spec_json)` | Read a shortcut; returns result JSON (caller-owned) |
| `lk_set_app_id(spec_json)` | Stamp/replace the AppUserModelID of an existing shortcut; returns an `LK_*` code |
| `lk_version` / `lk_last_error` / `lk_last_error_code` / `lk_free_string` | Version, thread-local error, string ownership |

## What a read looks like

```json
{"target":"C:\\Tools\\myapp.exe","args":"--open last","working_dir":"C:\\Tools","icon":"C:\\Tools\\myapp.exe","icon_index":0,"description":"My App","app_id":"MyCompany.MyApp"}
```

## Build

```sh
cargo build --release   # link_kit.dll / .so / .a + include/linkkit.h
cargo test              # full test suite
```

Rust 1.80+, edition 2021, no dependencies. Spec validation and result shaping build and test on any platform; `lk_create` / `lk_read` / `lk_set_app_id` return `LK_ERR_NOT_SUPPORTED` off Windows.

## Limitations

- The folder named by a create spec's `path` must already exist; LinkKit writes the file, it does not manage folders.
- A shortcut's hotkey and show-command state are not exposed in v0.1 — create writes the shell's defaults, read does not report them.
- String fields read back as `""` when unset (the `IShellLinkW` getters cannot distinguish unset from empty); only `app_id` distinguishes, via the property store.
- Windows runtime behaviour is **Unverified** until a field test: the COM layer compiles and clippies under the `x86_64-pc-windows-msvc` target and a Windows-only integration test (create → read → stamp → read in a temp dir) is in the suite for CI, but no Windows machine has run it yet. See QUALITY.md.
