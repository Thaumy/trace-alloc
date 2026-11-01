use std::io::Write;

use crate::Tracing;
use crate::state::STATE;

pub fn record_loaded_elfs() {
    let Some(_guard) = Tracing::disable() else {
        return;
    };
    let Some(state) = STATE.as_ref() else { return };
    let mut dump = state.dump.lock().expect("failed to get dump lock");
    for (offset, path) in loaded_elfs() {
        let _ = dump.write_all(b"e");
        let _ = dump.write_all(&(offset as u64).to_le_bytes());
        let _ = dump.write_all(&(path.len() as u16).to_le_bytes());
        let _ = dump.write_all(&path);
    }
}

#[cfg(target_os = "linux")]
fn loaded_elfs() -> Vec<(usize, Vec<u8>)> {
    use std::env;
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStringExt;

    unsafe extern "C" fn callback(
        info: *mut libc::dl_phdr_info,
        _: libc::size_t,
        data: *mut libc::c_void,
    ) -> libc::c_int {
        let info = unsafe { &*info };

        let ptr = info.dlpi_name;
        if ptr.is_null() {
            return 0;
        }
        let path = unsafe { CStr::from_ptr(ptr) };
        let path = if path.is_empty() {
            match env::current_exe() {
                Ok(o) => o.into_os_string().into_vec(),
                Err(_) => return 1,
            }
        } else {
            path.to_bytes().to_vec()
        };

        let elfs = unsafe { (data as *mut Vec<(usize, Vec<u8>)>).as_mut_unchecked() };
        elfs.push((info.dlpi_addr as usize, path));

        0
    }

    let mut elfs: Vec<(usize, Vec<u8>)> = Vec::new();
    unsafe {
        libc::dl_iterate_phdr(Some(callback), &mut elfs as *mut _ as _);
    }
    elfs
}

#[cfg(target_os = "macos")]
fn loaded_elfs() -> Vec<(usize, Vec<u8>)> {
    use std::ffi::CStr;

    unsafe extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_name(index: u32) -> *const libc::c_char;
        fn _dyld_get_image_vmaddr_slide(index: u32) -> isize;
    }

    let mut elfs = Vec::new();
    for i in 0..unsafe { _dyld_image_count() } {
        let ptr = unsafe { _dyld_get_image_name(i) };
        if ptr.is_null() {
            continue;
        }

        let path = unsafe { CStr::from_ptr(ptr) };
        let path = path.to_bytes().to_vec();

        let offset = unsafe { _dyld_get_image_vmaddr_slide(i) } as usize;
        elfs.push((offset, path));
    }
    elfs
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn loaded_elfs() -> Vec<(usize, PathBuf)> {
    Vec::new()
}
