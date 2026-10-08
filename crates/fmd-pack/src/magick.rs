//! Optional ImageMagick conversion through `magick` (baseunits/imagemagickmanager.pas).

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::PackError;

/// ImageMagick settings (`TImageMagickManager`, baseunits/imagemagickmanager.pas:14-60).
#[derive(Debug, Clone)]
pub struct MagickOptions {
    /// The `magick` executable; a bare name is looked up on PATH.
    pub executable: PathBuf,
    /// Target format and extension, e.g. `jpg`, `png`, `webp`, `jxl` (`SaveAs`).
    pub save_as: String,
    /// `-quality` value (`Quality`).
    pub quality: u32,
    /// `-compress` type, e.g. `JPEG` or `Zip`; omitted when `None` (`Compression`).
    pub compression: Option<String>,
    /// Kill `magick` after this long (`TimeoutMS`, 300000 ms in FMD2,
    /// baseunits/imagemagickmanager.pas:504). FMD2 measures it from the last output.
    pub timeout: Duration,
}

impl Default for MagickOptions {
    fn default() -> Self {
        Self {
            executable: PathBuf::from("magick"),
            save_as: "png".into(),
            quality: 90,
            compression: None,
            timeout: Duration::from_millis(300_000),
        }
    }
}

/// Whether the configured `magick` executable runs (`FindMagickBinary`).
pub fn magick_available(opts: &MagickOptions) -> bool {
    Command::new(&opts.executable)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Converts the `files` in `dir` that are not already `save_as` images with one `magick`
/// call, deletes each original whose converted file exists, and returns the resulting paths
/// in input order (`TTaskThread.Convert`, baseunits/uDownloadsManager.pas:613-711).
///
/// JPEG XL uses `magick mogrify -path <dir> <files>` as FMD2 does; FMD2 first copies the
/// files to a temporary folder and passes a wildcard, which the explicit file list makes
/// unnecessary. Other formats pass a quoted file list as `@list` and name the outputs with
/// `-set filename:name %t` (`ConvertImage`, baseunits/imagemagickmanager.pas:662-686).
pub fn magick_convert(
    dir: &Path,
    files: &[PathBuf],
    opts: &MagickOptions,
) -> Result<Vec<PathBuf>, PackError> {
    let save_as = opts.save_as.to_ascii_lowercase();
    let needs_convert = |path: &Path| {
        path.extension()
            .is_none_or(|e| e.to_string_lossy().to_ascii_lowercase() != save_as)
    };
    let to_convert: Vec<&PathBuf> = files.iter().filter(|f| needs_convert(f)).collect();
    if to_convert.is_empty() {
        return Ok(files.to_vec());
    }

    let mut options: Vec<String> = vec!["-quality".into(), opts.quality.to_string()];
    if let Some(compression) = &opts.compression {
        options.extend(["-compress".into(), compression.clone()]);
    }
    options.extend(["-format".into(), opts.save_as.clone()]);

    let mut command = Command::new(&opts.executable);
    let mut list_file = None;
    if save_as == "jxl" {
        command.arg("mogrify").args(&options).arg("-path").arg(dir);
        command.args(&to_convert);
    } else {
        let list = dir.join(format!("FQDNList_{}.txt", uuid::Uuid::new_v4()));
        let mut f = File::create(&list)?;
        for path in &to_convert {
            writeln!(f, "\"{}\"", path.display())?;
        }
        let mut input = std::ffi::OsString::from("@");
        input.push(&list);
        command.arg(input).args(&options);
        command.args(["+adjoin", "-set", "filename:name", "%t"]);
        command.arg(dir.join(format!("%[filename:name].{}", opts.save_as)));
        list_file = Some(list);
    }
    let result = run(command, opts.timeout);
    if let Some(list) = list_file {
        std::fs::remove_file(list)?;
    }
    result?;

    let mut out = Vec::with_capacity(files.len());
    for file in files {
        if !needs_convert(file) {
            out.push(file.clone());
            continue;
        }
        let mut name = file.file_stem().unwrap_or_default().to_owned();
        name.push(format!(".{}", opts.save_as));
        let converted = dir.join(name);
        if converted.exists() {
            std::fs::remove_file(file)?;
            out.push(converted);
        } else {
            out.push(file.clone());
        }
    }
    Ok(out)
}

/// `ExecuteMagickCommand` (baseunits/imagemagickmanager.pas:504-646): runs the command with
/// stderr merged into the captured output, killing it after `timeout`.
fn run(mut command: Command, timeout: Duration) -> Result<(), PackError> {
    let mut log = tempfile_for_output()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.0.try_clone()?))
        .stderr(Stdio::from(log.0.try_clone()?))
        .spawn()
        .map_err(|e| PackError::Magick(format!("ImageMagick execution failed: {e}")))?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > timeout {
            child.kill()?;
            child.wait()?;
            return Err(PackError::Magick(format!(
                "ImageMagick command timed out after {} ms",
                timeout.as_millis()
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if status.success() {
        return Ok(());
    }
    let output = log.read()?;
    Err(PackError::Magick(format!(
        "ImageMagick command failed ({status}): {}",
        if output.is_empty() {
            "No output from process"
        } else {
            output.trim()
        }
    )))
}

/// An anonymous temporary file collecting the process output.
struct OutputFile(File, PathBuf);

impl OutputFile {
    fn read(&mut self) -> std::io::Result<String> {
        std::fs::read_to_string(&self.1)
    }
}

impl Drop for OutputFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.1);
    }
}

fn tempfile_for_output() -> std::io::Result<OutputFile> {
    let path = std::env::temp_dir().join(format!("fmd-magick-{}.log", uuid::Uuid::new_v4()));
    Ok(OutputFile(File::create(&path)?, path))
}
