//! Platform layer: the only place Windows APIs are touched.
//! On non-Windows targets every operation returns
//! ERR_NOT_SUPPORTED so the spec validation and the FFI
//! contract stay testable anywhere.
//!
//! The Windows backend is COM over ole32 resolved at runtime
//! (LoadLibraryW/GetProcAddress), so this crate adds NO import
//! library dependency for its COM work; kernel32 is declared
//! with the house #[link] rule like every other extern block.

use crate::{CreateSpec, LinkInfo, LkResult};

#[cfg(not(windows))]
fn not_supported<T>(what: &str) -> LkResult<T> {
    Err((
        crate::ERR_NOT_SUPPORTED,
        format!("{what} is only available on Windows (spec validation works on every platform)"),
    ))
}

/// Create a shortcut file from a validated spec.
#[cfg(windows)]
pub fn create_link(spec: &CreateSpec) -> LkResult<()> {
    com::create_link(spec)
}

#[cfg(not(windows))]
pub fn create_link(_spec: &CreateSpec) -> LkResult<()> {
    not_supported("creating a shortcut")
}

/// Read a shortcut file's fields.
#[cfg(windows)]
pub fn read_link(path: &str) -> LkResult<LinkInfo> {
    com::read_link(path)
}

#[cfg(not(windows))]
pub fn read_link(_path: &str) -> LkResult<LinkInfo> {
    not_supported("reading a shortcut")
}

/// Stamp or replace the AppUserModelID in an existing
/// shortcut file.
#[cfg(windows)]
pub fn set_app_id(path: &str, app_id: &str) -> LkResult<()> {
    com::set_app_id(path, app_id)
}

#[cfg(not(windows))]
pub fn set_app_id(_path: &str, _app_id: &str) -> LkResult<()> {
    not_supported("stamping an AppUserModelID")
}

// ---------------------------------------------------------------------------
// The shortcut COM surface: IShellLinkW + IPersistFile +
// IPropertyStore.
//
// Everything below was checked against the `windows` crate
// 0.62.2 generated bindings before it was written — vtable
// layouts are ABI, and a wrong slot calls the wrong method
// silently:
//
// - IShellLinkW vtable (Windows::Win32::UI::Shell,
//   IShellLinkW_Vtbl): after IUnknown — GetPath, GetIDList,
//   SetIDList, GetDescription, SetDescription,
//   GetWorkingDirectory, SetWorkingDirectory, GetArguments,
//   SetArguments, GetHotkey, SetHotkey, GetShowCmd,
//   SetShowCmd, GetIconLocation, SetIconLocation,
//   SetRelativePath, Resolve, SetPath. IID
//   000214f9-0000-0000-c000-000000000046; class CLSID_ShellLink
//   00021401-0000-0000-c000-000000000046.
// - IPersistFile vtable (Windows::Win32::System::Com,
//   IPersistFile_Vtbl): base IPersist adds GetClassID, then
//   IsDirty, Load, Save, SaveCompleted, GetCurFile. IID
//   0000010b-0000-0000-c000-000000000046.
// - IPropertyStore vtable (Windows::Win32::UI::Shell::
//   PropertiesSystem, IPropertyStore_Vtbl): GetCount, GetAt,
//   GetValue, SetValue, Commit. IID
//   886d8eeb-8cf2-4446-8d02-cdba1dbdcf99.
// - PKEY_AppUserModel_ID (propkey.h; projected by the crate
//   under Windows::Win32::Storage::EnhancedStorage):
//   {9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, pid 5. A shortcut's
//   AppUserModelID is a VT_LPWSTR (31) under this key.
// - STGM_READ = 0, STGM_READWRITE = 2 (System::Com STGM
//   constants); SLGP_RAWPATH = 4 (UI::Shell SLGP_FLAGS), which
//   makes GetPath return the stored path instead of expanding
//   environment strings in it.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod com {
    use crate::{CreateSpec, LinkInfo, LkResult, ERR_ACCESS_DENIED, ERR_NOT_FOUND, ERR_PLATFORM};
    use std::ffi::c_void;

    type HResult = i32;

    const RPC_E_CHANGED_MODE: HResult = 0x8001_0106u32 as i32;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const CLSCTX_INPROC_SERVER: u32 = 0x1;
    const VT_EMPTY: u16 = 0;
    const VT_BSTR: u16 = 8;
    const VT_LPWSTR: u16 = 31;
    const STGM_READ: u32 = 0;
    const STGM_READWRITE: u32 = 2;
    const SLGP_RAWPATH: u32 = 4;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    const CLSID_SHELL_LINK: Guid = Guid {
        data1: 0x0002_1401,
        data2: 0x0000,
        data3: 0x0000,
        data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
    };
    const IID_SHELL_LINK_W: Guid = Guid {
        data1: 0x0002_14f9,
        data2: 0x0000,
        data3: 0x0000,
        data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
    };
    const IID_PERSIST_FILE: Guid = Guid {
        data1: 0x0000_010b,
        data2: 0x0000,
        data3: 0x0000,
        data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
    };
    const IID_PROPERTY_STORE: Guid = Guid {
        data1: 0x886d_8eeb,
        data2: 0x8cf2,
        data3: 0x4446,
        data4: [0x8d, 0x02, 0xcd, 0xba, 0x1d, 0xbd, 0xcf, 0x99],
    };

    #[repr(C)]
    struct PropertyKey {
        fmtid: Guid,
        pid: u32,
    }

    /// PKEY_AppUserModel_ID — the property the shell and the
    /// toast platform read to decide which app a shortcut (and
    /// its notifications) belongs to.
    const PKEY_APP_USER_MODEL_ID: PropertyKey = PropertyKey {
        fmtid: Guid {
            data1: 0x9f4c_2855,
            data2: 0x9f79,
            data3: 0x4b39,
            data4: [0xa8, 0xd0, 0xe1, 0xd4, 0x2d, 0xe1, 0xd5, 0xf3],
        },
        pid: 5,
    };

    /// PROPVARIANT with the layout of propidl.h: the VARTYPE,
    /// three reserved words, then the union (the string pointer
    /// for VT_LPWSTR / VT_BSTR). The trailing `usize` pads the
    /// struct to the real PROPVARIANT size on both 32- and
    /// 64-bit (the union's largest member, BLOB, is two words).
    #[repr(C)]
    struct PropVariant {
        vt: u16,
        reserved1: u16,
        reserved2: u16,
        reserved3: u16,
        pwsz_val: *mut u16,
        _pad: usize,
    }

    impl PropVariant {
        fn empty() -> PropVariant {
            PropVariant {
                vt: 0,
                reserved1: 0,
                reserved2: 0,
                reserved3: 0,
                pwsz_val: std::ptr::null_mut(),
                _pad: 0,
            }
        }
    }

    // Vtable slots (IUnknown occupies 0..=2 everywhere).
    const SLOT_QUERY_INTERFACE: usize = 0;
    const SLOT_RELEASE: usize = 2;
    // IShellLinkW
    const SL_GET_PATH: usize = 3;
    const SL_GET_DESCRIPTION: usize = 6;
    const SL_SET_DESCRIPTION: usize = 7;
    const SL_GET_WORKING_DIRECTORY: usize = 8;
    const SL_SET_WORKING_DIRECTORY: usize = 9;
    const SL_GET_ARGUMENTS: usize = 10;
    const SL_SET_ARGUMENTS: usize = 11;
    const SL_GET_ICON_LOCATION: usize = 16;
    const SL_SET_ICON_LOCATION: usize = 17;
    const SL_SET_PATH: usize = 20;
    // IPersistFile (IPersist's GetClassID is slot 3)
    const PF_LOAD: usize = 5;
    const PF_SAVE: usize = 6;
    // IPropertyStore
    const PS_GET_VALUE: usize = 5;
    const PS_SET_VALUE: usize = 6;
    const PS_COMMIT: usize = 7;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    type CoInitializeExFn =
        unsafe extern "system" fn(reserved: *mut c_void, co_init: u32) -> HResult;
    type CoUninitializeFn = unsafe extern "system" fn();
    type CoCreateInstanceFn = unsafe extern "system" fn(
        rclsid: *const Guid,
        outer: *mut c_void,
        cls_context: u32,
        riid: *const Guid,
        result: *mut *mut c_void,
    ) -> HResult;
    type PropVariantClearFn = unsafe extern "system" fn(value: *mut PropVariant) -> HResult;

    struct Ole32 {
        co_initialize_ex: CoInitializeExFn,
        co_uninitialize: CoUninitializeFn,
        co_create_instance: CoCreateInstanceFn,
        prop_variant_clear: PropVariantClearFn,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Map an HRESULT onto the kit's error codes: the two
    /// "not there" shapes (storage and Win32) become
    /// ERR_NOT_FOUND, access denied becomes ERR_ACCESS_DENIED,
    /// everything else is a platform failure. The message
    /// always carries the raw HRESULT in hex.
    fn hr_err(what: &str, hr: HResult) -> (i32, String) {
        let code = match hr as u32 {
            0x8003_0002 | 0x8007_0002 => ERR_NOT_FOUND,
            0x8007_0005 => ERR_ACCESS_DENIED,
            _ => ERR_PLATFORM,
        };
        (code, format!("{what} failed: HRESULT 0x{:08X}", hr as u32))
    }

    /// COM's own success test: SUCCEEDED(hr) is hr >= 0, so
    /// S_FALSE from a getter is success with an empty answer,
    /// not a failure.
    fn check(what: &str, hr: HResult) -> LkResult<()> {
        if hr >= 0 {
            Ok(())
        } else {
            Err(hr_err(what, hr))
        }
    }

    fn load_ole32() -> LkResult<Ole32> {
        let name = wide("ole32.dll");
        let module = unsafe { LoadLibraryW(name.as_ptr()) };
        if module.is_null() {
            return Err((
                ERR_PLATFORM,
                "could not load ole32.dll to reach the COM runtime".to_string(),
            ));
        }
        // The module stays loaded for the process lifetime by
        // design: the resolved pointers must outlive every call
        // below, and ole32 is a system DLL that is always safe
        // to keep mapped.
        unsafe fn resolve<T>(module: *mut c_void, proc: &[u8]) -> LkResult<T> {
            let p = unsafe { GetProcAddress(module, proc.as_ptr()) };
            if p.is_null() {
                return Err((
                    ERR_PLATFORM,
                    format!(
                        "ole32.dll has no {} entry point",
                        String::from_utf8_lossy(&proc[..proc.len() - 1])
                    ),
                ));
            }
            Ok(unsafe { std::mem::transmute_copy(&p) })
        }
        let co_initialize_ex: CoInitializeExFn = unsafe { resolve(module, b"CoInitializeEx\0") }?;
        let co_uninitialize: CoUninitializeFn = unsafe { resolve(module, b"CoUninitialize\0") }?;
        let co_create_instance: CoCreateInstanceFn =
            unsafe { resolve(module, b"CoCreateInstance\0") }?;
        let prop_variant_clear: PropVariantClearFn =
            unsafe { resolve(module, b"PropVariantClear\0") }?;
        Ok(Ole32 {
            co_initialize_ex,
            co_uninitialize,
            co_create_instance,
            prop_variant_clear,
        })
    }

    /// A COM apartment for one operation. When the calling
    /// thread already has an apartment of a DIFFERENT model,
    /// CoInitializeEx reports RPC_E_CHANGED_MODE: COM calls
    /// still work in the existing apartment, so proceed — but
    /// that initialization is not ours, so it is not balanced
    /// with a CoUninitialize on drop.
    struct ComSession {
        ole: Ole32,
        uninit_owed: bool,
    }

    impl ComSession {
        fn new() -> LkResult<ComSession> {
            let ole = load_ole32()?;
            let hr =
                unsafe { (ole.co_initialize_ex)(std::ptr::null_mut(), COINIT_APARTMENTTHREADED) };
            if hr >= 0 {
                // S_OK or S_FALSE: this call initialized (or
                // counted) the apartment; balance it on drop.
                Ok(ComSession {
                    ole,
                    uninit_owed: true,
                })
            } else if hr == RPC_E_CHANGED_MODE {
                Ok(ComSession {
                    ole,
                    uninit_owed: false,
                })
            } else {
                Err(hr_err("CoInitializeEx", hr))
            }
        }

        fn create(&self, clsid: &Guid, iid: &Guid, what: &str) -> LkResult<ComPtr> {
            let mut raw: *mut c_void = std::ptr::null_mut();
            let hr = unsafe {
                (self.ole.co_create_instance)(
                    clsid,
                    std::ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    iid,
                    &mut raw,
                )
            };
            check(&format!("CoCreateInstance({what})"), hr)?;
            if raw.is_null() {
                return Err((
                    ERR_PLATFORM,
                    format!("CoCreateInstance({what}) returned a null interface"),
                ));
            }
            Ok(ComPtr(raw))
        }
    }

    impl Drop for ComSession {
        fn drop(&mut self) {
            if self.uninit_owed {
                unsafe { (self.ole.co_uninitialize)() }
            }
        }
    }

    /// An owned interface pointer: released through its vtable
    /// on drop.
    struct ComPtr(*mut c_void);

    impl Drop for ComPtr {
        fn drop(&mut self) {
            unsafe { release(self.0) }
        }
    }

    unsafe fn vtable(p: *mut c_void) -> *const *const c_void {
        unsafe { *(p as *const *const *const c_void) }
    }

    unsafe fn slot<T>(p: *mut c_void, index: usize) -> T {
        unsafe {
            let entry = *vtable(p).add(index);
            std::mem::transmute_copy(&entry)
        }
    }

    unsafe fn release(p: *mut c_void) {
        if !p.is_null() {
            type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
            let f: ReleaseFn = unsafe { slot(p, SLOT_RELEASE) };
            unsafe { f(p) };
        }
    }

    unsafe fn query_interface(p: *mut c_void, iid: &Guid, what: &str) -> LkResult<ComPtr> {
        type QueryInterfaceFn =
            unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HResult;
        let f: QueryInterfaceFn = unsafe { slot(p, SLOT_QUERY_INTERFACE) };
        let mut raw: *mut c_void = std::ptr::null_mut();
        let hr = unsafe { f(p, iid, &mut raw) };
        check(&format!("QueryInterface({what})"), hr)?;
        if raw.is_null() {
            return Err((
                ERR_PLATFORM,
                format!("QueryInterface({what}) returned a null interface"),
            ));
        }
        Ok(ComPtr(raw))
    }

    // -- IShellLinkW setters -------------------------------------------------

    unsafe fn sl_set_path(link: *mut c_void, path: *const u16) -> HResult {
        type Fn = unsafe extern "system" fn(*mut c_void, *const u16) -> HResult;
        let f: Fn = unsafe { slot(link, SL_SET_PATH) };
        unsafe { f(link, path) }
    }

    unsafe fn sl_set_working_directory(link: *mut c_void, dir: *const u16) -> HResult {
        type Fn = unsafe extern "system" fn(*mut c_void, *const u16) -> HResult;
        let f: Fn = unsafe { slot(link, SL_SET_WORKING_DIRECTORY) };
        unsafe { f(link, dir) }
    }

    unsafe fn sl_set_arguments(link: *mut c_void, args: *const u16) -> HResult {
        type Fn = unsafe extern "system" fn(*mut c_void, *const u16) -> HResult;
        let f: Fn = unsafe { slot(link, SL_SET_ARGUMENTS) };
        unsafe { f(link, args) }
    }

    unsafe fn sl_set_description(link: *mut c_void, description: *const u16) -> HResult {
        type Fn = unsafe extern "system" fn(*mut c_void, *const u16) -> HResult;
        let f: Fn = unsafe { slot(link, SL_SET_DESCRIPTION) };
        unsafe { f(link, description) }
    }

    unsafe fn sl_set_icon_location(link: *mut c_void, icon: *const u16, index: i32) -> HResult {
        type Fn = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HResult;
        let f: Fn = unsafe { slot(link, SL_SET_ICON_LOCATION) };
        unsafe { f(link, icon, index) }
    }

    // -- IShellLinkW getters ---------------------------------------------------

    /// Buffer size for the string getters: 4096 UTF-16 code
    /// units covers long-path targets with room to spare.
    const GETTER_CHARS: usize = 4096;

    fn buffer_to_string(buf: &[u16]) -> String {
        let end = buf.iter().position(|&u| u == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    unsafe fn sl_get_path(link: *mut c_void) -> LkResult<String> {
        type Fn =
            unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut c_void, u32) -> HResult;
        let f: Fn = unsafe { slot(link, SL_GET_PATH) };
        let mut buf = vec![0u16; GETTER_CHARS];
        // SLGP_RAWPATH: return the stored path as-is rather
        // than expanding environment strings in it, so a read
        // reports what the file actually says.
        let hr = unsafe {
            f(
                link,
                buf.as_mut_ptr(),
                GETTER_CHARS as i32,
                std::ptr::null_mut(),
                SLGP_RAWPATH,
            )
        };
        check("IShellLinkW::GetPath", hr)?;
        Ok(buffer_to_string(&buf))
    }

    unsafe fn sl_get_string(link: *mut c_void, slot_index: usize, what: &str) -> LkResult<String> {
        type Fn = unsafe extern "system" fn(*mut c_void, *mut u16, i32) -> HResult;
        let f: Fn = unsafe { slot(link, slot_index) };
        let mut buf = vec![0u16; GETTER_CHARS];
        let hr = unsafe { f(link, buf.as_mut_ptr(), GETTER_CHARS as i32) };
        check(what, hr)?;
        Ok(buffer_to_string(&buf))
    }

    unsafe fn sl_get_icon_location(link: *mut c_void) -> LkResult<(String, i32)> {
        type Fn = unsafe extern "system" fn(*mut c_void, *mut u16, i32, *mut i32) -> HResult;
        let f: Fn = unsafe { slot(link, SL_GET_ICON_LOCATION) };
        let mut buf = vec![0u16; GETTER_CHARS];
        let mut index: i32 = 0;
        let hr = unsafe { f(link, buf.as_mut_ptr(), GETTER_CHARS as i32, &mut index) };
        check("IShellLinkW::GetIconLocation", hr)?;
        Ok((buffer_to_string(&buf), index))
    }

    // -- IPersistFile ---------------------------------------------------------

    unsafe fn pf_load(persist: *mut c_void, path: *const u16, mode: u32) -> HResult {
        type Fn = unsafe extern "system" fn(*mut c_void, *const u16, u32) -> HResult;
        let f: Fn = unsafe { slot(persist, PF_LOAD) };
        unsafe { f(persist, path, mode) }
    }

    unsafe fn pf_save(persist: *mut c_void, path: *const u16) -> HResult {
        // BOOL is a 4-byte int in the Win32 ABI; TRUE = 1 makes
        // the saved path the link's current file.
        type Fn = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HResult;
        let f: Fn = unsafe { slot(persist, PF_SAVE) };
        unsafe { f(persist, path, 1) }
    }

    // -- IPropertyStore ---------------------------------------------------------

    unsafe fn ps_set_value(store: *mut c_void, key: &PropertyKey, value: &PropVariant) -> HResult {
        type Fn = unsafe extern "system" fn(
            *mut c_void,
            *const PropertyKey,
            *const PropVariant,
        ) -> HResult;
        let f: Fn = unsafe { slot(store, PS_SET_VALUE) };
        unsafe { f(store, key, value) }
    }

    unsafe fn ps_get_value(
        store: *mut c_void,
        key: &PropertyKey,
        value: *mut PropVariant,
    ) -> HResult {
        type Fn =
            unsafe extern "system" fn(*mut c_void, *const PropertyKey, *mut PropVariant) -> HResult;
        let f: Fn = unsafe { slot(store, PS_GET_VALUE) };
        unsafe { f(store, key, value) }
    }

    unsafe fn ps_commit(store: *mut c_void) -> HResult {
        type Fn = unsafe extern "system" fn(*mut c_void) -> HResult;
        let f: Fn = unsafe { slot(store, PS_COMMIT) };
        unsafe { f(store) }
    }

    /// Read a NUL-terminated UTF-16 string out of a
    /// PROPVARIANT, defensively capped: a corrupt store that
    /// never terminates must not walk memory forever. The
    /// AppUserModelID cap in the spec parser is 1024 chars;
    /// 8192 units is generous headroom for reading one back
    /// that another writer made longer.
    unsafe fn prop_string(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut units = Vec::new();
        for i in 0..8192 {
            let u = unsafe { *p.add(i) };
            if u == 0 {
                break;
            }
            units.push(u);
        }
        String::from_utf16_lossy(&units)
    }

    // -- Shared building blocks -------------------------------------------------

    /// A fresh IShellLinkW object.
    fn new_link(session: &ComSession) -> LkResult<ComPtr> {
        session.create(&CLSID_SHELL_LINK, &IID_SHELL_LINK_W, "IShellLinkW")
    }

    /// Stamp `app_id` as the link's AppUserModelID through its
    /// property store. The commit makes the value part of the
    /// link object's state; the caller's IPersistFile::Save is
    /// what writes it into the file (the sequence MuseDesktop
    /// uses: stamp, then save).
    fn stamp_app_id(link: *mut c_void, app_id: &str) -> LkResult<()> {
        let store = unsafe { query_interface(link, &IID_PROPERTY_STORE, "IPropertyStore") }?;
        let app_id_w = wide(app_id);
        // IPropertyStore::SetValue copies the PROPVARIANT's
        // contents synchronously (the windows crate binding
        // takes it as a `*const PROPVARIANT`), so the string is
        // lent, not handed over: no CoTaskMem allocation, no
        // PropVariantClear owed on this one. The buffer outlives
        // the call because it is a local and SetValue is
        // synchronous.
        let value = PropVariant {
            vt: VT_LPWSTR,
            reserved1: 0,
            reserved2: 0,
            reserved3: 0,
            pwsz_val: app_id_w.as_ptr() as *mut u16,
            _pad: 0,
        };
        check("IPropertyStore::SetValue(PKEY_AppUserModel_ID)", unsafe {
            ps_set_value(store.0, &PKEY_APP_USER_MODEL_ID, &value)
        })?;
        check("IPropertyStore::Commit", unsafe { ps_commit(store.0) })?;
        Ok(())
    }

    /// Read the link's AppUserModelID back: Ok(None) when the
    /// property is absent (VT_EMPTY), the string when present.
    fn read_app_id(session: &ComSession, link: *mut c_void) -> LkResult<Option<String>> {
        let store = unsafe { query_interface(link, &IID_PROPERTY_STORE, "IPropertyStore") }?;
        let mut value = PropVariant::empty();
        check("IPropertyStore::GetValue(PKEY_AppUserModel_ID)", unsafe {
            ps_get_value(store.0, &PKEY_APP_USER_MODEL_ID, &mut value)
        })?;
        // The returned PROPVARIANT is ours: copy what we need,
        // then clear it through ole32 before doing anything
        // else (including erroring).
        let vt = value.vt;
        let text = if vt == VT_LPWSTR || vt == VT_BSTR {
            Some(unsafe { prop_string(value.pwsz_val) })
        } else {
            None
        };
        unsafe { (session.ole.prop_variant_clear)(&mut value) };
        match vt {
            VT_EMPTY => Ok(None),
            VT_LPWSTR | VT_BSTR => Ok(text),
            other => Err((
                ERR_PLATFORM,
                format!(
                    "PKEY_AppUserModel_ID has unexpected variant type {other} (expected VT_LPWSTR {VT_LPWSTR} or VT_EMPTY)"
                ),
            )),
        }
    }

    /// Load `path` into a fresh link object; returns the link
    /// and its IPersistFile (kept alive for a later Save).
    fn load_link(session: &ComSession, path: &str, mode: u32) -> LkResult<(ComPtr, ComPtr)> {
        let link = new_link(session)?;
        let persist = unsafe { query_interface(link.0, &IID_PERSIST_FILE, "IPersistFile") }?;
        let path_w = wide(path);
        check("IPersistFile::Load", unsafe {
            pf_load(persist.0, path_w.as_ptr(), mode)
        })?;
        Ok((link, persist))
    }

    // -- The three operations ---------------------------------------------------

    pub fn create_link(spec: &CreateSpec) -> LkResult<()> {
        let session = ComSession::new()?;
        let link = new_link(&session)?;
        let target = wide(&spec.target);
        check("IShellLinkW::SetPath", unsafe {
            sl_set_path(link.0, target.as_ptr())
        })?;
        if let Some(dir) = &spec.working_dir {
            let dir = wide(dir);
            check("IShellLinkW::SetWorkingDirectory", unsafe {
                sl_set_working_directory(link.0, dir.as_ptr())
            })?;
        }
        if let Some(args) = &spec.args {
            let args = wide(args);
            check("IShellLinkW::SetArguments", unsafe {
                sl_set_arguments(link.0, args.as_ptr())
            })?;
        }
        if let Some(description) = &spec.description {
            let description = wide(description);
            check("IShellLinkW::SetDescription", unsafe {
                sl_set_description(link.0, description.as_ptr())
            })?;
        }
        if let Some(icon) = &spec.icon {
            let icon = wide(icon);
            check("IShellLinkW::SetIconLocation", unsafe {
                sl_set_icon_location(link.0, icon.as_ptr(), spec.icon_index)
            })?;
        }
        if let Some(app_id) = &spec.app_id {
            stamp_app_id(link.0, app_id)?;
        }
        let persist = unsafe { query_interface(link.0, &IID_PERSIST_FILE, "IPersistFile") }?;
        let path = wide(&spec.path);
        check("IPersistFile::Save", unsafe {
            pf_save(persist.0, path.as_ptr())
        })?;
        Ok(())
    }

    pub fn read_link(path: &str) -> LkResult<LinkInfo> {
        let session = ComSession::new()?;
        let (link, _persist) = load_link(&session, path, STGM_READ)?;
        let target = unsafe { sl_get_path(link.0) }?;
        let args = unsafe { sl_get_string(link.0, SL_GET_ARGUMENTS, "IShellLinkW::GetArguments") }?;
        let working_dir = unsafe {
            sl_get_string(
                link.0,
                SL_GET_WORKING_DIRECTORY,
                "IShellLinkW::GetWorkingDirectory",
            )
        }?;
        let description =
            unsafe { sl_get_string(link.0, SL_GET_DESCRIPTION, "IShellLinkW::GetDescription") }?;
        let (icon, icon_index) = unsafe { sl_get_icon_location(link.0) }?;
        let app_id = read_app_id(&session, link.0)?;
        Ok(LinkInfo {
            target,
            args,
            working_dir,
            icon,
            icon_index,
            description,
            app_id,
        })
    }

    pub fn set_app_id(path: &str, app_id: &str) -> LkResult<()> {
        let session = ComSession::new()?;
        let (link, persist) = load_link(&session, path, STGM_READWRITE)?;
        stamp_app_id(link.0, app_id)?;
        let path_w = wide(path);
        check("IPersistFile::Save", unsafe {
            pf_save(persist.0, path_w.as_ptr())
        })?;
        Ok(())
    }
}
