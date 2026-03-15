/// POSIX shared-memory bitmap for AFL++-compatible coverage feedback.
///
/// The sandbox creates a SHM region before spawning a target process, sets
/// `__APEX_SHM_NAME` in the child's environment, and reads the bitmap after
/// the process exits. The target must link against the APEX coverage shim
/// (see `shim.rs`) or be compiled with a compatible SanitizerCoverage setup.
use apex_core::error::{ApexError, Result};
use std::ffi::CString;
use uuid::Uuid;

pub const MAP_SIZE: usize = 65_536;
pub const SHM_ENV_VAR: &str = "__APEX_SHM_NAME";

/// Owned POSIX shared-memory bitmap.
pub struct ShmBitmap {
    name: CString,
    ptr: *mut u8,
}

// SAFETY: The pointer is only accessed through &self methods which are
// synchronised by the caller (one bitmap per child process lifetime).
// Send is safe because ownership transfers between threads.
// Sync is intentionally NOT implemented: concurrent reads through the raw
// pointer are not guaranteed safe. The single-writer-after-fork pattern
// means callers must not share &ShmBitmap across threads simultaneously.
unsafe impl Send for ShmBitmap {}

impl ShmBitmap {
    /// Create a new SHM region of `MAP_SIZE` bytes, zeroed.
    pub fn create() -> Result<Self> {
        // macOS PSHMNAMLEN is 31 chars max; truncate UUID to fit.
        let uuid = Uuid::new_v4().simple().to_string();
        let name_str = format!("/apx_{}", &uuid[..16]);
        let name = CString::new(name_str.clone())
            .map_err(|e| ApexError::Sandbox(format!("shm name: {e}")))?;

        unsafe {
            // Open / create
            let fd = libc::shm_open(name.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o600);
            if fd < 0 {
                return Err(ApexError::Sandbox(format!(
                    "shm_open failed: {}",
                    std::io::Error::last_os_error()
                )));
            }

            // Set size
            if libc::ftruncate(fd, MAP_SIZE as libc::off_t) < 0 {
                libc::close(fd);
                libc::shm_unlink(name.as_ptr());
                return Err(ApexError::Sandbox(format!(
                    "ftruncate failed: {}",
                    std::io::Error::last_os_error()
                )));
            }

            // Map
            let ptr = libc::mmap(
                std::ptr::null_mut(),
                MAP_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            libc::close(fd);

            if ptr == libc::MAP_FAILED {
                libc::shm_unlink(name.as_ptr());
                return Err(ApexError::Sandbox(format!(
                    "mmap failed: {}",
                    std::io::Error::last_os_error()
                )));
            }

            // Zero it out.
            std::ptr::write_bytes(ptr as *mut u8, 0, MAP_SIZE);

            Ok(ShmBitmap {
                name,
                ptr: ptr as *mut u8,
            })
        }
    }

    /// The POSIX SHM name to export via `SHM_ENV_VAR`.
    pub fn name_str(&self) -> &str {
        self.name.to_str().unwrap_or("")
    }

    /// Read the full bitmap into a `Vec<u8>`.
    pub fn read(&self) -> Vec<u8> {
        unsafe { std::slice::from_raw_parts(self.ptr, MAP_SIZE).to_vec() }
    }

    /// Zero the bitmap in preparation for the next run.
    pub fn clear(&self) {
        unsafe { std::ptr::write_bytes(self.ptr, 0, MAP_SIZE) }
    }
}

impl Drop for ShmBitmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, MAP_SIZE);
            libc::shm_unlink(self.name.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_bitmap_succeeds() {
        let bm = ShmBitmap::create().expect("ShmBitmap::create should succeed");
        // Name should start with /apex_
        assert!(
            bm.name_str().starts_with("/apx_"),
            "SHM name should start with /apx_: got {}",
            bm.name_str()
        );
    }

    #[test]
    fn bitmap_initially_zeroed() {
        let bm = ShmBitmap::create().unwrap();
        let data = bm.read();
        assert_eq!(data.len(), MAP_SIZE);
        assert!(
            data.iter().all(|&b| b == 0),
            "bitmap should be all zeros after creation"
        );
    }

    #[test]
    fn bitmap_read_returns_correct_size() {
        let bm = ShmBitmap::create().unwrap();
        assert_eq!(bm.read().len(), MAP_SIZE);
    }

    #[test]
    fn bitmap_clear_zeroes_data() {
        let bm = ShmBitmap::create().unwrap();
        // Write some data via the raw pointer (simulates what the shim does).
        unsafe {
            *bm.ptr.add(0) = 42;
            *bm.ptr.add(100) = 7;
        }
        let data = bm.read();
        assert_eq!(data[0], 42);
        assert_eq!(data[100], 7);

        bm.clear();
        let data2 = bm.read();
        assert!(
            data2.iter().all(|&b| b == 0),
            "bitmap should be zeroed after clear()"
        );
    }

    #[test]
    fn bitmap_write_and_read_roundtrip() {
        let bm = ShmBitmap::create().unwrap();
        // Simulate the shim writing coverage hits.
        unsafe {
            for i in 0..256 {
                *bm.ptr.add(i) = (i % 256) as u8;
            }
        }
        let data = bm.read();
        for i in 0..256 {
            assert_eq!(data[i], (i % 256) as u8);
        }
    }

    #[test]
    fn each_bitmap_gets_unique_name() {
        let bm1 = ShmBitmap::create().unwrap();
        let bm2 = ShmBitmap::create().unwrap();
        assert_ne!(
            bm1.name_str(),
            bm2.name_str(),
            "each bitmap should have a unique SHM name"
        );
    }

    #[test]
    fn map_size_is_65536() {
        assert_eq!(MAP_SIZE, 65_536);
    }

    #[test]
    fn shm_env_var_name() {
        assert_eq!(SHM_ENV_VAR, "__APEX_SHM_NAME");
    }

    #[test]
    fn drop_cleans_up_shm() {
        let name;
        {
            let bm = ShmBitmap::create().unwrap();
            name = bm.name_str().to_owned();
        }
        // After drop, shm_open should fail (the region was unlinked).
        let c_name = std::ffi::CString::new(name).unwrap();
        let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDONLY, 0) };
        assert!(fd < 0, "SHM region should be unlinked after drop");
    }

    #[test]
    fn bitmap_write_last_byte() {
        let bm = ShmBitmap::create().unwrap();
        unsafe {
            *bm.ptr.add(MAP_SIZE - 1) = 0xFF;
        }
        let data = bm.read();
        assert_eq!(data[MAP_SIZE - 1], 0xFF);
    }

    #[test]
    fn multiple_reads_are_consistent() {
        let bm = ShmBitmap::create().unwrap();
        unsafe {
            *bm.ptr.add(512) = 99;
        }
        let r1 = bm.read();
        let r2 = bm.read();
        assert_eq!(r1, r2);
    }

    #[test]
    fn clear_after_multiple_writes() {
        let bm = ShmBitmap::create().unwrap();
        unsafe {
            for i in 0..MAP_SIZE {
                *bm.ptr.add(i) = 0xAB;
            }
        }
        bm.clear();
        let data = bm.read();
        assert!(data.iter().all(|&b| b == 0));
    }

    #[test]
    fn name_str_contains_apx_prefix() {
        let bm = ShmBitmap::create().unwrap();
        let name = bm.name_str();
        assert!(name.starts_with("/apx_"), "name: {name}");
        assert!(!name.is_empty());
    }

    #[test]
    fn multiple_creates_all_succeed() {
        let bms: Vec<_> = (0..5).map(|_| ShmBitmap::create().unwrap()).collect();
        // All unique names
        let names: std::collections::HashSet<String> =
            bms.iter().map(|b| b.name_str().to_string()).collect();
        assert_eq!(names.len(), 5);
    }
}
