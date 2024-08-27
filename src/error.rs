use std::fmt::Debug;
use thiserror::Error;

/// Enum with all errors in this crate.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum AiocogeoError {
    /// General error.
    #[error("General error: {0}")]
    General(String),

    #[error(transparent)]
    IOError(#[from] std::io::Error),

    #[error(transparent)]
    JPEGDecodingError(#[from] jpeg::Error),

    #[error(transparent)]
    ObjectStore(#[from] object_store::Error),

    #[error(transparent)]
    TIFFError(#[from] tiff::TiffError),
}

/// Crate-specific result type.
pub type Result<T> = std::result::Result<T, AiocogeoError>;
