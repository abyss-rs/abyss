#[path = "../storage/helper_protocol.rs"]
mod helper_protocol;

#[path = "abyss-kube-helper/compress.rs"]
mod compress;
#[path = "abyss-kube-helper/decompress.rs"]
mod decompress;
#[path = "abyss-kube-helper/ops.rs"]
mod ops;
#[path = "abyss-kube-helper/paths.rs"]
mod paths;
#[path = "abyss-kube-helper/tree.rs"]
mod tree;

#[cfg(test)]
#[path = "abyss-kube-helper/tests.rs"]
mod tests;

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};

use crate::ops::execute;
use crate::paths::{read_frame, write_frame};
use crate::tree::read_tree;
use helper_protocol::{HelperOperation, HelperRequest, HelperResult, PROTOCOL_VERSION};

pub(crate) const ROOT: &str = "/data";
pub(crate) const MAX_FRAME: usize = 16 * 1024 * 1024;
pub(crate) const LZ4_BLOCK: usize = 256 * 1024;
pub(crate) const BROTLI_BLOCK: usize = 16 * 1024 * 1024;
pub(crate) const STORED_BLOCK: u32 = 1 << 31;
pub(crate) const HELPER_PORT: u16 = 31_777;

fn main() {
    let action = std::env::args().nth(1).unwrap_or_else(|| "idle".to_owned());
    if action == "idle" {
        if let Err(error) = listen() {
            eprintln!("helper listener failed: {error}");
            std::process::exit(1);
        }
        unreachable!();
    }
    if action != "serve" {
        eprintln!("usage: abyss-kube-helper [idle|serve]");
        std::process::exit(2);
    }
    if let Err(error) = serve() {
        let _ = write_frame(
            &mut io::stdout().lock(),
            &HelperResult::Error {
                kind: "io".to_owned(),
                message: error.to_string(),
            },
        );
        std::process::exit(1);
    }
}

pub(crate) fn serve() -> io::Result<()> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    serve_io(&mut input, &mut output)
}

pub(crate) fn listen() -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", HELPER_PORT))?;
    for connection in listener.incoming() {
        match connection {
            Ok(connection) => {
                std::thread::spawn(move || {
                    if let Err(error) = serve_connection(connection) {
                        eprintln!("helper connection failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("accept helper connection: {error}"),
        }
    }
    Ok(())
}

pub(crate) fn serve_connection(mut input: TcpStream) -> io::Result<()> {
    let mut output = input.try_clone()?;
    serve_io(&mut input, &mut output)
}

pub(crate) fn serve_io(input: &mut impl Read, output: &mut impl Write) -> io::Result<()> {
    let request: HelperRequest = read_frame(input)?;
    if request.version != PROTOCOL_VERSION {
        return write_frame(
            output,
            &HelperResult::Error {
                kind: "version".to_owned(),
                message: format!(
                    "protocol version {} is unsupported; expected {}",
                    request.version, PROTOCOL_VERSION
                ),
            },
        );
    }
    if let HelperOperation::ReadTree {
        root,
        entries,
        compression,
    } = &request.operation
    {
        return read_tree(root, entries, *compression, output);
    }
    let result = execute(&request.operation, input);
    match result {
        Ok((response, payload)) => {
            write_frame(output, &response)?;
            if let Some((file, size)) = payload {
                let copied = io::copy(&mut file.take(size), output)?;
                if copied != size {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        format!("file ended after {copied} of {size} bytes"),
                    ));
                }
            }
            output.flush()
        }
        Err(error) => write_frame(
            output,
            &HelperResult::Error {
                kind: error.kind().to_string(),
                message: error.to_string(),
            },
        ),
    }
}
