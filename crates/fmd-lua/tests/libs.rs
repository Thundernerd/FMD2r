//! The remaining `fmd.*` host libraries and the `pb` C module, exercised through Lua snippets
//! run on the public runtime (docs/tickets/T13-remaining-libs-pb.md, "Seams under test").

// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used)]

use fmd_lua::Runtime;

fn run(chunk: &str) {
    Runtime::new().unwrap().exec(chunk).unwrap();
}

#[test]
fn fileutil_extracts_names_across_both_separators() {
    run(r#"
        local fu = require 'fmd.fileutil'
        assert(fu.ExtractFileName('a\\b\\c.jpg') == 'c.jpg')
        assert(fu.ExtractFileNameOnly('a/b/c.jpg') == 'c')
    "#);
}

/// A stand-in for T04's TStrings with the surface `SerializeAndMaintainNames` uses, exposed to
/// the snippet as the global `List`.
fn runtime_with_list(items: &[&str]) -> Runtime {
    use std::cell::RefCell;
    use std::rc::Rc;

    use fmd_lua::{LuaClass, mlua};

    let runtime = Runtime::new().unwrap();
    let items: Vec<Vec<u8>> = items.iter().map(|s| s.as_bytes().to_vec()).collect();
    let list = LuaClass::new(Rc::new(RefCell::new(items)))
        .read_only_property("Count", |_, l: &mut Vec<Vec<u8>>| Ok(l.len()))
        .method("Get", |lua, l: &mut Vec<Vec<u8>>, i: usize| {
            lua.create_string(&l[i])
        })
        .method("Clear", |_, l: &mut Vec<Vec<u8>>, ()| {
            l.clear();
            Ok(())
        })
        .method("Add", |_, l: &mut Vec<Vec<u8>>, s: mlua::LuaString| {
            l.push(s.as_bytes().to_vec());
            Ok(())
        })
        .build(runtime.lua())
        .unwrap();
    runtime.lua().globals().set("List", list).unwrap();
    runtime
}

/// Runs `SerializeAndMaintainNames` on a list and returns its items joined by `|`.
fn serialized(items: &[&str]) -> String {
    let runtime = runtime_with_list(items);
    runtime
        .eval(
            r#"(function()
                require('fmd.fileutil').SerializeAndMaintainNames(List)
                local t = {}
                for i = 0, List.Count - 1 do t[#t + 1] = List.Get(i) end
                return table.concat(t, '|')
            end)()"#,
        )
        .unwrap()
}

// Expected values traced by hand through `SerializeAndMaintainNames` and `PadZero`
// (baseunits/uBaseUnit.pas:1669-1742, :1591-1637).
#[test]
fn serialize_keeps_a_list_that_already_sorts() {
    assert_eq!(serialized(&["a", "B", "c"]), "a|B|c");
    assert_eq!(
        serialized(&["1.jpg", "10.jpg", "2.jpg"]),
        "1.jpg|10.jpg|2.jpg"
    );
}

#[test]
fn serialize_pads_numbers_when_that_makes_the_list_sort() {
    assert_eq!(serialized(&["2.jpg", "10.jpg"]), "002.jpg|010.jpg");
    // PadZero drops a lone non-digit at the very end of a name.
    assert_eq!(serialized(&["2a", "10b"]), "002|010");
}

#[test]
fn serialize_prefixes_a_counter_as_a_last_resort() {
    assert_eq!(serialized(&["b", "a"]), "001_b|002_a");
}

#[test]
fn serialize_ignores_a_non_userdata_argument() {
    run("require('fmd.fileutil').SerializeAndMaintainNames({'b', 'a'})");
}

// Fixtures: "hello" compressed by Python's `gzip.compress(mtime=0)`, `zlib.compress` and a raw
// `zlib.compressobj(wbits=-15)`.
const GZIP_HELLO: &str = r"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\xff\xcb\x48\xcd\xc9\xc9\x07\x00\x86\xa6\x10\x36\x05\x00\x00\x00";
const ZLIB_HELLO: &str = r"\x78\x9c\xcb\x48\xcd\xc9\xc9\x07\x00\x06\x2c\x02\x15";
const RAW_HELLO: &str = r"\xcb\x48\xcd\xc9\xc9\x07\x00";

#[test]
fn gzip_inflates_gzip_zlib_and_raw_deflate() {
    // unzipStream sniffs the header (baseunits/GZIPUtils.pas:172-235).
    for fixture in [GZIP_HELLO, ZLIB_HELLO, RAW_HELLO] {
        run(&format!(
            "assert(require('fmd.gzip').Inflate('{fixture}') == 'hello')"
        ));
    }
}

#[test]
fn gzip_returns_nothing_when_a_checksum_fails() {
    // A wrong CRC32 (gzip) or Adler-32 (zlib) fails unzipStream (baseunits/GZIPUtils.pas:257-267),
    // and lua_inflate then returns no value (baseunits/lua/LuaGZip.pas:29-36).
    let bad_gzip = GZIP_HELLO.replace(r"\x86\xa6", r"\x00\x00");
    let bad_zlib = ZLIB_HELLO.replace(r"\x06\x2c", r"\x00\x00");
    for fixture in [bad_gzip, bad_zlib] {
        run(&format!(
            "assert(select('#', require('fmd.gzip').Inflate('{fixture}')) == 0)"
        ));
    }
}

#[test]
fn gzip_returns_nothing_for_input_shorter_than_a_header() {
    // Reading the 4-byte header raises, which lua_inflate catches (baseunits/lua/LuaGZip.pas:41-44).
    run("assert(select('#', require('fmd.gzip').Inflate('ab')) == 0)");
}

/// One event a [`Recorder`] saw: level, target, `module` field and message.
type Recorded = (tracing::Level, String, String, String);

/// A tracing subscriber that records every event.
#[derive(Clone, Default)]
struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<Recorded>>>);

/// Collects an event's `module` field and message.
#[derive(Default)]
struct Fields {
    module: String,
    message: String,
}

impl tracing::field::Visit for Fields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "module" => self.module = value.to_owned(),
            "message" => self.message = value.to_owned(),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.record_str(field, &format!("{value:?}"));
    }
}

impl tracing::Subscriber for Recorder {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let meta = event.metadata();
        self.0.lock().unwrap().push((
            *meta.level(),
            meta.target().to_owned(),
            fields.module,
            fields.message,
        ));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Runs `chunk` and returns the events it logged.
fn logged(chunk: &str) -> Vec<Recorded> {
    let recorder = Recorder::default();
    tracing::subscriber::with_default(recorder.clone(), || run(chunk));
    recorder.0.lock().unwrap().clone()
}

#[test]
fn logger_sends_at_info_warn_and_error_with_the_module_name() {
    use tracing::Level;

    let events = logged(
        r#"
        MODULE = { Name = 'Site' }
        local logger = require 'fmd.logger'
        logger.Send('info message')
        logger.SendWarning('warning message')
        logger.SendError(42)
    "#,
    );
    let module = |level, message: &str| {
        (
            level,
            "fmd.logger".to_owned(),
            "Site".to_owned(),
            message.to_owned(),
        )
    };
    assert_eq!(
        events,
        vec![
            module(Level::INFO, "info message"),
            module(Level::WARN, "warning message"),
            // luaToString turns a number into its string form (baseunits/lua/LuaUtils.pas:206).
            module(Level::ERROR, "42"),
        ]
    );
}

mod subprocess {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use fmd_lua::Runtime;
    use fmd_lua::subprocess::{Command, Output, Spawner};

    /// Records every command and answers each with `output`.
    #[derive(Clone)]
    struct FakeSpawner {
        commands: Rc<RefCell<Vec<Command>>>,
        output: Output,
    }

    impl Spawner for FakeSpawner {
        fn run(&self, command: &Command) -> std::io::Result<Output> {
            self.commands.borrow_mut().push(command.clone());
            Ok(self.output.clone())
        }
    }

    fn fake(stdout: &str, stderr: &str, status: i32) -> FakeSpawner {
        FakeSpawner {
            commands: Rc::default(),
            output: Output {
                stdout: stdout.as_bytes().to_vec(),
                stderr: stderr.as_bytes().to_vec(),
                status,
            },
        }
    }

    /// A runtime whose subprocesses go to `spawner` and run in `dir`.
    fn runtime(spawner: &FakeSpawner, dir: &Path) -> Runtime {
        let runtime = Runtime::new().unwrap();
        runtime.set_spawner(spawner.clone());
        runtime.set_working_dir(dir);
        runtime
    }

    fn command(program: &str, args: &[&str], dir: impl Into<PathBuf>) -> Command {
        Command {
            program: program.to_owned(),
            args: args.iter().map(|&a| a.to_owned()).collect(),
            current_dir: dir.into(),
        }
    }

    #[test]
    fn cmd_c_becomes_a_direct_exec_like_nodejs_lua() {
        // lua/utils/nodejs.lua:41, :56
        let spawner = fake("v20.1.0\n", "", 0);
        let dir = Path::new("/srv/fmd2r");
        runtime(&spawner, dir)
            .exec(
                r#"
                local ok, out, err, code =
                    require('fmd.subprocess').RunCommandHide('cmd.exe', '/c', 'node', '-v')
                assert(ok == true and out == 'v20.1.0\n' and err == '' and code == 0)
            "#,
            )
            .unwrap();
        assert_eq!(
            *spawner.commands.borrow(),
            vec![command("node", &["-v"], dir)]
        );
    }

    #[test]
    fn backslash_paths_become_slashes_like_cloudflare_lua() {
        // lua/websitebypass/cloudflare.lua:166-183, :343-344
        let spawner = fake("{}", "", 0);
        let dir = Path::new("/srv/fmd2r");
        runtime(&spawner, dir)
            .exec(
                r#"
                local cmd = {'python', [[lua\websitebypass\cloudflare.py]], 'https://example.com',
                    '--flaresolverr-ip', '127.0.0.1', '--flaresolverr-port', '8191'}
                assert(require('fmd.subprocess').RunCommandHide(table.unpack(cmd)))
            "#,
            )
            .unwrap();
        let args = [
            "lua/websitebypass/cloudflare.py",
            "https://example.com",
            "--flaresolverr-ip",
            "127.0.0.1",
            "--flaresolverr-port",
            "8191",
        ];
        assert_eq!(
            *spawner.commands.borrow(),
            vec![command("python", &args, dir)]
        );
    }

    #[test]
    fn arguments_that_are_not_paths_keep_their_backslashes() {
        let spawner = fake("", "", 0);
        runtime(&spawner, Path::new("/"))
            .exec(r#"require('fmd.subprocess').RunCommand('node', '-e', 'console.log("a\\nb")')"#)
            .unwrap();
        assert_eq!(
            spawner.commands.borrow()[0].args,
            vec!["-e".to_owned(), r#"console.log("a\nb")"#.to_owned()]
        );
    }

    #[test]
    fn runs_a_real_translated_command() {
        crate::run(
            r#"
            local ok, out, err, code = require('fmd.subprocess').RunCommandHide('cmd.exe', '/c', 'echo', 'hi')
            assert(ok and out:match('hi') and err == '' and code == 0)
        "#,
        );
    }

    #[test]
    fn a_non_zero_exit_status_is_not_ok() {
        // `if exitstatus<>0 then presult:=false` (baseunits/lua/LuaSubprocess.pas:56).
        let spawner = fake("partial", "boom", 3);
        runtime(&spawner, Path::new("/"))
            .exec(
                r#"
                local ok, out, err, code = require('fmd.subprocess').RunCommand('tool')
                assert(ok == false and out == 'partial' and err == 'boom' and code == 3)
            "#,
            )
            .unwrap();
    }

    #[test]
    fn output_ends_at_the_first_nul() {
        // lua_pushstring copies the output as C strings (baseunits/lua/LuaSubprocess.pas:58-59).
        let spawner = fake("out\0more", "err\0more", 0);
        runtime(&spawner, Path::new("/"))
            .exec(
                r#"
                local ok, out, err = require('fmd.subprocess').RunCommand('tool')
                assert(ok and out == 'out' and err == 'err')
            "#,
            )
            .unwrap();
    }

    #[test]
    fn a_command_that_cannot_start_is_not_ok() {
        // RunCommandLoop catches the failed start and returns 1 (processbody.inc:580-588 in FPC).
        crate::run(
            r#"
            local ok, out, err = require('fmd.subprocess').RunCommand('fmd2r-no-such-program')
            assert(ok == false and out == '' and err == '')
        "#,
        );
    }

    #[test]
    fn new_and_create_return_a_process_object() {
        // baseunits/lua/LuaSubprocess.pas:21-25
        crate::run(
            r#"
            local sp = require 'fmd.subprocess'
            local p = sp.New()
            assert(type(p) == 'userdata' and p.self() == p and type(sp.Create()) == 'userdata')
        "#,
        );
    }

    #[test]
    fn cd_and_chains_run_without_a_shell_like_nodejs_lua() {
        // lua/utils/nodejs.lua:82, :92: `cd <dir> && npm list <mod>` through cmd.exe.
        let work = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(work.path().join("lua/utils/npm")).unwrap();
        let spawner = fake("puppeteer@22", "", 0);
        runtime(&spawner, work.path())
            .exec(
                r#"
                local ok, out, err, code = require('fmd.subprocess').RunCommandHide(
                    'cmd.exe', '/c', 'cd', 'lua/utils/npm', '&&', 'npm', 'list', 'puppeteer')
                assert(ok and out == 'puppeteer@22' and code == 0)
            "#,
            )
            .unwrap();
        assert_eq!(
            *spawner.commands.borrow(),
            vec![command(
                "npm",
                &["list", "puppeteer"],
                work.path().join("lua/utils/npm")
            )]
        );
    }

    #[test]
    fn a_failing_step_stops_the_chain() {
        let spawner = fake("", "", 0);
        runtime(&spawner, Path::new("/"))
            .exec(
                r#"
                local ok, out, err, code = require('fmd.subprocess').RunCommandHide(
                    'cmd.exe', '/c', 'cd', 'fmd2r-no-such-dir', '&&', 'npm', 'install')
                assert(not ok and code == 1 and err ~= '')
            "#,
            )
            .unwrap();
        assert!(spawner.commands.borrow().is_empty());
    }

    #[test]
    fn mkdir_creates_the_directory_like_nodejs_lua() {
        // lua/utils/nodejs.lua:68-71: the path arrives with backslashes.
        let work = tempfile::tempdir().unwrap();
        let spawner = fake("", "", 0);
        runtime(&spawner, work.path())
            .exec(
                r#"
                local ok, out, err, code = require('fmd.subprocess').RunCommandHide(
                    'cmd.exe', '/c', 'mkdir', [[lua\utils\npm]])
                assert(ok and out == '' and code == 0)
            "#,
            )
            .unwrap();
        assert!(work.path().join("lua/utils/npm").is_dir());
        assert!(spawner.commands.borrow().is_empty());
    }
}

mod imagepuzzle {
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::rc::Rc;

    use fmd_lua::{LuaClass, Runtime, mlua};
    use image::{ImageFormat, Rgba, RgbaImage};

    /// A stand-in for T04's MemoryStream with the surface `DeScramble` uses.
    fn stream(runtime: &Runtime, bytes: Vec<u8>) -> (mlua::AnyUserData, Rc<RefCell<Vec<u8>>>) {
        let state = Rc::new(RefCell::new(bytes));
        let stream = LuaClass::new(state.clone())
            .method("ToString", |lua, s: &mut Vec<u8>, ()| {
                lua.create_string(&*s)
            })
            .method("Clear", |_, s: &mut Vec<u8>, ()| {
                s.clear();
                Ok(())
            })
            .method(
                "WriteString",
                |_, s: &mut Vec<u8>, data: mlua::LuaString| {
                    s.extend_from_slice(&data.as_bytes());
                    Ok(())
                },
            )
            .build(runtime.lua())
            .unwrap();
        (stream, state)
    }

    /// A `size`×`size` image whose pixel (x, y) is `(50x, 50y, 100, 255)`, so every pixel
    /// tells where it came from.
    fn coordinates_image(size: u32) -> RgbaImage {
        RgbaImage::from_fn(size, size, |x, y| {
            Rgba([50 * x as u8, 50 * y as u8, 100, 255])
        })
    }

    fn encode(image: &RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        match format {
            ImageFormat::Jpeg => image::DynamicImage::ImageRgba8(image.clone())
                .to_rgb8()
                .write_to(&mut out, format)
                .unwrap(),
            _ => image.write_to(&mut out, format).unwrap(),
        }
        out.into_inner()
    }

    /// Runs `chunk` with the global `Doc` holding `input`, and returns what `Doc` holds after.
    fn descramble(input: Vec<u8>, chunk: &str) -> Vec<u8> {
        let runtime = Runtime::new().unwrap();
        let (doc, state) = stream(&runtime, input);
        runtime.lua().globals().set("Doc", doc).unwrap();
        runtime.exec(chunk).unwrap();
        state.borrow().clone()
    }

    #[test]
    fn descrambles_a_2x2_grid_with_flips() {
        let out = descramble(
            encode(&coordinates_image(4), ImageFormat::Png),
            r#"
            local p = require('fmd.imagepuzzle').Create(2, 2)
            assert(p.HorBlock == 2 and p.VerBlock == 2 and p.Matrix[3] == 3 and p.Flips[0] == 0)
            p.Matrix[0] = 1; p.Matrix[1] = 2; p.Matrix[2] = 3; p.Matrix[3] = 0
            p.Flips[0] = 1   -- mirrored left to right
            p.Flips[3] = 2   -- mirrored top to bottom
            assert(p.Matrix[0] == 1 and p.Flips[3] == 2)
            p.DeScramble(Doc, Doc)
        "#,
        );
        // Source tile i lands where Matrix[i] points, flipped as Flips[i] says
        // (baseunits/ImagePuzzle.pas:246-291), traced by hand. Each entry is the source
        // pixel's (x, y).
        let expected = [
            [(2, 3), (3, 3), (1, 0), (0, 0)],
            [(2, 2), (3, 2), (1, 1), (0, 1)],
            [(2, 0), (3, 0), (0, 2), (1, 2)],
            [(2, 1), (3, 1), (0, 3), (1, 3)],
        ];
        assert_eq!(image::guess_format(&out).unwrap(), ImageFormat::Png);
        let image = image::load_from_memory(&out).unwrap().to_rgba8();
        for (y, row) in expected.iter().enumerate() {
            for (x, &(sx, sy)) in row.iter().enumerate() {
                assert_eq!(
                    *image.get_pixel(x as u32, y as u32),
                    Rgba([50 * sx, 50 * sy, 100, 255]),
                    "pixel ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn pixels_outside_the_blocks_stay_white() {
        // 5 div 2 = 2-pixel blocks; the destination starts out filled with 255
        // (baseunits/ImagePuzzle.pas:230-239).
        let out = descramble(
            encode(&coordinates_image(5), ImageFormat::Png),
            "require('fmd.imagepuzzle').Create(2, 2).DeScramble(Doc, Doc)",
        );
        let image = image::load_from_memory(&out).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (5, 5));
        assert_eq!(*image.get_pixel(3, 3), Rgba([150, 150, 100, 255]));
        for i in 0..5 {
            assert_eq!(*image.get_pixel(4, i), Rgba([255; 4]));
            assert_eq!(*image.get_pixel(i, 4), Rgba([255; 4]));
        }
    }

    #[test]
    fn a_jpeg_comes_back_as_a_jpeg() {
        // Only PNG and WebP input switch the output format from 'jpg' (baseunits/ImagePuzzle.pas:149, :187, :205-208).
        let out = descramble(
            encode(&coordinates_image(4), ImageFormat::Jpeg),
            "require('fmd.imagepuzzle').Create(2, 2).DeScramble(Doc, Doc)",
        );
        assert_eq!(image::guess_format(&out).unwrap(), ImageFormat::Jpeg);
    }

    #[test]
    fn an_out_of_range_matrix_entry_empties_the_output() {
        // LogAndFail clears the output stream (baseunits/ImagePuzzle.pas:151-156, :248-252).
        let out = descramble(
            encode(&coordinates_image(4), ImageFormat::Png),
            "local p = require('fmd.imagepuzzle').Create(2, 2); p.Matrix[1] = 4; p.DeScramble(Doc, Doc)",
        );
        assert!(out.is_empty());
    }

    #[test]
    fn multiply_is_a_plain_integer_property() {
        // The Pascal `default 1` does not initialise the field, so it starts at 0
        // (baseunits/ImagePuzzle.pas:23, baseunits/lua/LuaImagePuzzle.pas:99).
        crate::run(
            r#"
            local p = require('fmd.imagepuzzle').New(4, 4)
            assert(p.Multiply == 0)
            p.Multiply = 8
            assert(p.Multiply == 8)
        "#,
        );
    }
}

mod mangafoxwatermark {
    use std::path::Path;

    use image::{ImageFormat, Rgb, RgbImage};

    /// A 10×8 watermark: four white rows, then four rows black on the left and white on the
    /// right. FMD2 wants at least four white rows on top (MinWhiteBorder,
    /// baseunits/modules/MangaFoxWatermark.pas:283, :413-427).
    fn watermark() -> RgbImage {
        RgbImage::from_fn(10, 8, |x, y| {
            if y >= 4 && x < 5 {
                Rgb([0, 0, 0])
            } else {
                Rgb([255, 255, 255])
            }
        })
    }

    /// A 10×20 page: twelve rows of content above `bottom`.
    fn page(bottom: &RgbImage) -> RgbImage {
        RgbImage::from_fn(10, 20, |x, y| {
            if y < 12 {
                Rgb([200, 30, (10 * x) as u8])
            } else {
                *bottom.get_pixel(x, y - 12)
            }
        })
    }

    fn save(image: &RgbImage, path: &Path, format: ImageFormat) {
        match format {
            ImageFormat::Jpeg => {
                let file = std::fs::File::create(path).unwrap();
                image::codecs::jpeg::JpegEncoder::new_with_quality(file, 95)
                    .encode_image(image)
                    .unwrap();
            }
            _ => image.save_with_format(path, format).unwrap(),
        }
    }

    fn lua_path(path: &Path) -> String {
        path.display().to_string()
    }

    // The template set is process-wide, as in FMD2 (MangaFoxWatermark.pas:84-85), so the whole
    // flow runs in one test.
    #[test]
    fn loads_templates_and_crops_a_matching_watermark() {
        let runtime = fmd_lua::Runtime::new().unwrap();
        let work = tempfile::tempdir().unwrap();
        let templates = work.path().join("templates");
        std::fs::create_dir(&templates).unwrap();
        save(&watermark(), &templates.join("w.png"), ImageFormat::Png);
        std::fs::write(templates.join("notes.txt"), "not an image").unwrap();

        let png = work.path().join("01.png");
        save(&page(&watermark()), &png, ImageFormat::Png);
        let jpg = work.path().join("02.jpg");
        save(&page(&watermark()), &jpg, ImageFormat::Jpeg);
        let clean = work.path().join("03.png");
        save(&page(&RgbImage::new(10, 8)), &clean, ImageFormat::Png);

        let upstream = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/lua/extras/mangafoxtemplate");
        fn call<T: fmd_lua::mlua::FromLua>(runtime: &fmd_lua::Runtime, chunk: &str) -> T {
            runtime
                .eval(&format!("require('fmd.mangafoxwatermark').{chunk}"))
                .unwrap()
        }
        let mf = |chunk: &str| -> bool { call(&runtime, chunk) };
        let count = |chunk: &str| -> i64 { call(&runtime, chunk) };

        // Nothing loaded yet: no template directory to fall back on (:377-382, :303).
        assert!(!mf(&format!("RemoveWatermark([[{}]])", lua_path(&png))));
        // Every image in the directory becomes a template (:293-320); upstream ships ten.
        assert_eq!(
            count(&format!("LoadTemplate([[{}]])", lua_path(&upstream))),
            10
        );
        // Loading again replaces them; files that are no image are skipped (:247-257). FanFox
        // passes the directory with a backslash (lua/modules/FanFox.lua:36).
        let windows_style = format!("{}\\templates", lua_path(work.path()));
        assert_eq!(count(&format!("LoadTemplate([[{windows_style}]])")), 1);

        // The watermark rows are cut off and the file is rewritten in its format (:442-477).
        assert!(mf(&format!("RemoveWatermark([[{}]])", lua_path(&png))));
        let cropped = image::open(&png).unwrap().to_rgb8();
        assert_eq!(cropped.dimensions(), (10, 12));
        assert_eq!(*cropped.get_pixel(3, 5), Rgb([200, 30, 30]));

        // SaveAsPNG writes `<name>.png` and deletes the original (:448-457).
        assert!(mf(&format!(
            "RemoveWatermark([[{}]], true)",
            lua_path(&jpg)
        )));
        assert!(!jpg.exists());
        let converted = work.path().join("02.png");
        assert_eq!(
            image::ImageReader::open(&converted)
                .unwrap()
                .with_guessed_format()
                .unwrap()
                .format(),
            Some(ImageFormat::Png)
        );
        assert_eq!(image::open(&converted).unwrap().height(), 12);

        // A page whose bottom is not white on top is left alone (:413-428).
        assert!(!mf(&format!("RemoveWatermark([[{}]])", lua_path(&clean))));
        assert_eq!(image::open(&clean).unwrap().height(), 20);
    }
}

/// A runtime that finds Lua files in the fixture corpus' `lua/` directory, as the T06 searcher
/// will.
fn runtime_with_lua_dir() -> Runtime {
    let runtime = Runtime::new().unwrap();
    let lua_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/lua");
    runtime
        .exec(&format!(
            "package.path = [[{lua_dir}/?.lua;]] .. package.path"
        ))
        .unwrap();
    runtime
}

#[test]
fn pb_backs_utils_protoc() {
    // lua/utils/protoc.lua:993 loads pb with `pcall(require, "pb")`.
    runtime_with_lua_dir()
        .exec(
            r#"
            local pb = require 'pb'; local protoc = require 'utils.protoc'
            assert(protoc:load('syntax="proto3"; message M { int32 a = 1; }'))
            assert(pb.decode('M', pb.encode('M', {a = 5})).a == 5)
        "#,
        )
        .unwrap();
}

#[test]
fn pb_submodules_load_like_from_pb_dll() {
    // FMD2's pb.dll also exports luaopen_pb_{io,conv,buffer,slice,unsafe}, which Lua's
    // all-in-one C searcher finds for `require 'pb.slice'` etc.
    run(r#"
        for _, name in ipairs {'pb.io', 'pb.conv', 'pb.buffer', 'pb.slice', 'pb.unsafe'} do
            assert(type(require(name)) == 'table', name)
        end
        -- Zigzag encoding maps -1 to 1 (protobuf encoding spec, "Signed Integers").
        assert(require('pb.conv').encode_sint32(-1) == 1)
    "#);
}

#[test]
fn mangaplus_loads_its_proto_through_pb() {
    // lua/modules/MangaPlus.lua:53-54, :105-110: the module body compiles MangaPlus.proto next
    // to itself with protoc and decodes responses with pb.
    let runtime = runtime_with_lua_dir();
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/lua/modules/MangaPlus.lua"
    );
    let source = std::fs::read(path).unwrap();
    runtime
        .lua()
        .load(source)
        .set_name(format!("@{path}"))
        .exec()
        .unwrap();
    runtime
        .exec(
            r#"
            local pb = require 'pb'
            assert(pb.type('.Response'))
            local r = pb.decode('Response', pb.encode('Response', {}))
            assert(type(r) == 'table')
        "#,
        )
        .unwrap();
}

#[test]
fn pcre2_is_a_stub_that_fails_clearly() {
    // The names of baseunits/lua/LuaPCRE2.pas:237-245; no upstream module uses the library.
    run(r#"
        local re = require 'fmd.pcre2'
        for _, name in ipairs {'exec', 'find', 'match', 'gmatch', 'gsub'} do
            local ok, err = pcall(re[name], 'a', 'a')
            assert(not ok and tostring(err):find('fmd.pcre2.' .. name .. ' is not implemented', 1, true), name)
        end
    "#);
}

#[test]
fn every_lib_exposes_exactly_its_pascal_names() {
    let libs: &[(&str, &[&str])] = &[
        // baseunits/lua/LuaGZip.pas:47-51
        ("fmd.gzip", &["Inflate"]),
        // baseunits/lua/LuaFileUtil.pas:33-39
        (
            "fmd.fileutil",
            &[
                "ExtractFileName",
                "ExtractFileNameOnly",
                "SerializeAndMaintainNames",
            ],
        ),
        // baseunits/lua/LuaLogger.pas:33-39
        ("fmd.logger", &["Send", "SendError", "SendWarning"]),
        // baseunits/lua/LuaSubprocess.pas:74-81
        (
            "fmd.subprocess",
            &["Create", "New", "RunCommand", "RunCommandHide"],
        ),
        // baseunits/lua/LuaImagePuzzle.pas:72-77
        ("fmd.imagepuzzle", &["Create", "New"]),
        // baseunits/lua/LuaMangaFox.pas:32-37
        (
            "fmd.mangafoxwatermark",
            &["LoadTemplate", "RemoveWatermark"],
        ),
        // baseunits/lua/LuaPCRE2.pas:237-245
        ("fmd.pcre2", &["exec", "find", "gmatch", "gsub", "match"]),
    ];
    let runtime = Runtime::new().unwrap();
    for (lib, names) in libs {
        let keys: String = runtime
            .eval(&format!(
                "(function() local t = {{}} for k in pairs(require '{lib}') do t[#t + 1] = k end \
                 table.sort(t) return table.concat(t, ',') end)()"
            ))
            .unwrap();
        assert_eq!(keys, names.join(","), "{lib}");
    }
    // The ImagePuzzle object's members (baseunits/lua/LuaImagePuzzle.pas:78-100).
    runtime
        .exec(
            r#"
            local p = require('fmd.imagepuzzle').Create(1, 1)
            assert(type(p.DeScramble) == 'function' and p.HorBlock == 1 and p.VerBlock == 1)
            assert(p.Matrix[0] == 0 and p.Flips[0] == 0 and p.Multiply == 0)
        "#,
        )
        .unwrap();
}
