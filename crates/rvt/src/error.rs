//! What can be wrong with a Revit file, said so that a person can act on it.

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// The file is not a Compound File, or it is one and not Revit's.
    NotRevit(String),
    /// Something is over a limit (`size` is what was found, `limit` what was allowed); `what` says what.
    TooBig { what: String, size: u64, limit: u64 },
    /// The container is damaged.
    Damaged(String),
    /// The file has no stream of this name.
    NoStream(String),
    /// A stream that should hold gzip data holds none.
    NoGzip(String),
    /// A gzip member of a stream does not inflate.
    BadDeflate { stream: String, offset: usize, reason: String },
    /// A stream inflates to more than the limit allows (a compression bomb, or a file too big for the limit).
    TooMuch { stream: String, limit: usize },
    /// The file or a stream could not be read.
    Io(String),
    /// A stream is there and inflates, but its contents are not what Revit writes.
    Format(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotRevit(why) | Error::Damaged(why) | Error::Io(why) | Error::Format(why) => f.write_str(why),
            Error::TooBig { what, limit, .. } => write!(f, "{what}, over the limit of {limit}"),
            Error::NoStream(name) => write!(f, "the file has no stream named {name}"),
            Error::NoGzip(name) => write!(f, "stream {name} has no gzip data in it"),
            Error::BadDeflate { stream, offset, reason } => {
                write!(f, "stream {name} does not inflate at byte {offset}: {reason}", name = stream)
            }
            Error::TooMuch { stream, limit } => {
                write!(f, "stream {stream} inflates to more than {limit} bytes, the limit; the file is damaged or hostile")
            }
        }
    }
}

impl std::error::Error for Error {}
