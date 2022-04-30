#![feature(concat_bytes)]
use enumset::{EnumSet, EnumSetType};
use num_enum::{IntoPrimitive, TryFromPrimitive, TryFromPrimitiveError};
use std::io::Write;

use clap::Parser;
use tokio::{
    io::{AsyncBufRead, AsyncReadExt, AsyncWriteExt, BufStream},
    net::TcpStream,
};

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Cli {
    /// host to connect to
    host: String,
}

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

#[derive(Debug, Copy, Clone, PartialEq)]
enum FrameTypeRaw {
    Known(FrameType),
    Unknown(u8),
}

fn write_frame<W: Write>(
    w: &mut W,
    r#type: FrameType,
    flags: EnumSet<Flags>,
    stream_ident: u32,
    r: bool,
    payload: &[u8],
) -> Result<(), std::io::Error> {
    assert_eq!(stream_ident & (1 << 31), 0);

    // 24 bit length limit
    assert!(payload.len() < 0x00FF_FFFF);

    let len_bytes = &(payload.len() as u32).to_be_bytes()[1..4];
    assert_eq!(len_bytes.len(), 3);

    w.write_all(len_bytes)?;
    w.write_all(
        &({
            let x: u8 = r#type.into();
            x
        })
        .to_be_bytes(),
    )?;

    w.write_all(&flags.as_u8().to_be_bytes())?;
    let si = if r {
        stream_ident | (1 << 31)
    } else {
        stream_ident
    };
    w.write_all(&si.to_be_bytes())?;
    w.write_all(payload)?;

    Ok(())
}

#[derive(EnumSetType, Debug)]
enum Flags {
    EndStream = 0,
    EndHeaders = 2,
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct FrameHeader {
    len: u32,
    r#type: FrameTypeRaw,
    flags: u8, //EnumSet<Flags>,
    stream_ident: u32,
    r: bool,
}

#[derive(Debug)]
enum FrameHeaderReadError {
    Io(std::io::Error),
    InvalidFrameType(num_enum::TryFromPrimitiveError<FrameType>),
}

impl From<std::io::Error> for FrameHeaderReadError {
    fn from(io: std::io::Error) -> Self {
        Self::Io(io)
    }
}

impl From<TryFromPrimitiveError<FrameType>> for FrameHeaderReadError {
    fn from(tfpe: TryFromPrimitiveError<FrameType>) -> Self {
        Self::InvalidFrameType(tfpe)
    }
}

fn u24_from_be_bytes(bytes: [u8; 3]) -> u32 {
    (bytes[0] as u32) << 16 | (bytes[1] as u32) << 8 | (bytes[2] as u32)
}

fn parse_frame_header(buf: &[u8]) -> FrameHeader {
    let len = u24_from_be_bytes(buf[0..3].try_into().unwrap());
    let r#type: FrameTypeRaw = match buf[3].try_into() {
        Ok(v) => FrameTypeRaw::Known(v),
        Err(_) => FrameTypeRaw::Unknown(buf[3]),
    };

    let si = u32::from_be_bytes(buf[5..9].try_into().unwrap());
    let stream_ident = si & !(1 << 31);
    let r = (si & (1 << 31)) != 0;

    FrameHeader {
        len,
        r#type,
        flags: buf[4],
        stream_ident,
        r,
    }
}

async fn read_frame_header<R: AsyncBufRead + std::marker::Unpin>(
    r: &mut R,
) -> Result<FrameHeader, FrameHeaderReadError> {
    let mut buf = [0u8; 3 + 1 + 1 + 4];

    r.read_exact(&mut buf).await?;
    Ok(parse_frame_header(&buf[..]))
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, IntoPrimitive, TryFromPrimitive)]
enum FrameType {
    Data = 0,
    Headers = 1,
    Priority = 2,
    RstStream = 3,
    Settings = 4,
    PushPromise = 5,
    Ping = 6,
    Goaway = 7,
    WindowUpdate = 8,
    Continuation = 9,
}

#[derive(Debug, Clone, Copy, TryFromPrimitive)]
#[repr(u32)]
enum H2Error {
    NoError = 0,
    Protocol = 1,
    Internal = 2,
    FlowControl = 3,
    SettingsTimeout = 4,
    StreamClosed = 5,
    FrameSize = 6,
    RefusedStream = 7,
    Cancel = 8,
    Compression = 9,
    Connect = 0xa,
    EnhanceYourCalm = 0xb,
    InadaquateSecurity = 0xc,
    Http11Required = 0xd,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let mut c = BufStream::new(TcpStream::connect(cli.host).await.unwrap());

    let mut buf = Vec::new();
    std::io::Write::write_all(&mut buf, PREFACE).unwrap();

    // SETTINGS
    {
        let pl = buf.len();
        // HEADERS, with `:scheme`, `:method`, and `:path` pseudo headers
        write_frame(
            &mut buf,
            FrameType::Settings,
            Flags::EndStream.into(),
            0,
            false,
            &[],
        )
        .unwrap();

        let fh = parse_frame_header(&buf[pl..]);
        println!("WRITE: {:?}", fh);
    }

    /*
    // WINDOW_UPDATE
    {
        let pl = buf.len();
        // HEADERS, with `:scheme`, `:method`, and `:path` pseudo headers
        write_frame(
            &mut buf,
            FrameType::WindowUpdate,
            EnumSet::new(),
            0,
            false,
            &[],
        )
        .unwrap();

        let fh = parse_frame_header(&buf[pl..]);
        println!("WRITE: {:?}", fh);
    }
    */

    {
        let pl = buf.len();
        // HEADERS, with `:scheme`, `:method`, and `:path` pseudo headers
        write_frame(
            &mut buf,
            FrameType::Headers,
            Flags::EndHeaders | Flags::EndStream,
            1,
            false,
            // see https://www.rfc-editor.org/rfc/rfc7541.html for encoding
            // Here, we're using literals all with N=7 length packing without hufman encoding
            //&concat_bytes!(b"\x40\x07:scheme\x04http\x40\x07:method\x03get\x40\x05:path\x01/")[..],
            // this is `:method GET`, `:path /`, `:scheme http` using the static dictionary
            &[1 << 7 | 2, 1 << 7 | 4, 1 << 7 | 6],
        )
        .unwrap();

        let fh = parse_frame_header(&buf[pl..]);
        println!("WRITE: {:?}", fh);
    }

    /*
    let pl = buf.len();
    // DATA (probably can be empty for GET, but for multipart form post, we should figure something
    // out
    write_frame(&mut buf, FrameType::Data, 0, 1, false, &[]).unwrap();

    let fh = parse_frame_header(&buf[pl..]);
    println!("WRITE: {:?}", fh);
    */

    c.write_all(&buf[..]).await.unwrap();
    c.flush().await.unwrap();

    println!("SENT!");

    // read frames, expect HEADERS and DATA (with potential CONTINUATION)

    loop {
        // read a frame header
        let fh = read_frame_header(&mut c).await.unwrap();
        println!("fh: {:?}", fh);

        let mut payload = vec![0u8; fh.len as usize];

        c.read_exact(&mut payload[..]).await.unwrap();

        println!("payload: {:?}", payload);

        // TODO: exit loop when we get a flag
        match fh.r#type {
            FrameTypeRaw::Known(FrameType::Goaway) => {
                let r_sid = u32::from_be_bytes(payload[0..4].try_into().unwrap());
                let r = (r_sid & (1 << 31)) != 0;
                let last_stream_id = r_sid & !(1 << 31);
                let error: Result<H2Error, _> =
                    u32::from_be_bytes(payload[4..8].try_into().unwrap()).try_into();
                println!("GOAWAY: r={r}, last_stream_id={last_stream_id:x}, error={error:?}");
                // TODO: we _should_ send a GOAWAY frame here before closing the connection
                break;
            }
            FrameTypeRaw::Known(FrameType::Data) => {
                let flags = EnumSet::from_u8_truncated(fh.flags);
                if flags.contains(Flags::EndStream) {
                    break;
                }
            }
            _ => {}
        }
    }
}
