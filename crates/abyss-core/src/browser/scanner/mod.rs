pub(crate) mod local;
#[cfg(target_os = "macos")]
pub(crate) mod macos;
pub(crate) mod remote;

pub(crate) use self::local::{os_string_from_external, read_directory_streamed};
#[cfg(target_os = "macos")]
pub(crate) use self::macos::filesystem_hides_dot_underscore;
pub(crate) use self::remote::read_remote_directory;

#[cfg(not(target_os = "macos"))]
pub(crate) fn filesystem_hides_dot_underscore(_path: &std::path::Path) -> std::io::Result<bool> {
    Ok(true)
}
