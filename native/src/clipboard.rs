//! Places the generated GIF on the Windows clipboard as a real file (CF_HDROP).
//! This preserves animation when pasting into applications that accept file paste.

use std::{
    mem::size_of,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr,
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{GlobalFree, POINT},
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
    },
    UI::Shell::DROPFILES,
};

const CF_HDROP: u32 = 15;

pub fn set_file(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("clipboard file does not exist: {}", path.display()));
    }
    // Do not canonicalize here. On Windows, canonicalize commonly produces a
    // verbatim \\?\ path, and some desktop apps consuming CF_HDROP still
    // mishandle that representation. Capture paths are normalized before use.
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(|e| e.to_string())?.join(path)
    };
    let mut wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    wide.push(0);
    wide.push(0);

    let payload_bytes = wide.len() * size_of::<u16>();
    let total_bytes = size_of::<DROPFILES>() + payload_bytes;

    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, total_bytes);
        if memory.is_null() {
            return Err("GlobalAlloc failed for clipboard".into());
        }

        let locked = GlobalLock(memory) as *mut u8;
        if locked.is_null() {
            GlobalFree(memory);
            return Err("GlobalLock failed for clipboard".into());
        }

        let header = DROPFILES {
            pFiles: size_of::<DROPFILES>() as u32,
            pt: POINT { x: 0, y: 0 },
            fNC: 0,
            fWide: 1,
        };
        ptr::copy_nonoverlapping(&header as *const DROPFILES as *const u8, locked, size_of::<DROPFILES>());
        ptr::copy_nonoverlapping(
            wide.as_ptr() as *const u8,
            locked.add(size_of::<DROPFILES>()),
            payload_bytes,
        );
        GlobalUnlock(memory);

        let mut opened = false;
        for _ in 0..20 {
            if OpenClipboard(ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
        if !opened {
            GlobalFree(memory);
            return Err("clipboard is busy".into());
        }

        if EmptyClipboard() == 0 {
            CloseClipboard();
            GlobalFree(memory);
            return Err("EmptyClipboard failed".into());
        }

        if SetClipboardData(CF_HDROP, memory).is_null() {
            CloseClipboard();
            GlobalFree(memory);
            return Err("SetClipboardData(CF_HDROP) failed".into());
        }

        // Ownership of `memory` transferred to the system after successful SetClipboardData.
        CloseClipboard();
    }

    Ok(())
}
