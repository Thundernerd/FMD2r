use std::path::PathBuf;

/// Errors from packing and converting.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("PDF error: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("no images to pack in {0}")]
    NoImages(PathBuf),
    #[error("{0}")]
    Magick(String),
    #[error("{0} is not a supported image")]
    UnsupportedImage(PathBuf),
}
