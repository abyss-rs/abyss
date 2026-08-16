use futures_util::StreamExt;

use super::{LZ4_BLOCK, STORED_BLOCK};
use crate::storage::{ByteStream, ErrorKind, StorageError, WireProgress};

struct Lz4EncodeState {
    source: ByteStream,
    buffered: bytes::BytesMut,
    eof: bool,
    wire_progress: Option<WireProgress>,
}

pub(crate) fn encode_lz4_stream(
    source: ByteStream,
    wire_progress: Option<WireProgress>,
) -> ByteStream {
    Box::pin(futures_util::stream::unfold(
        Lz4EncodeState {
            source,
            buffered: bytes::BytesMut::with_capacity(LZ4_BLOCK),
            eof: false,
            wire_progress,
        },
        |mut state| async move {
            loop {
                if state.buffered.len() >= LZ4_BLOCK || (state.eof && !state.buffered.is_empty()) {
                    let length = state.buffered.len().min(LZ4_BLOCK);
                    let block = state.buffered.split_to(length);
                    let compressed = lz4_flex::block::compress(&block);
                    let raw_len = block.len() as u32;
                    let mut output = Vec::with_capacity(8 + block.len());
                    output.extend_from_slice(&raw_len.to_be_bytes());
                    if compressed.len() < block.len() {
                        output.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
                        output.extend_from_slice(&compressed);
                    } else {
                        output.extend_from_slice(&(raw_len | STORED_BLOCK).to_be_bytes());
                        output.extend_from_slice(&block);
                    }
                    if let Some(progress) = &state.wire_progress {
                        progress(output.len() as u64);
                    }
                    return Some((Ok(bytes::Bytes::from(output)), state));
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

struct Lz4DecodeState {
    source: ByteStream,
    buffered: Vec<u8>,
    eof: bool,
    wire_progress: Option<WireProgress>,
}

pub(crate) fn decode_lz4_stream(
    source: ByteStream,
    wire_progress: Option<WireProgress>,
) -> ByteStream {
    Box::pin(futures_util::stream::unfold(
        Lz4DecodeState {
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
                        || raw_len > LZ4_BLOCK
                        || encoded_len == 0
                        || encoded_len > LZ4_BLOCK + 1024
                    {
                        return Some((
                            Err(StorageError::new(
                                ErrorKind::Transport,
                                "invalid LZ4 transport block",
                            )),
                            state,
                        ));
                    }
                    if state.buffered.len() >= 8 + encoded_len {
                        let encoded_data = &state.buffered[8..8 + encoded_len];
                        let decoded = if stored {
                            if encoded_len != raw_len {
                                return Some((
                                    Err(StorageError::new(
                                        ErrorKind::Transport,
                                        "stored LZ4 block length differs from raw length",
                                    )),
                                    state,
                                ));
                            }
                            encoded_data.to_vec()
                        } else {
                            match lz4_flex::block::decompress(encoded_data, raw_len) {
                                Ok(decoded) => decoded,
                                Err(error) => {
                                    return Some((
                                        Err(StorageError::new(
                                            ErrorKind::Transport,
                                            format!("invalid LZ4 transport data: {error}"),
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
                            "truncated LZ4 transport stream",
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
