//! Read and write Chronoplex My Family Tree `.familyx` files.
//!
//! A `.familyx` file is an OPC (zip) package. The `family8` part holds the
//! tree as encrypted, deflated XML (see [`codec`]); photos live under
//! `Multimedia/`. The XML is kept as a lossless [`doc::Document`] so that
//! saving preserves everything this crate does not understand.

pub mod codec;
pub mod date;
pub mod doc;
pub mod edit;
pub mod model;
pub mod package;

pub use package::Package;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("xml: {0}")]
    Xml(String),
    #[error("unsupported or corrupt family data: {0}")]
    Format(String),
    #[error("HMAC mismatch: file is corrupt or not a family8 v3 part")]
    Mac,
}

pub type Result<T> = std::result::Result<T, Error>;
