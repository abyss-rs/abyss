use std::fs;
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Accessor;

use crate::progress::human_bytes;
use crate::storage::Location;

#[derive(Clone, Debug)]
pub struct InspectDialog {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
}

impl InspectDialog {
    pub fn from_location(location: &Location) -> Self {
        let mut lines = Vec::new();
        lines.push(format!("Location:    {}", location.display()));

        let Location::Local(path) = location else {
            lines.push(String::new());
            lines.push("Remote locations show basic URI details.".to_string());
            return Self {
                title: format!(" Inspect: {} ", location.display()),
                lines,
                scroll: 0,
            };
        };

        let title = format!(
            " Inspect: {} ",
            path.file_name()
                .unwrap_or_else(|| path.as_os_str())
                .to_string_lossy()
        );

        if let Ok(metadata) = fs::metadata(path) {
            let file_type = if metadata.is_dir() {
                "Directory"
            } else if metadata.is_symlink() {
                "Symlink"
            } else {
                "Regular File"
            };
            lines.push(format!("Kind:        {}", file_type));
            lines.push(format!(
                "Size:        {} ({} bytes)",
                human_bytes(metadata.len()),
                metadata.len()
            ));

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = metadata.permissions().mode();
                lines.push(format!("Permissions: {:04o}", mode & 0o777));
            }
            #[cfg(not(unix))]
            {
                let readonly = metadata.permissions().readonly();
                lines.push(format!("Read-only:   {}", readonly));
            }

            if let Ok(modified) = metadata.modified() {
                lines.push(format!("Modified:    {}", format_time(modified)));
            }
            if let Ok(created) = metadata.created() {
                lines.push(format!("Created:     {}", format_time(created)));
            }
            if let Ok(accessed) = metadata.accessed() {
                lines.push(format!("Accessed:    {}", format_time(accessed)));
            }
        } else {
            lines.push("Error: Could not read filesystem metadata".to_string());
        }

        let media_file = if path.is_file() {
            Probe::open(path)
                .ok()
                .and_then(|probe| probe.guess_file_type().ok())
                .and_then(|probe| probe.read().ok())
        } else {
            None
        };

        if let Some(tagged_file) = media_file {
            lines.push(String::new());
            lines.push("── Media & Tag Metadata ──".to_string());
            lines.push(format!("Format:      {:?}", tagged_file.file_type()));

            let properties = tagged_file.properties();
            let duration_secs = properties.duration().as_secs();
            let mins = duration_secs / 60;
            let secs = duration_secs % 60;
            if duration_secs >= 3600 {
                let hrs = mins / 60;
                let mins = mins % 60;
                lines.push(format!(
                    "Duration:    {:02}:{:02}:{:02} ({}s)",
                    hrs, mins, secs, duration_secs
                ));
            } else if duration_secs > 0 {
                lines.push(format!(
                    "Duration:    {:02}:{:02} ({}s)",
                    mins, secs, duration_secs
                ));
            }

            if let Some(bitrate) = properties.audio_bitrate() {
                lines.push(format!("Bitrate:     {} kbps", bitrate));
            }
            if let Some(sample_rate) = properties.sample_rate() {
                lines.push(format!("Sample Rate: {} Hz", sample_rate));
            }
            if let Some(channels) = properties.channels() {
                let ch_str = match channels {
                    1 => "1 (Mono)".to_string(),
                    2 => "2 (Stereo)".to_string(),
                    6 => "6 (5.1 Surround)".to_string(),
                    8 => "8 (7.1 Surround)".to_string(),
                    other => format!("{other} channels"),
                };
                lines.push(format!("Channels:    {}", ch_str));
            }

            let tag = tagged_file
                .primary_tag()
                .or_else(|| tagged_file.first_tag());
            if let Some(tag) = tag {
                let mut tag_lines = Vec::new();
                if let Some(title) = tag.title() {
                    tag_lines.push(format!("Title:       {}", title));
                }
                if let Some(artist) = tag.artist() {
                    tag_lines.push(format!("Artist:      {}", artist));
                }
                if let Some(album) = tag.album() {
                    tag_lines.push(format!("Album:       {}", album));
                }
                if let Some(date) = tag.date() {
                    tag_lines.push(format!("Date:        {}", date));
                }
                if let Some(track) = tag.track() {
                    tag_lines.push(format!("Track:       {}", track));
                }
                if let Some(genre) = tag.genre() {
                    tag_lines.push(format!("Genre:       {}", genre));
                }
                if let Some(comment) = tag.comment() {
                    tag_lines.push(format!("Comment:     {}", comment));
                }

                if !tag_lines.is_empty() {
                    lines.push(String::new());
                    lines.push("── Metadata Tags ──".to_string());
                    lines.extend(tag_lines);
                }
            }
        }

        Self {
            title,
            lines,
            scroll: 0,
        }
    }
}

fn format_time(time: SystemTime) -> String {
    let datetime: DateTime<Utc> = time.into();
    datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}
