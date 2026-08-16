pub(crate) mod brotli;
pub(crate) mod deflate;
pub(crate) mod lz4;

use futures_util::StreamExt;

use crate::storage::helper_protocol::HelperCompression;
use crate::storage::{ByteStream, TreeEntry, WireProgress};

pub(crate) use self::brotli::{decode_brotli_stream, encode_brotli_stream};
pub(crate) use self::deflate::{decode_deflate_stream, encode_deflate_stream};
pub(crate) use self::lz4::{decode_lz4_stream, encode_lz4_stream};

pub(crate) const LZ4_BLOCK: usize = 256 * 1024;
pub(crate) const BROTLI_BLOCK: usize = 16 * 1024 * 1024;
pub(crate) const DEFLATE_BLOCK: usize = 16 * 1024 * 1024;
pub(crate) const STORED_BLOCK: u32 = 1 << 31;

pub(crate) fn tree_compression(_entries: &[TreeEntry]) -> HelperCompression {
    HelperCompression::Brotli
}

pub(crate) fn count_wire_stream(
    source: ByteStream,
    wire_progress: Option<WireProgress>,
) -> ByteStream {
    Box::pin(source.map(move |result| {
        let chunk = result?;
        if let Some(progress) = &wire_progress {
            progress(chunk.len() as u64);
        }
        Ok(chunk)
    }))
}
