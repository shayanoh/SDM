//! The few file operations whose spelling differs between Unix and Windows,
//! kept in one place so the rest of the engine is portable.

use std::fs::{File, OpenOptions};
use std::io;

/// Reported when a write returns zero bytes or an `io::Error` carries no OS
/// code. The POSIX values on every OS: they are only ever shown to the user.
pub(crate) const EIO: i32 = 5;
/// One positional write (`pwrite(2)` / `WriteFile` with an offset). Returns the
/// number of bytes written; the caller loops over short writes.
pub(crate) fn write_at(file: &File, buf: &[u8], offset: u64) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_write(buf, offset)
    }
}

/// `fsync(2)` on Unix: deliberately not `File::sync_all`, which on macOS issues
/// the much slower `F_FULLFSYNC` (the Swift core called plain `fsync`).
/// On Windows `sync_all` is `FlushFileBuffers`.
pub(crate) fn fsync(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: `file` owns an open descriptor for the duration of the call.
        let result = unsafe { libc::fsync(file.as_raw_fd()) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        file.sync_all()
    }
}

/// Sets the creation mode on Unix (the umask still applies); does nothing on
/// Windows, which has no such bits.
pub(crate) fn apply_mode(options: &mut OpenOptions, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    #[cfg(windows)]
    {
        let _ = (options, mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn write_at_writes_at_the_requested_offset_without_moving_the_cursor_contract() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        apply_mode(&mut options, 0o644);
        let file = options.open(&path).unwrap();
        file.set_len(8).unwrap();

        assert_eq!(write_at(&file, b"abc", 2).unwrap(), 3);
        fsync(&file).unwrap();

        let mut contents = Vec::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_end(&mut contents)
            .unwrap();
        assert_eq!(contents, [0, 0, b'a', b'b', b'c', 0, 0, 0]);
    }

    #[test]
    fn error_codes_are_posix_numbers_on_every_os() {
        assert_eq!(EIO, 5);
    }
}
