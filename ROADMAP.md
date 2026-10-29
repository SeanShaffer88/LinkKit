# LinkKit Roadmap

## v0.1.0 (current)
Shortcut files behind a C ABI with a JSON spec: `lk_create` (target, args, working_dir, icon + icon_index, description, optional `app_id`) via IShellLinkW + IPersistFile::Save, stamping PKEY_AppUserModel_ID through the link's IPropertyStore before the save when an `app_id` is given; `lk_read` returning all fields as JSON with `app_id` null when the property is absent (target read raw, without environment-string expansion); `lk_set_app_id` loading an existing shortcut, stamping/replacing its AppUserModelID, and saving it back with every other field preserved. COM runs over ole32 resolved at runtime — hand-declared vtables verified against the `windows` crate 0.62.2 bindings, no new dependencies or import libraries. All spec validation (absolute paths, `.lnk` output name, icon-index rules, the 1024-character description limit, AppUserModelID shape, unknown-field and duplicate-key rejection) is pure and unit-tested on every platform; off Windows the operations return `LK_ERR_NOT_SUPPORTED` after validation. The COM runtime is Unverified until a field test on a real Windows machine (see QUALITY.md).

## Next, in order
1. **Update-in-place (`lk_update`)**: patch selected fields of an existing shortcut (the create spec's optional fields as a patch, `null` clearing a field) — today changing one field means recreating the shortcut or hand-running create + read + merge.
2. **Hotkey and show command**: expose the shortcut's hotkey and initial window state in create and read (the IShellLinkW slots are already verified; the spec shape — especially how a hotkey is spelled in JSON — needs a decision first).
3. **Resolve check**: an optional read mode that reports whether the stored target still exists (IShellLinkW::Resolve with the no-UI flag), kept separate from plain read so a read never touches more of the shell than it says.

## Explicitly not planned
- Executing or "launching" a shortcut: LinkKit writes and reads files; running one is the caller's shell call.
- Icon extraction or thumbnail rendering: that is ShellInteropKit / ThumbKit territory.
- Start Menu / taskbar management (pinning, enumerating the user's shortcuts): LinkKit operates on the one path a spec names.
