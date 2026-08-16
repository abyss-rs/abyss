use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;

pub(super) fn copy_xattrs(source: &File, destination: &File) -> io::Result<()> {
    let list_len = unsafe { libc::flistxattr(source.as_raw_fd(), std::ptr::null_mut(), 0) };
    if list_len <= 0 {
        return Ok(());
    }
    let mut list_buf = vec![0_u8; list_len as usize];
    let list_len = unsafe {
        libc::flistxattr(
            source.as_raw_fd(),
            list_buf.as_mut_ptr().cast(),
            list_buf.len(),
        )
    };
    if list_len <= 0 {
        return Ok(());
    }
    list_buf.truncate(list_len as usize);
    let mut val_buf = Vec::new();
    for name_slice in list_buf.split(|&b| b == 0).filter(|s| !s.is_empty()) {
        let mut name = Vec::with_capacity(name_slice.len() + 1);
        name.extend_from_slice(name_slice);
        name.push(0);
        let val_len = unsafe {
            libc::fgetxattr(
                source.as_raw_fd(),
                name.as_ptr().cast(),
                std::ptr::null_mut(),
                0,
            )
        };
        if val_len < 0 {
            continue;
        }
        val_buf.resize(val_len as usize, 0);
        let val_len = unsafe {
            libc::fgetxattr(
                source.as_raw_fd(),
                name.as_ptr().cast(),
                val_buf.as_mut_ptr().cast(),
                val_buf.len(),
            )
        };
        if val_len >= 0 {
            unsafe {
                libc::fsetxattr(
                    destination.as_raw_fd(),
                    name.as_ptr().cast(),
                    val_buf.as_ptr().cast(),
                    val_len as usize,
                    0,
                );
            }
        }
    }
    Ok(())
}
