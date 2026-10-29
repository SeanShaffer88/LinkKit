/* LinkKit.dll - Windows shortcut (.lnk) files behind a small
 * C ABI: create a shortcut, read one back, and stamp or replace
 * the AppUserModelID property a shortcut carries.
 *
 * Why the AppUserModelID matters: Windows files an app's
 * taskbar identity, jump list and toast notifications under
 * its Application User Model ID, and for an unpackaged app the
 * durable way Windows learns that id is a shortcut (typically
 * in the Start Menu) stamped with it as a property. Writing
 * that stamp by hand means raw COM - IShellLinkW, IPersistFile
 * and IPropertyStore with positional vtables. LinkKit does it
 * behind three calls.
 *
 * Strings: all strings are UTF-8, NUL-terminated. Specs and
 * results are JSON.
 *
 * Ownership: the char* result of lk_read() is caller-owned and
 * must be freed with lk_free_string(). lk_version() and
 * lk_last_error() return borrowed strings - do NOT free them.
 *
 * Errors: a NULL return (or a nonzero return from lk_create /
 * lk_set_app_id) means failure; lk_last_error() /
 * lk_last_error_code() describe it on the calling thread.
 * HRESULTs from the COM layer are surfaced in the message in
 * hex, and the common shapes are mapped: a missing shortcut
 * file is LK_ERR_NOT_FOUND, a permission refusal is
 * LK_ERR_ACCESS_DENIED.
 *
 * Platforms: spec validation runs on every platform. The
 * operations themselves are Windows-only; elsewhere they fail
 * with LK_ERR_NOT_SUPPORTED after validation.
 *
 * Threading: calls are independent except for the thread-local
 * last-error. COM is initialized per call on the calling
 * thread (apartment-threaded; if the thread already has an
 * apartment of a different model, the calls work in it).
 */
#ifndef LINKKIT_H
#define LINKKIT_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#if defined(_WIN32) && defined(LK_BUILD)
#define LK_API __declspec(dllexport)
#elif defined(_WIN32)
#define LK_API __declspec(dllimport)
#else
#define LK_API
#endif

#define LK_OK 0
#define LK_ERR_INVALID_ARG 1
#define LK_ERR_NOT_SUPPORTED 2
#define LK_ERR_NOT_FOUND 3
#define LK_ERR_ACCESS_DENIED 4
#define LK_ERR_PLATFORM 5
#define LK_ERR_INTERNAL 6

/* Library version, e.g. "0.1.0". Borrowed string; do not free. */
LK_API const char *lk_version(void);

/* Last error message on this thread ("" when none). Borrowed. */
LK_API const char *lk_last_error(void);

/* Last LK_ERR_* code on this thread (LK_OK when none). */
LK_API int lk_last_error_code(void);

/* Free a caller-owned string returned by this library.
 * NULL is accepted and does nothing. */
LK_API void lk_free_string(char *p);

/* Create a shortcut file. Windows only.
 *
 * spec_json: one JSON object:
 *   {"path":"C:\\Users\\me\\AppData\\Roaming\\Microsoft\\Windows\\
 *          Start Menu\\Programs\\MyApp.lnk",   (required; ABSOLUTE
 *          path ending in .lnk - the file to write)
 *    "target":"C:\\Tools\\myapp.exe",          (required; ABSOLUTE
 *          path the shortcut points at. It does not have to
 *          exist yet - installers create shortcuts for files
 *          they are about to lay down)
 *    "args":"--open last",                     (optional)
 *    "working_dir":"C:\\Tools",                (optional; ABSOLUTE)
 *    "icon":"C:\\Tools\\myapp.exe",             (optional; ABSOLUTE)
 *    "icon_index":0,                           (optional; only
 *          with "icon"; must fit an i32; negative values are
 *          resource ids; defaults to 0)
 *    "description":"My App",                   (optional; at most
 *          1024 characters - the shell's own limit, INFOTIPSIZE)
 *    "app_id":"MyCompany.MyApp"}               (optional; stamped
 *          into the file as PKEY_AppUserModel_ID)
 * Unknown fields are rejected; duplicate keys in one JSON
 * object are a parse error. Relative, drive-relative and
 * root-relative paths are rejected for every path field: over
 * a C ABI they would resolve against a current directory the
 * caller cannot see.
 * The folder named by "path" must already exist.
 *
 * Returns LK_OK (0) on success or an LK_ERR_* code; the
 * message is in lk_last_error(). */
LK_API int lk_create(const char *spec_json);

/* Read a shortcut file back. Windows only.
 *
 * spec_json: {"path":"C:\\...\\MyApp.lnk"}   (required; ABSOLUTE
 * path - the .lnk suffix is not demanded for a read)
 *
 * Returns caller-owned result JSON, or NULL + last-error:
 *   {"target":"C:\\Tools\\myapp.exe",
 *    "args":"--open last",
 *    "working_dir":"C:\\Tools",
 *    "icon":"C:\\Tools\\myapp.exe",
 *    "icon_index":0,
 *    "description":"My App",
 *    "app_id":"MyCompany.MyApp"}
 * Every field is always present. A string field the shortcut
 * does not carry reads as "" (the shell's getters cannot
 * distinguish unset from set-to-empty) - except "app_id",
 * which is JSON null when the property is absent, because the
 * property store does know. "target" is the path as stored,
 * without environment-string expansion.
 * A missing file fails with LK_ERR_NOT_FOUND. */
LK_API char *lk_read(const char *spec_json);

/* Stamp or replace the AppUserModelID of an EXISTING shortcut
 * file: load it, set PKEY_AppUserModel_ID through its
 * property store, save it back. Every other field of the
 * shortcut is preserved. Windows only.
 *
 * This is the operation an unpackaged app runs on its Start
 * Menu shortcut so Windows will deliver its toast
 * notifications and file its taskbar identity correctly.
 *
 * spec_json: {"path":"C:\\...\\MyApp.lnk",     (required;
 *              ABSOLUTE, ending in .lnk)
 *             "app_id":"MyCompany.MyApp"}     (required;
 *              non-empty, at most 1024 characters, no control
 *              characters)
 *
 * Returns LK_OK (0) on success or an LK_ERR_* code
 * (LK_ERR_NOT_FOUND when the file does not exist); the
 * message is in lk_last_error(). */
LK_API int lk_set_app_id(const char *spec_json);

#ifdef __cplusplus
}
#endif
#endif
