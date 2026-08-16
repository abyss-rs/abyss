use std::cmp::Ordering;

use crate::browser::types::{BrowserEntry, BrowserKind, SortMode, SortSpec};

pub(crate) fn compare_entries(
    left: &BrowserEntry,
    right: &BrowserEntry,
    spec: SortSpec,
) -> Ordering {
    if left.kind == BrowserKind::Parent || right.kind == BrowserKind::Parent {
        return match (
            left.kind == BrowserKind::Parent,
            right.kind == BrowserKind::Parent,
        ) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => Ordering::Equal,
        };
    }
    let group_directories = spec.directories_first || spec.mode == SortMode::Hybrid;
    if group_directories {
        let ordering = is_directory(right).cmp(&is_directory(left));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    let ordering = match spec.mode {
        SortMode::Hybrid if is_directory(left) && is_directory(right) => compare_names(left, right),
        SortMode::Hybrid => right
            .size
            .unwrap_or(0)
            .cmp(&left.size.unwrap_or(0))
            .then_with(|| compare_names(left, right)),
        SortMode::Name => compare_names(left, right),
        SortMode::Extension => natural_bytes_cmp(extension(left), extension(right))
            .then_with(|| compare_names(left, right)),
        SortMode::Size => right
            .size
            .unwrap_or(0)
            .cmp(&left.size.unwrap_or(0))
            .then_with(|| compare_names(left, right)),
        SortMode::Modified => right
            .modified
            .cmp(&left.modified)
            .then_with(|| compare_names(left, right)),
        SortMode::Unsorted => left.ordinal.cmp(&right.ordinal),
    };
    if spec.reverse {
        ordering.reverse()
    } else {
        ordering
    }
}

pub(crate) fn contextual_sort(entries: &[BrowserEntry], requested: SortSpec) -> SortSpec {
    if requested.mode != SortMode::Hybrid || !is_natural_name_collection(entries) {
        return requested;
    }
    SortSpec {
        mode: SortMode::Name,
        ..requested
    }
}

pub(crate) fn is_natural_name_collection(entries: &[BrowserEntry]) -> bool {
    let mut files = 0_usize;
    let mut episodes = 0_usize;
    let mut numbered = 0_usize;
    for entry in entries {
        if matches!(entry.kind, BrowserKind::Parent | BrowserKind::Directory) {
            continue;
        }
        files += 1;
        episodes += usize::from(looks_like_episode_name(entry.name.as_encoded_bytes()));
        numbered += usize::from(has_numeric_prefix(entry.name.as_encoded_bytes()));
    }
    (episodes >= 8 && episodes.saturating_mul(2) >= files)
        || (numbered >= 3 && numbered.saturating_mul(2) >= files)
}

pub(crate) fn has_numeric_prefix(name: &[u8]) -> bool {
    name.first().is_some_and(u8::is_ascii_digit)
}

pub(crate) fn looks_like_episode_name(name: &[u8]) -> bool {
    for start in 0..name.len() {
        if !name[start].eq_ignore_ascii_case(&b's') {
            continue;
        }
        let mut index = start + 1;
        let season_start = index;
        while index < name.len() && name[index].is_ascii_digit() {
            index += 1;
        }
        if index == season_start {
            continue;
        }
        while index < name.len() && matches!(name[index], b' ' | b'.' | b'_' | b'-') {
            index += 1;
        }
        if index >= name.len() || !name[index].eq_ignore_ascii_case(&b'e') {
            continue;
        }
        index += 1;
        if index < name.len() && name[index].eq_ignore_ascii_case(&b'p') {
            index += 1;
        }
        if index < name.len() && name[index].is_ascii_digit() {
            return true;
        }
    }
    false
}

pub(crate) fn is_directory(entry: &BrowserEntry) -> bool {
    entry.kind == BrowserKind::Directory
}

pub(crate) fn extension(entry: &BrowserEntry) -> &[u8] {
    let bytes = entry.name.as_encoded_bytes();
    if let Some(pos) = bytes
        .iter()
        .rposition(|&b| b == b'.')
        .filter(|&pos| pos > 0 && pos < bytes.len() - 1)
    {
        return &bytes[pos + 1..];
    }
    b""
}

pub(crate) fn compare_names(left: &BrowserEntry, right: &BrowserEntry) -> Ordering {
    let left = left.name.as_encoded_bytes();
    let right = right.name.as_encoded_bytes();
    natural_bytes_cmp(left, right).then_with(|| left.cmp(right))
}

pub(crate) fn natural_bytes_cmp(left: &[u8], right: &[u8]) -> Ordering {
    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() && right_index < right.len() {
        let left_byte = left[left_index];
        let right_byte = right[right_index];
        if left_byte == right_byte {
            left_index += 1;
            right_index += 1;
            continue;
        }

        if left_byte.is_ascii_digit() && right_byte.is_ascii_digit() {
            let left_end = digit_run_end(left, left_index);
            let right_end = digit_run_end(right, right_index);
            let left_significant = trim_numeric_zeros(&left[left_index..left_end]);
            let right_significant = trim_numeric_zeros(&right[right_index..right_end]);
            let ordering = left_significant
                .len()
                .cmp(&right_significant.len())
                .then_with(|| left_significant.cmp(right_significant));
            if ordering != Ordering::Equal {
                return ordering;
            }
            left_index = left_end;
            right_index = right_end;
            continue;
        }

        let ordering = left_byte
            .to_ascii_lowercase()
            .cmp(&right_byte.to_ascii_lowercase());
        if ordering != Ordering::Equal {
            return ordering;
        }
        left_index += 1;
        right_index += 1;
    }
    left_index
        .cmp(&left.len())
        .reverse()
        .then_with(|| right_index.cmp(&right.len()))
}

pub(crate) fn digit_run_end(value: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < value.len() && value[end].is_ascii_digit() {
        end += 1;
    }
    end
}

pub(crate) fn trim_numeric_zeros(value: &[u8]) -> &[u8] {
    let first_nonzero = value
        .iter()
        .position(|byte| *byte != b'0')
        .unwrap_or(value.len());
    &value[first_nonzero..]
}
