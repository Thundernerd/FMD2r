//! `fmd.mangafoxwatermark` (baseunits/lua/LuaMangaFox.pas:15-44) over FMD2's watermark remover
//! (baseunits/modules/MangaFoxWatermark.pas), which crops a known watermark off the bottom of
//! a saved page.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageEncoder, ImageFormat};
use mlua::{Lua, Table, Value, Variadic};

use super::{JPEG_QUALITY, lib_table, to_string_arg};

/// Smallest PSNR at which a page's bottom counts as a template (MangaFoxWatermark.pas:282).
const MIN_PSNR: f32 = 9.0;
/// Rows on top of a matched region that must be white (MangaFoxWatermark.pas:283).
const MIN_WHITE_BORDER: usize = 4;

/// A thresholded grayscale image: every byte is 0 or 255 (MangaFoxWatermark.pas:49-55).
struct OneBitImage {
    width: usize,
    height: usize,
    bits: Vec<u8>,
}

/// The process-wide remover: FMD2 keeps one global `TWatermarkRemover`
/// (MangaFoxWatermark.pas:84-85, :277-284).
struct Remover {
    templates: Vec<OneBitImage>,
    directory: PathBuf,
    /// Whether templates were cleared (or never loaded), so a removal may load them again.
    cleared: bool,
}

static REMOVER: Mutex<Remover> = Mutex::new(Remover {
    templates: Vec::new(),
    directory: PathBuf::new(),
    cleared: true,
});

/// How FMD2 reads and rewrites an image format, picked by sniffing its content
/// (baseunits/ImgInfos.pas:634-639): GIF is rewritten as PNG; FMD2 has no WebP reader. TIFF is
/// left out: the `image` crate is built without it, and the watermarked site serves JPEG.
fn handler(format: ImageFormat) -> Option<(ImageFormat, &'static str)> {
    match format {
        ImageFormat::Jpeg => Some((ImageFormat::Jpeg, "jpg")),
        ImageFormat::Png => Some((ImageFormat::Png, "png")),
        ImageFormat::Gif => Some((ImageFormat::Png, "png")),
        ImageFormat::Bmp => Some((ImageFormat::Bmp, "bmp")),
        _ => None,
    }
}

/// Reads an image file whose format FMD2 handles, with that format.
fn read_image(path: &Path) -> Option<(DynamicImage, ImageFormat)> {
    let data = std::fs::read(path).ok()?;
    let format = image::guess_format(&data).ok()?;
    handler(format)?;
    let image = image::load_from_memory_with_format(&data, format).ok()?;
    Some((image, format))
}

/// `CalculateGray(...) shr 8` on an 8-bit RGB pixel: FPImage widens each channel to 16 bits
/// and weighs them with `GCM_JPEG` (fcl-image fpcolcnv.inc:30-39, fpimage.pp:358 in FPC 3.2.2).
fn gray(pixel: [u8; 3]) -> u8 {
    let [r, g, b] = pixel.map(|c| f32::from(c) * 257.0);
    let gray = (0.299 * r + 0.587 * g + 0.114 * b).round_ties_even();
    (gray.min(65535.0) as u32 >> 8) as u8
}

/// Turns every byte into 0 or 255 at the threshold Otsu's method picks
/// (MangaFoxWatermark.pas:138-186).
fn otsu_threshold(bits: &mut [u8]) {
    let mut histogram = [0u32; 256];
    for &bit in bits.iter() {
        histogram[bit as usize] += 1;
    }
    let len = bits.len() as f32;
    let mean_total: f32 = (0..256).map(|i| i as f32 * histogram[i] as f32 / len).sum();
    let (mut zeroth, mut first, mut max_variance, mut threshold) = (0f32, 0f32, 0f32, 0u8);
    for (i, &count) in histogram.iter().enumerate() {
        zeroth += count as f32 / len;
        first += i as f32 * count as f32 / len;
        let variance = (mean_total * zeroth - first).powi(2);
        if zeroth != 0.0 && zeroth != 1.0 {
            let variance = variance / (zeroth * (1.0 - zeroth));
            if max_variance < variance {
                max_variance = variance;
                threshold = i as u8;
            }
        }
    }
    for bit in bits.iter_mut() {
        *bit = if *bit > threshold { 255 } else { 0 };
    }
}

/// The region `left..right` × `top..bottom` of `image` as a thresholded image, like
/// `BuildImageToOneBit` (MangaFoxWatermark.pas:188-245). A region wider than the image is
/// padded with white and the image centred in it.
fn one_bit(
    image: &DynamicImage,
    left: usize,
    top: usize,
    right: usize,
    bottom: usize,
) -> OneBitImage {
    let rgb = image.to_rgb8();
    let (image_width, image_height) = (rgb.width() as usize, rgb.height() as usize);
    let (width, height) = (right - left, bottom - top);
    let pixel = |x: usize, y: usize| gray(rgb.get_pixel(x as u32, y as u32).0);
    let mut bits = vec![0xff; width * height];
    if right <= image_width && bottom <= image_height {
        for y in 0..height {
            for x in 0..width {
                bits[y * width + x] = pixel(left + x, top + y);
            }
        }
    } else {
        // :221-243: only a too-wide region happens here (the height always fits).
        let margin = right.saturating_sub(image_width) / 2;
        for y in 0..height.min(image_height.saturating_sub(top)) {
            for x in left..image_width {
                bits[y * width + margin + x - left] = pixel(x, top + y);
            }
        }
    }
    otsu_threshold(&mut bits);
    OneBitImage {
        width,
        height,
        bits,
    }
}

/// Peak signal-to-noise ratio between two thresholded images; 0 when their sizes differ
/// (MangaFoxWatermark.pas:332-356).
fn psnr(a: &OneBitImage, b: &OneBitImage) -> f32 {
    if a.bits.len() != b.bits.len() || a.bits.is_empty() {
        return 0.0;
    }
    let sum: f32 = a
        .bits
        .iter()
        .zip(&b.bits)
        .map(|(&x, &y)| (f32::from(x) - f32::from(y)).powi(2))
        .sum();
    let mse = sum / a.bits.len() as f32;
    if mse.sqrt() < 0.0001 {
        1e6
    } else {
        10.0 * (255f32.powi(2) / mse).log10()
    }
}

/// A path as FMD2 receives it on Windows, with `\` read as a separator.
fn local_path(path: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(path).replace('\\', "/"))
}

impl Remover {
    /// `TWatermarkRemover.LoadTemplate` (MangaFoxWatermark.pas:293-320): loads every image in
    /// the directory (or the last one used, when `directory` is empty) as a template and returns
    /// how many loaded.
    fn load_templates(&mut self, directory: &Path) -> usize {
        self.cleared = false;
        if !directory.as_os_str().is_empty() {
            directory.clone_into(&mut self.directory);
        }
        if self.directory.as_os_str().is_empty() || !self.directory.is_dir() {
            return 0;
        }
        self.templates.clear();
        let Ok(entries) = std::fs::read_dir(&self.directory) else {
            return 0;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        for file in files {
            // AddFileToTemplates (:247-275): files FMD2 has no reader for are skipped.
            if let Some((image, _)) = read_image(&file) {
                let (w, h) = (image.width() as usize, image.height() as usize);
                self.templates.push(one_bit(&image, 0, 0, w, h));
            }
        }
        self.templates.len()
    }

    /// `TWatermarkRemover.RemoveWatermark` (MangaFoxWatermark.pas:358-487): finds the template
    /// that best matches the bottom of the page and, when it matches well enough, rewrites the
    /// page without those rows. Returns whether the page was rewritten.
    fn remove(&mut self, file: &Path, save_as_png: bool) -> bool {
        // :377-382
        if self.templates.is_empty() {
            if !self.cleared {
                return false;
            }
            self.load_templates(Path::new(""));
        }
        if self.templates.is_empty() {
            return false;
        }
        let Some((image, format)) = read_image(file) else {
            return false;
        };
        let (width, height) = (image.width() as usize, image.height() as usize);
        // :398-441
        let mut best: Option<(usize, f32)> = None;
        for (i, template) in self.templates.iter().enumerate() {
            if height < template.height {
                continue;
            }
            let left = width.saturating_sub(template.width) / 2;
            let region = one_bit(
                &image,
                left,
                height - template.height,
                left + template.width,
                height,
            );
            let border = (MIN_WHITE_BORDER * region.width).min(region.bits.len());
            if region.bits[..border].contains(&0) {
                continue;
            }
            let value = psnr(&region, template);
            if value > best.map_or(0.0, |(_, v)| v) {
                best = Some((i, value));
            }
        }
        let Some((index, value)) = best else {
            return false;
        };
        if value < MIN_PSNR {
            return false;
        }
        // :442-477
        let kept = (height - self.templates[index].height) as u32;
        let cropped = image.crop_imm(0, 0, width as u32, kept);
        let (mut writer, mut ext) = handler(format).unwrap_or((ImageFormat::Png, "png"));
        if save_as_png {
            (writer, ext) = (ImageFormat::Png, "png");
        }
        let target = file.with_extension(ext);
        // FMD2 deletes both files first; a failure shows up as the write failing.
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(&target);
        write_image(&cropped, &target, writer).is_ok() && target.exists()
    }
}

/// Writes `image` like FMD2's writers: a grayscale JPEG stays grayscale, also when saved as PNG,
/// where FMD2 writes an indexed (gray palette) PNG (:463-470); other PNGs are RGB without
/// alpha, as `TFPWriterPNG` writes them by default.
fn write_image(image: &DynamicImage, path: &Path, format: ImageFormat) -> image::ImageResult<()> {
    let gray = matches!(image, DynamicImage::ImageLuma8(_));
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    match format {
        ImageFormat::Jpeg if gray => {
            let luma = image.to_luma8();
            JpegEncoder::new_with_quality(file, JPEG_QUALITY).encode_image(&luma)
        }
        ImageFormat::Jpeg => {
            JpegEncoder::new_with_quality(file, JPEG_QUALITY).encode_image(&image.to_rgb8())
        }
        ImageFormat::Png if gray => {
            let luma = image.to_luma8();
            PngEncoder::new(file).write_image(
                luma.as_raw(),
                luma.width(),
                luma.height(),
                image::ExtendedColorType::L8,
            )
        }
        ImageFormat::Png => {
            let rgb = image.to_rgb8();
            PngEncoder::new(file).write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
        }
        _ => {
            let mut file = file;
            image.to_rgb8().write_to(&mut file, format)
        }
    }
}

/// Opens the library (baseunits/lua/LuaMangaFox.pas:32-43).
pub(super) fn open(lua: &Lua) -> mlua::Result<Table> {
    lib_table(
        lua,
        vec![
            (
                // baseunits/lua/LuaMangaFox.pas:15-19
                "LoadTemplate",
                lua.create_function(|lua, directory: Value| {
                    let directory = local_path(&to_string_arg(lua, directory)?);
                    let mut remover = REMOVER.lock().unwrap_or_else(PoisonError::into_inner);
                    Ok(remover.load_templates(&directory))
                })?,
            ),
            (
                // baseunits/lua/LuaMangaFox.pas:21-30: the PNG flag counts only as a second
                // argument.
                "RemoveWatermark",
                lua.create_function(|lua, args: Variadic<Value>| {
                    let mut args = args.into_iter();
                    let file = local_path(&to_string_arg(lua, args.next().unwrap_or(Value::Nil))?);
                    let save_as_png = match (args.next(), args.next()) {
                        (Some(flag), None) => !matches!(flag, Value::Nil | Value::Boolean(false)),
                        _ => false,
                    };
                    let mut remover = REMOVER.lock().unwrap_or_else(PoisonError::into_inner);
                    Ok(remover.remove(&file, save_as_png))
                })?,
            ),
        ],
    )
}
