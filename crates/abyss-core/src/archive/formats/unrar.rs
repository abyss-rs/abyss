use std::collections::HashSet;
use std::io::Write;
use std::path::Path;

use crate::archive::formats::external::{
    extract_external_member, list_external_archive, read_selected_external,
};
use crate::archive::types::{ArchiveIndex, ArchiveMember, ArchiveOpenError};

pub(crate) fn list_rar(
    path: &Path,
    password: Option<&str>,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    list_external_archive(path, password)
}

pub(crate) fn extract_rar(
    source: &Path,
    member_path: &str,
    password: Option<&str>,
    output: &mut impl Write,
) -> Result<u64, ArchiveOpenError> {
    extract_external_member(source, member_path, password, output)
}

pub(crate) fn read_selected_rar(
    index: &ArchiveIndex,
    selected: &HashSet<String>,
    password: Option<&str>,
    consume: impl FnMut(&ArchiveMember, &mut dyn std::io::Read) -> Result<(), ArchiveOpenError>,
) -> Result<(), ArchiveOpenError> {
    read_selected_external(index, selected, password, consume)
}
