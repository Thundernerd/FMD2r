//! Output: folder / zip / cbz (zip crate), pdf (lopdf), epub (custom on zip); image conversion via
//! `image`, optionally ImageMagick; FMD2-compatible file naming (`CustomRename`) and natural sort.

mod convert;
mod epub;
mod error;
mod magick;
mod naming;
mod natural_sort;
mod pack;
mod pdf;
mod whole;

pub use convert::{ConvertTarget, PngCompression, convert, convert_bytes};
pub use error::PackError;
pub use magick::{MagickOptions, magick_available, magick_convert};
pub use naming::{
    MAX_IMAGE_FILE_PATH, RenameContext, RenameOptions, SymbolMode, custom_rename, fit_file_name,
    page_file_name,
};
pub use natural_sort::natural_cmp;
pub use pack::{PackFormat, PackOptions, pack};
pub use whole::{part_path, write_whole};
