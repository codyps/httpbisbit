use std::io::Write;

use clap::Parser;
use tokio::{io::AsyncWriteExt, net::TcpStream};

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Cli {
    /// host to connect to
    host: String,
}

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

async fn write_frame<W: Write>(
    w: &mut W,
    type_: u8,
    flags: u8,
    stream_ident: u32,
    r: bool,
    payload: &[u8],
) {
    todo!()
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let c = TcpStream::connect(cli.host).await.unwrap();

    let mut buf = Vec::new();
    std::io::Write::write_all(&mut buf, PREFACE).unwrap();

    c.write_all
}
