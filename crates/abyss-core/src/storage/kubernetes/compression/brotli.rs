use std::io::Cursor;

use futures_util::StreamExt;

use super::{BROTLI_BLOCK, STORED_BLOCK};
use crate::storage::{ByteStream, ErrorKind, StorageError, WireProgress};

struct BrotliEncodeState {
    source: ByteStream,
    buffered: bytes::BytesMut,
    eof: bool,
    wire_progress: Option<WireProgress>,
}

pub(crate) fn encode_brotli_stream(
    source: ByteStream,
    wire_progress: Option<WireProgress>,
) -> ByteStream {
    Box::pin(futures_util::stream::unfold(
        BrotliEncodeState {
            source,
            buffered: bytes::BytesMut::with_capacity(BROTLI_BLOCK),
            eof: false,
            wire_progress,
        },
        |mut state| async move {
            loop {
                if state.buffered.len() >= BROTLI_BLOCK || (state.eof && !state.buffered.is_empty())
                {
                    let length = state.buffered.len().min(BROTLI_BLOCK);
                    let block = state.buffered.split_to(length).freeze();
                    let compressed =
                        match tokio::task::spawn_blocking(move || compress_brotli_block(&block))
                            .await
                        {
                            Ok(Ok(compressed)) => compressed,
                            Ok(Err(error)) => return Some((Err(error), state)),
                            Err(error) => {
                                return Some((
                                    Err(StorageError::new(
                                        ErrorKind::Other,
                                        format!("Brotli compression task failed: {error}"),
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

fn compress_brotli_block(block: &[u8]) -> Result<Vec<u8>, StorageError> {
    use std::io::Write as _;

    let mut writer = brotli::CompressorWriter::new(Vec::new(), 256 * 1024, 1, 24);
    writer
        .write_all(block)
        .and_then(|_| writer.flush())
        .map_err(|error| {
            StorageError::new(
                ErrorKind::Other,
                format!("compress Kubernetes transfer: {error}"),
            )
        })?;
    let compressed = writer.into_inner();
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

struct BrotliDecodeState {
    source: ByteStream,
    buffered: Vec<u8>,
    eof: bool,
    wire_progress: Option<WireProgress>,
}

pub(crate) fn decode_brotli_stream(
    source: ByteStream,
    wire_progress: Option<WireProgress>,
) -> ByteStream {
    Box::pin(futures_util::stream::unfold(
        BrotliDecodeState {
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
                        || raw_len > BROTLI_BLOCK
                        || encoded_len == 0
                        || encoded_len > BROTLI_BLOCK + 1024
                        || (stored && encoded_len != raw_len)
                    {
                        return Some((
                            Err(StorageError::new(
                                ErrorKind::Transport,
                                "invalid Brotli transport block",
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
                                decompress_brotli_block(&encoded_data, raw_len)
                            })
                            .await;
                            match result {
                                Ok(Ok(decoded)) => decoded,
                                Ok(Err(error)) => return Some((Err(error), state)),
                                Err(error) => {
                                    return Some((
                                        Err(StorageError::new(
                                            ErrorKind::Transport,
                                            format!("Brotli decompression task failed: {error}"),
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
                            "truncated Brotli transport stream",
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

fn decompress_brotli_block(encoded: &[u8], raw_len: usize) -> Result<Vec<u8>, StorageError> {
    use std::io::Read as _;

    let mut decoder = brotli::Decompressor::new(Cursor::new(encoded), 256 * 1024);
    let mut decoded = Vec::with_capacity(raw_len);
    decoder.read_to_end(&mut decoded).map_err(|error| {
        StorageError::new(
            ErrorKind::Transport,
            format!("invalid Brotli transport data: {error}"),
        )
    })?;
    if decoded.len() != raw_len {
        return Err(StorageError::new(
            ErrorKind::Transport,
            "Brotli transport block length differs from raw length",
        ));
    }
    Ok(decoded)
}
