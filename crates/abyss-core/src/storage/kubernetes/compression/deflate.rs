use std::io::Cursor;

use futures_util::StreamExt;

use super::{DEFLATE_BLOCK, STORED_BLOCK};
use crate::storage::{ByteStream, ErrorKind, StorageError, WireProgress};

struct DeflateEncodeState {
    source: ByteStream,
    buffered: bytes::BytesMut,
    eof: bool,
    wire_progress: Option<WireProgress>,
}

pub(crate) fn encode_deflate_stream(
    source: ByteStream,
    wire_progress: Option<WireProgress>,
) -> ByteStream {
    Box::pin(futures_util::stream::unfold(
        DeflateEncodeState {
            source,
            buffered: bytes::BytesMut::with_capacity(DEFLATE_BLOCK),
            eof: false,
            wire_progress,
        },
        |mut state| async move {
            loop {
                if state.buffered.len() >= DEFLATE_BLOCK
                    || (state.eof && !state.buffered.is_empty())
                {
                    let length = state.buffered.len().min(DEFLATE_BLOCK);
                    let block = state.buffered.split_to(length).freeze();
                    let compressed =
                        match tokio::task::spawn_blocking(move || compress_deflate_block(&block))
                            .await
                        {
                            Ok(Ok(compressed)) => compressed,
                            Ok(Err(error)) => return Some((Err(error), state)),
                            Err(error) => {
                                return Some((
                                    Err(StorageError::new(
                                        ErrorKind::Other,
                                        format!("Deflate compression task failed: {error}"),
                                    )),
                                    state,
                                ));
                            }
                        };
                    if let Some(progress) = &state.wire_progress {
                        progress(compressed.len() as u64);
                    }
                    return Some((Ok(bytes::Bytes::from(compressed)), state));
                }
                if state.eof {
                    return None;
                }
                match state.source.next().await {
                    Some(Ok(chunk)) if chunk.is_empty() => {}
                    Some(Ok(chunk)) => state.buffered.extend_from_slice(&chunk),
                    Some(Err(error)) => return Some((Err(error), state)),
                    None => state.eof = true,
                }
            }
        },
    ))
}

fn compress_deflate_block(block: &[u8]) -> Result<Vec<u8>, StorageError> {
    use std::io::Write as _;

    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(block).map_err(|error| {
        StorageError::new(
            ErrorKind::Other,
            format!("compress Kubernetes transfer: {error}"),
        )
    })?;
    let compressed = encoder.finish().map_err(|error| {
        StorageError::new(
            ErrorKind::Other,
            format!("finish Kubernetes transfer compression: {error}"),
        )
    })?;
    let raw_len = block.len() as u32;
    let mut output = Vec::with_capacity(8 + block.len());
    output.extend_from_slice(&raw_len.to_be_bytes());
    if compressed.len() < block.len() {
        output.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
        output.extend_from_slice(&compressed);
    } else {
        output.extend_from_slice(&(raw_len | STORED_BLOCK).to_be_bytes());
        output.extend_from_slice(block);
    }
    Ok(output)
}

struct DeflateDecodeState {
    source: ByteStream,
    buffered: Vec<u8>,
    eof: bool,
    wire_progress: Option<WireProgress>,
}

pub(crate) fn decode_deflate_stream(
    source: ByteStream,
    wire_progress: Option<WireProgress>,
) -> ByteStream {
    Box::pin(futures_util::stream::unfold(
        DeflateDecodeState {
            source,
            buffered: Vec::new(),
            eof: false,
            wire_progress,
        },
        |mut state| async move {
            loop {
                if state.buffered.len() >= 8 {
                    let raw_len =
                        u32::from_be_bytes(state.buffered[..4].try_into().expect("four bytes"))
                            as usize;
                    let encoded =
                        u32::from_be_bytes(state.buffered[4..8].try_into().expect("four bytes"));
                    let stored = encoded & STORED_BLOCK != 0;
                    let encoded_len = (encoded & !STORED_BLOCK) as usize;
                    if raw_len == 0
                        || raw_len > DEFLATE_BLOCK
                        || encoded_len == 0
                        || encoded_len > DEFLATE_BLOCK + 1024
                        || (stored && encoded_len != raw_len)
                    {
                        return Some((
                            Err(StorageError::new(
                                ErrorKind::Transport,
                                "invalid Deflate transport block",
                            )),
                            state,
                        ));
                    }
                    if state.buffered.len() >= 8 + encoded_len {
                        let encoded_data = state.buffered[8..8 + encoded_len].to_vec();
                        let decoded = if stored {
                            encoded_data
                        } else {
                            let result = tokio::task::spawn_blocking(move || {
                                decompress_deflate_block(&encoded_data, raw_len)
                            })
                            .await;
                            match result {
                                Ok(Ok(decoded)) => decoded,
                                Ok(Err(error)) => return Some((Err(error), state)),
                                Err(error) => {
                                    return Some((
                                        Err(StorageError::new(
                                            ErrorKind::Transport,
                                            format!("Deflate decompression task failed: {error}"),
                                        )),
                                        state,
                                    ));
                                }
                            }
                        };
                        state.buffered.drain(..8 + encoded_len);
                        return Some((Ok(bytes::Bytes::from(decoded)), state));
                    }
                }
                if state.eof {
                    if state.buffered.is_empty() {
                        return None;
                    }
                    return Some((
                        Err(StorageError::new(
                            ErrorKind::Transport,
                            "truncated Deflate transport stream",
                        )),
                        state,
                    ));
                }
                match state.source.next().await {
                    Some(Ok(chunk)) => {
                        if let Some(progress) = &state.wire_progress {
                            progress(chunk.len() as u64);
                        }
                        state.buffered.extend_from_slice(&chunk);
                    }
                    Some(Err(error)) => return Some((Err(error), state)),
                    None => state.eof = true,
                }
            }
        },
    ))
}

fn decompress_deflate_block(encoded: &[u8], raw_len: usize) -> Result<Vec<u8>, StorageError> {
    use std::io::Read as _;

    let mut decoder = flate2::read::DeflateDecoder::new(Cursor::new(encoded));
    let mut decoded = Vec::with_capacity(raw_len);
    decoder.read_to_end(&mut decoded).map_err(|error| {
        StorageError::new(
            ErrorKind::Transport,
            format!("invalid Deflate transport data: {error}"),
        )
    })?;
    if decoded.len() != raw_len {
        return Err(StorageError::new(
            ErrorKind::Transport,
            "Deflate transport block length differs from raw length",
        ));
    }
    Ok(decoded)
}
