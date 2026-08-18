use std::collections::HashSet;
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::archive::reader::normalize_member_path;
use crate::archive::types::{ArchiveIndex, ArchiveMember, ArchiveOpenError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalToolKind {
    Unrar(PathBuf),
    SevenZip(PathBuf),
    Unar {
        unar: PathBuf,
        lsar: Option<PathBuf>,
    },
    Rar(PathBuf),
}

impl ExternalToolKind {
    #[allow(dead_code)]
    pub fn name(&self) -> &str {
        match self {
            ExternalToolKind::Unrar(_) => "unrar",
            ExternalToolKind::SevenZip(path) => {
                path.file_name().and_then(|n| n.to_str()).unwrap_or("7z")
            }
            ExternalToolKind::Unar { .. } => "unar",
            ExternalToolKind::Rar(_) => "rar",
        }
    }
}

/// Finds the best available external archive extraction tool on the system PATH.
pub fn discover_tool() -> Result<ExternalToolKind, ArchiveOpenError> {
    // 1. Check unrar
    if let Some(path) = find_binary_on_path("unrar") {
        return Ok(ExternalToolKind::Unrar(path));
    }

    // 2. Check 7zz (modern standalone 7-Zip with built-in RAR codec)
    if let Some(path) = find_binary_on_path("7zz") {
        return Ok(ExternalToolKind::SevenZip(path));
    }

    // 3. Check unar / lsar (The Unarchiver)
    if let Some(unar_path) = find_binary_on_path("unar") {
        let lsar_path = find_binary_on_path("lsar");
        return Ok(ExternalToolKind::Unar {
            unar: unar_path,
            lsar: lsar_path,
        });
    }

    // 4. Check 7z (generic 7-Zip binary)
    if let Some(path) = find_binary_on_path("7z") {
        return Ok(ExternalToolKind::SevenZip(path));
    }

    // 5. Check rar
    if let Some(path) = find_binary_on_path("rar") {
        return Ok(ExternalToolKind::Rar(path));
    }

    Err(ArchiveOpenError::Other(
        "No external archive extractor found. Please install 'unrar', '7zip' (7z/7zz), or 'unar' on your system PATH to unpack this format.".to_string(),
    ))
}

fn find_binary_on_path(binary_name: &str) -> Option<PathBuf> {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(binary_name);
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                let candidate_exe = dir.join(format!("{binary_name}.exe"));
                if candidate_exe.is_file() {
                    return Some(candidate_exe);
                }
            }
        }
    }

    // Common standard fallback locations
    #[cfg(target_os = "macos")]
    let standard_dirs = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];

    #[cfg(target_os = "linux")]
    let standard_dirs = ["/usr/bin", "/usr/local/bin", "/bin", "/snap/bin"];

    #[cfg(windows)]
    let standard_dirs = [
        "C:\\Program Files\\7-Zip",
        "C:\\Program Files (x86)\\7-Zip",
        "C:\\Program Files\\WinRAR",
    ];

    for dir_str in standard_dirs {
        let dir = Path::new(dir_str);
        let candidate = dir.join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate_exe = dir.join(format!("{binary_name}.exe"));
            if candidate_exe.is_file() {
                return Some(candidate_exe);
            }
        }
    }

    None
}

/// Lists all members from an archive using an external CLI unpacker.
pub fn list_external_archive(
    path: &Path,
    password: Option<&str>,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    let tool = discover_tool()?;
    match tool {
        ExternalToolKind::SevenZip(bin) => list_with_7z(&bin, path, password),
        ExternalToolKind::Unrar(bin) | ExternalToolKind::Rar(bin) => {
            list_with_unrar(&bin, path, password)
        }
        ExternalToolKind::Unar { ref lsar, .. } => {
            if let Some(lsar_bin) = lsar {
                list_with_lsar(lsar_bin, path, password)
            } else {
                list_with_unrar_fallback(path, password)
            }
        }
    }
}

fn list_with_7z(
    bin: &Path,
    archive: &Path,
    password: Option<&str>,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    let mut cmd = Command::new(bin);
    cmd.arg("l").arg("-ba").arg("-slt");
    if let Some(pass) = password {
        cmd.arg(format!("-p{pass}"));
    } else {
        cmd.arg("-p-");
    }
    cmd.arg(archive);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| ArchiveOpenError::Other(format!("Failed to execute 7z: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("Wrong password") || stderr.contains("password") {
            return Err(ArchiveOpenError::PasswordRequired(format!(
                "Archive '{}' is encrypted",
                archive.display()
            )));
        }
        return Err(ArchiveOpenError::Other(format!("7z error: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (members, has_encrypted) = parse_7z_slt_output(&stdout)?;

    if has_encrypted {
        match password {
            None => {
                return Err(ArchiveOpenError::PasswordRequired(format!(
                    "Archive '{}' contains encrypted entries",
                    archive.display()
                )));
            }
            Some(pass) => {
                validate_7z_password(bin, archive, pass)?;
            }
        }
    }

    Ok(members)
}

fn validate_7z_password(
    bin: &Path,
    archive: &Path,
    password: &str,
) -> Result<(), ArchiveOpenError> {
    let mut cmd = Command::new(bin);
    cmd.arg("t")
        .arg("-bd")
        .arg(format!("-p{password}"))
        .arg(archive);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| ArchiveOpenError::Other(format!("Failed to validate password: {e}")))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
        if combined.contains("wrong password")
            || combined.contains("data error in encrypted file")
            || combined.contains("crc failed in encrypted file")
            || combined.contains("cannot open encrypted")
            || combined.contains("incorrect password")
        {
            return Err(ArchiveOpenError::InvalidPassword(
                "Invalid password for archive".to_string(),
            ));
        }
        if combined.contains("unsupported")
            || combined.contains("no codec")
            || combined.contains("can not open the file as archive")
        {
            return Err(ArchiveOpenError::Other(format!(
                "7-Zip binary does not support decompressing this archive format: {stderr}"
            )));
        }
        return Err(ArchiveOpenError::InvalidPassword(
            "Invalid password for archive".to_string(),
        ));
    }
    Ok(())
}

fn parse_7z_slt_output(output: &str) -> Result<(Vec<ArchiveMember>, bool), ArchiveOpenError> {
    let mut members = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_size: u64 = 0;
    let mut current_is_dir = false;
    let mut any_encrypted = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some(path_str) = current_path.take()
                && let Some(norm_path) = normalize_member_path(&path_str)
            {
                members.push(ArchiveMember {
                    path: norm_path,
                    size: current_size,
                    is_directory: current_is_dir,
                });
            }
            current_size = 0;
            current_is_dir = false;
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Path" => current_path = Some(value.to_string()),
                "Size" => current_size = value.parse::<u64>().unwrap_or(0),
                "Folder" => current_is_dir = value == "+" || value == "1",
                "Encrypted" => {
                    if value == "+" || value == "1" {
                        any_encrypted = true;
                    }
                }
                "Attributes" if value.starts_with('D') || value.starts_with('d') => {
                    current_is_dir = true;
                }
                _ => {}
            }
        }
    }

    if let Some(path_str) = current_path.take()
        && let Some(norm_path) = normalize_member_path(&path_str)
    {
        members.push(ArchiveMember {
            path: norm_path,
            size: current_size,
            is_directory: current_is_dir,
        });
    }

    Ok((members, any_encrypted))
}

fn list_with_unrar(
    bin: &Path,
    archive: &Path,
    password: Option<&str>,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    let mut cmd = Command::new(bin);
    cmd.arg("lt").arg("-s-");
    if let Some(pass) = password {
        cmd.arg(format!("-p{pass}"));
    } else {
        cmd.arg("-p-");
    }
    cmd.arg(archive);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| ArchiveOpenError::Other(format!("Failed to execute unrar: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stderr.contains("password")
            || stdout.contains("password")
            || stderr.contains("encrypted")
        {
            return Err(ArchiveOpenError::PasswordRequired(format!(
                "Archive '{}' is encrypted",
                archive.display()
            )));
        }
        return Err(ArchiveOpenError::Other(format!("unrar error: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (members, has_encrypted) = parse_unrar_lt_output(&stdout)?;

    if has_encrypted {
        match password {
            None => {
                return Err(ArchiveOpenError::PasswordRequired(format!(
                    "Archive '{}' contains encrypted entries",
                    archive.display()
                )));
            }
            Some(pass) => {
                validate_unrar_password(bin, archive, pass)?;
            }
        }
    }

    Ok(members)
}

fn validate_unrar_password(
    bin: &Path,
    archive: &Path,
    password: &str,
) -> Result<(), ArchiveOpenError> {
    let mut cmd = Command::new(bin);
    cmd.arg("t").arg(format!("-p{password}")).arg(archive);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| ArchiveOpenError::Other(format!("Failed to validate password: {e}")))?;

    if !output.status.success() {
        return Err(ArchiveOpenError::InvalidPassword(
            "Invalid password for archive".to_string(),
        ));
    }
    Ok(())
}

fn parse_unrar_lt_output(output: &str) -> Result<(Vec<ArchiveMember>, bool), ArchiveOpenError> {
    let mut members = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_size: u64 = 0;
    let mut current_is_dir = false;
    let mut any_encrypted = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some(name) = current_name.take()
                && let Some(norm_path) = normalize_member_path(&name)
            {
                members.push(ArchiveMember {
                    path: norm_path,
                    size: current_size,
                    is_directory: current_is_dir,
                });
            }
            current_size = 0;
            current_is_dir = false;
            continue;
        }

        if let Some(stripped) = trimmed.strip_prefix("Name: ") {
            current_name = Some(stripped.to_string());
        } else if let Some(stripped) = trimmed.strip_prefix("Size: ") {
            current_size = stripped.parse::<u64>().unwrap_or(0);
        } else if let Some(stripped) = trimmed.strip_prefix("Type: ") {
            if stripped.eq_ignore_ascii_case("Directory") {
                current_is_dir = true;
            }
        } else if let Some(stripped) = trimmed.strip_prefix("Flags: ") {
            if stripped.contains("encrypted") || stripped.contains("password") {
                any_encrypted = true;
            }
        } else if let Some(stripped) = trimmed.strip_prefix("Attributes: ")
            && (stripped.starts_with('d') || stripped.starts_with('D'))
        {
            current_is_dir = true;
        }
    }

    if let Some(name) = current_name.take()
        && let Some(norm_path) = normalize_member_path(&name)
    {
        members.push(ArchiveMember {
            path: norm_path,
            size: current_size,
            is_directory: current_is_dir,
        });
    }

    Ok((members, any_encrypted))
}

fn list_with_lsar(
    bin: &Path,
    archive: &Path,
    password: Option<&str>,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    let mut cmd = Command::new(bin);
    cmd.arg("-l");
    if let Some(pass) = password {
        cmd.arg("-p").arg(pass);
    }
    cmd.arg(archive);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| ArchiveOpenError::Other(format!("Failed to execute lsar: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ArchiveOpenError::Other(format!("lsar error: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut members = Vec::new();

    // lsar -l output lines: size flags timestamp name
    for line in stdout.lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 4 {
            let size = parts[0].parse::<u64>().unwrap_or(0);
            let flags = parts[1];
            let is_directory = flags.contains('d') || flags.contains('D');
            // The file name is everything after timestamp
            let name = parts[3..].join(" ");
            if let Some(norm_path) = normalize_member_path(&name) {
                members.push(ArchiveMember {
                    path: norm_path,
                    size,
                    is_directory,
                });
            }
        }
    }

    Ok(members)
}

fn list_with_unrar_fallback(
    archive: &Path,
    _password: Option<&str>,
) -> Result<Vec<ArchiveMember>, ArchiveOpenError> {
    Err(ArchiveOpenError::Other(format!(
        "Cannot inspect '{}': 'lsar' or 'unrar' or '7z' required",
        archive.display()
    )))
}

/// Extracts a single archive member to an output stream (e.g. for VFS viewer or file preview).
pub fn extract_external_member(
    source: &Path,
    member_path: &str,
    password: Option<&str>,
    output: &mut impl Write,
) -> Result<u64, ArchiveOpenError> {
    let tool = discover_tool()?;
    match tool {
        ExternalToolKind::SevenZip(bin) => {
            let mut cmd = Command::new(&bin);
            cmd.arg("e").arg("-so").arg("-bd");
            if let Some(pass) = password {
                cmd.arg(format!("-p{pass}"));
            } else {
                cmd.arg("-p-");
            }
            cmd.arg(source).arg(member_path);
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let child = cmd
                .spawn()
                .map_err(|e| ArchiveOpenError::Other(format!("Failed to spawn 7z: {e}")))?;

            let result = child
                .wait_with_output()
                .map_err(|e| ArchiveOpenError::Other(format!("7z extraction failed: {e}")))?;

            if !result.status.success() {
                let stderr = String::from_utf8_lossy(&result.stderr);
                if stderr.contains("Wrong password") {
                    return Err(ArchiveOpenError::PasswordRequired(format!(
                        "Archive member '{member_path}' is encrypted"
                    )));
                }
                return Err(ArchiveOpenError::Other(format!(
                    "7z extraction error: {stderr}"
                )));
            }

            output
                .write_all(&result.stdout)
                .map_err(|e| ArchiveOpenError::Other(e.to_string()))?;
            Ok(result.stdout.len() as u64)
        }
        ExternalToolKind::Unrar(bin) | ExternalToolKind::Rar(bin) => {
            let mut cmd = Command::new(&bin);
            cmd.arg("p").arg("-inul");
            if let Some(pass) = password {
                cmd.arg(format!("-p{pass}"));
            } else {
                cmd.arg("-p-");
            }
            cmd.arg(source).arg(member_path);
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let child = cmd
                .spawn()
                .map_err(|e| ArchiveOpenError::Other(format!("Failed to spawn unrar: {e}")))?;

            let result = child
                .wait_with_output()
                .map_err(|e| ArchiveOpenError::Other(format!("unrar extraction failed: {e}")))?;

            if !result.status.success() {
                let stderr = String::from_utf8_lossy(&result.stderr);
                return Err(ArchiveOpenError::Other(format!(
                    "unrar extraction error: {stderr}"
                )));
            }

            output
                .write_all(&result.stdout)
                .map_err(|e| ArchiveOpenError::Other(e.to_string()))?;
            Ok(result.stdout.len() as u64)
        }
        ExternalToolKind::Unar { unar, .. } => {
            let mut cmd = Command::new(&unar);
            cmd.arg("-o").arg("-");
            if let Some(pass) = password {
                cmd.arg("-p").arg(pass);
            }
            cmd.arg(source).arg(member_path);
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let child = cmd
                .spawn()
                .map_err(|e| ArchiveOpenError::Other(format!("Failed to spawn unar: {e}")))?;

            let result = child
                .wait_with_output()
                .map_err(|e| ArchiveOpenError::Other(format!("unar extraction failed: {e}")))?;

            if !result.status.success() {
                let stderr = String::from_utf8_lossy(&result.stderr);
                return Err(ArchiveOpenError::Other(format!(
                    "unar extraction error: {stderr}"
                )));
            }

            output
                .write_all(&result.stdout)
                .map_err(|e| ArchiveOpenError::Other(e.to_string()))?;
            Ok(result.stdout.len() as u64)
        }
    }
}

/// Reads selected members from an external archive, feeding each member stream to the callback.
pub fn read_selected_external(
    index: &ArchiveIndex,
    selected: &HashSet<String>,
    password: Option<&str>,
    mut consume: impl FnMut(&ArchiveMember, &mut dyn io::Read) -> Result<(), ArchiveOpenError>,
) -> Result<(), ArchiveOpenError> {
    for member in &index.members {
        if !selected.contains(&member.path) || member.is_directory {
            continue;
        }
        let mut buffer = Vec::with_capacity(member.size.min(16 * 1024 * 1024) as usize);
        extract_external_member(&index.source, &member.path, password, &mut buffer)?;
        let mut reader = Cursor::new(buffer);
        consume(member, &mut reader)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_7z_slt_multi_file_and_folders() {
        let sample = r#"
Path = documents/reports/annual.pdf
Size = 1048576
Packed Size = 524288
Modified = 2026-08-18 12:00:00
Attributes = -rw-r--r--
Encrypted = -

Path = documents/reports
Size = 0
Folder = +
Attributes = D -rwxr-xr-x

Path = readme.txt
Size = 42
Folder = -
"#;
        let (members, has_enc) = parse_7z_slt_output(sample).unwrap();
        assert!(!has_enc);
        assert_eq!(members.len(), 3);
        assert_eq!(members[0].path, "documents/reports/annual.pdf");
        assert_eq!(members[0].size, 1048576);
        assert!(!members[0].is_directory);

        assert_eq!(members[1].path, "documents/reports");
        assert_eq!(members[1].size, 0);
        assert!(members[1].is_directory);

        assert_eq!(members[2].path, "readme.txt");
        assert_eq!(members[2].size, 42);
        assert!(!members[2].is_directory);
    }

    #[test]
    fn parses_unrar_lt_output() {
        let sample = r#"
Name: photos/vacation.jpg
Type: File
Size: 2097152
Packed size: 1980000
Attributes: -rw-r--r--
Flags: encrypted

Name: photos
Type: Directory
Size: 0
Attributes: drwxr-xr-x
"#;
        let (members, has_enc) = parse_unrar_lt_output(sample).unwrap();
        assert!(has_enc);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].path, "photos/vacation.jpg");
        assert_eq!(members[0].size, 2097152);
        assert!(!members[0].is_directory);

        assert_eq!(members[1].path, "photos");
        assert!(members[1].is_directory);
    }

    #[test]
    fn discovers_available_system_tool() {
        let tool = discover_tool();
        if let Ok(t) = tool {
            assert!(!t.name().is_empty());
        }
    }
}
