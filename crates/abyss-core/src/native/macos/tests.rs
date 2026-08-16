use std::fs;
use std::mem::zeroed;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::symlink;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::clone::clone_supported;
use super::copyfile::{CallbackContext, copy_callback};
use super::temp::{Temporary, replace};
use super::{
    CloneCapabilities, copy_regular_file, recover_unremovable_directory, remove_directory_tree,
};
use crate::inventory::Inventory;
use crate::progress::CopyStats;
use crate::test_support::TempDir;

#[test]
fn clone_capability_must_be_valid_and_enabled() {
    // SAFETY: This C value is valid when zero-initialized.
    let mut capabilities: libc::vol_capabilities_attr_t = unsafe { zeroed() };
    let index = libc::VOL_CAPABILITIES_INTERFACES;

    capabilities.capabilities[index] |= libc::VOL_CAP_INT_CLONE;
    assert!(!clone_supported(&capabilities));

    capabilities.valid[index] |= libc::VOL_CAP_INT_CLONE;
    assert!(clone_supported(&capabilities));

    capabilities.capabilities[index] &= !libc::VOL_CAP_INT_CLONE;
    assert!(!clone_supported(&capabilities));
}

#[test]
fn native_recursive_removal_does_not_follow_symlinks() {
    let temp = TempDir::new();
    let outside = temp.path().join("outside");
    let selected = temp.path().join("selected");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("keep"), b"safe").unwrap();
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("delete"), b"gone").unwrap();
    symlink(&outside, selected.join("link")).unwrap();

    remove_directory_tree(&selected).unwrap();

    assert!(!selected.exists());
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"safe");
}

#[test]
fn recovery_renames_a_directory_to_ascii_before_removing_it() {
    let temp = TempDir::new();
    let selected = temp.path().join("Ты — мой триумф [Flarrow Films]");
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("content"), b"gone").unwrap();

    recover_unremovable_directory(&selected).unwrap();

    assert!(!selected.exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn temporary_cleanup_preserves_appledouble_companion() {
    let temp = TempDir::new();
    let temporary = temp.path().join(".abyss.temporary");
    let companion = temp.path().join("._.abyss.temporary");
    fs::write(&temporary, b"partial").unwrap();
    fs::write(&companion, [0x00, 0x05, 0x16, 0x07]).unwrap();

    drop(Temporary::new(temporary.clone()));

    assert!(!temporary.exists());
    assert!(companion.exists());
}

#[test]
fn installing_a_temporary_file_preserves_its_appledouble_companion() {
    let temp = TempDir::new();
    let temporary = temp.path().join(".abyss.temporary");
    let companion = temp.path().join("._.abyss.temporary");
    let destination = temp.path().join("destination");
    fs::write(&temporary, b"complete").unwrap();
    fs::write(&companion, [0x00, 0x05, 0x16, 0x07]).unwrap();

    replace(Temporary::new(temporary), &destination).unwrap();

    assert_eq!(fs::read(destination).unwrap(), b"complete");
    assert!(companion.exists());
}

#[test]
fn streams_without_attempting_a_clone_when_capability_is_disabled() {
    let temp = TempDir::new();
    let source = temp.path().join("source");
    let destination = temp.path().join("destination");
    fs::write(&source, vec![7_u8; 128 * 1024]).unwrap();
    let inventory = Inventory::scan(&source, &AtomicBool::new(false)).unwrap();
    let entry = &inventory.entries[0];
    let destination_device = fs::metadata(temp.path()).unwrap().dev();
    let mut capabilities = CloneCapabilities::default();
    capabilities
        .by_destination_device
        .insert(destination_device, false);
    let stats = Arc::new(CopyStats::default());

    let outcome = copy_regular_file(
        &source,
        &destination,
        entry,
        &Arc::new(AtomicBool::new(false)),
        &stats,
        &mut capabilities,
    )
    .unwrap();

    assert!(!outcome.cloned);
    assert_eq!(outcome.physical_bytes, 128 * 1024);
    assert_eq!(fs::read(destination).unwrap(), vec![7_u8; 128 * 1024]);
}

#[test]
fn callback_quits_immediately_when_cancelled() {
    let cancelled = AtomicBool::new(true);
    let copied = std::sync::atomic::AtomicU64::new(0);
    let mut context = CallbackContext {
        cancelled: &cancelled,
        copied: &copied,
        stats: None,
    };

    let result = copy_callback(
        libc::COPYFILE_COPY_DATA,
        libc::COPYFILE_PROGRESS,
        std::ptr::null_mut(),
        std::ptr::null(),
        std::ptr::null(),
        (&raw mut context).cast(),
    );

    assert_eq!(result, libc::COPYFILE_QUIT);
}

#[test]
fn copies_extended_attributes_with_native_copy() {
    let temp = TempDir::new();
    let source = temp.path().join("source_xattr");
    let destination = temp.path().join("dest_xattr");
    fs::write(&source, b"test payload").unwrap();

    let attr_name = std::ffi::CString::new("com.apple.abyss.test").unwrap();
    let attr_val = b"hello_xattr";
    unsafe {
        libc::setxattr(
            std::ffi::CString::new(source.to_str().unwrap())
                .unwrap()
                .as_ptr(),
            attr_name.as_ptr(),
            attr_val.as_ptr().cast(),
            attr_val.len(),
            0,
            0,
        );
    }

    let inventory = Inventory::scan(&source, &AtomicBool::new(false)).unwrap();
    let entry = &inventory.entries[0];
    let destination_device = fs::metadata(temp.path()).unwrap().dev();
    let mut capabilities = CloneCapabilities::default();
    capabilities
        .by_destination_device
        .insert(destination_device, false);
    let stats = Arc::new(CopyStats::default());

    copy_regular_file(
        &source,
        &destination,
        entry,
        &Arc::new(AtomicBool::new(false)),
        &stats,
        &mut capabilities,
    )
    .unwrap();

    let mut read_buf = vec![0_u8; 32];
    let read_len = unsafe {
        libc::getxattr(
            std::ffi::CString::new(destination.to_str().unwrap())
                .unwrap()
                .as_ptr(),
            attr_name.as_ptr(),
            read_buf.as_mut_ptr().cast(),
            read_buf.len(),
            0,
            0,
        )
    };
    assert_eq!(read_len, attr_val.len() as isize);
    assert_eq!(&read_buf[..attr_val.len()], attr_val);
}
