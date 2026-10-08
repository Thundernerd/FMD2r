//! `fmd.subprocess` (baseunits/lua/LuaSubprocess.pas:21-93), with the Windows command lines
//! upstream modules build translated for Linux. Commands never go through a shell.
//!
//! Translation rules, applied to `RunCommand(exe, args...)` and `RunCommandHide(exe, args...)`:
//! - `cmd.exe /c X args...` (any case, `cmd` without `.exe` too) runs `X args...` directly
//!   (lua/utils/nodejs.lua:41). Its command line may chain steps with `&&` tokens; each runs
//!   only when the previous one succeeded. Two cmd built-ins are emulated, because nodejs.lua
//!   relies on them: `cd <dir>` (also `cd /d <dir>`, `chdir`) changes the directory later steps
//!   run in (:82), and `mkdir <dir>...` (also `md`) creates directories with their parents
//!   (:71). Other cmd syntax (pipes, redirection, quoting) is not interpreted.
//! - In the executable and every path-like argument, `\` becomes `/`. An argument is path-like
//!   when it contains a `\` and otherwise only letters, digits, non-ASCII characters, spaces
//!   and `._-~:/+@#$%`; anything else (quotes, brackets, `;`, `=`, ...) marks code or data,
//!   whose backslashes are kept.
//! - Every command runs in the runtime's working directory (see [`Runtime::set_working_dir`]),
//!   so relative paths like `lua\websitebypass\cloudflare.py` resolve against it, as they
//!   resolve against FMD2's directory on Windows (lua/websitebypass/cloudflare.lua:344).
//!
//! - `io.open` gets the same treatment, so upstream scripts find the files they name with
//!   Windows paths (`lua\websitebypass\websitebypass_config.json`,
//!   lua/websitebypass/cloudflare.lua:272): a path-like name has its `\` turned into `/`, and a
//!   relative name resolves against the working directory once one is set.
//!
//! [`Runtime::set_working_dir`]: crate::Runtime::set_working_dir

use std::path::PathBuf;
use std::rc::Rc;

use mlua::{Lua, LuaString, Table, Value, Variadic};

use super::{build_object, constructors, lib_table, to_string_arg};

/// One process to start: the translated executable, its arguments and its directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// The executable, looked up on `PATH` when it has no directory part.
    pub program: String,
    /// The arguments, passed as they are, never through a shell.
    pub args: Vec<String>,
    /// The directory the process starts in.
    pub current_dir: PathBuf,
}

/// What a finished process produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// Everything it wrote to stdout.
    pub stdout: Vec<u8>,
    /// Everything it wrote to stderr.
    pub stderr: Vec<u8>,
    /// Its exit status.
    pub status: i32,
}

/// Starts processes for `fmd.subprocess`; tests inject a fake one with
/// [`Runtime::set_spawner`](crate::Runtime::set_spawner).
pub trait Spawner {
    /// Runs `command` to completion. An error means it could not be started.
    fn run(&self, command: &Command) -> std::io::Result<Output>;
}

/// Runs commands as real child processes, stdin closed and both output pipes captured, like
/// `RunCommandLoop` with `poUsePipes` (fcl-process processbody.inc:536-589 in FPC 3.2.2).
///
/// The one exception is upstream's `websitebypass/cloudflare.py` run with Python, which cannot
/// work on Linux; FMD2r answers it with a built-in FlareSolverr client printing the script's
/// JSON (see `flaresolverr.rs`).
pub struct SystemSpawner;

impl Spawner for SystemSpawner {
    fn run(&self, command: &Command) -> std::io::Result<Output> {
        if super::flaresolverr::is_cloudflare_py(&command.program, &command.args) {
            return Ok(super::flaresolverr::run(&command.args));
        }
        let output = std::process::Command::new(&command.program)
            .args(&command.args)
            .current_dir(&command.current_dir)
            .stdin(std::process::Stdio::null())
            .output()?;
        Ok(Output {
            stdout: output.stdout,
            stderr: output.stderr,
            // A process killed by a signal has no exit code; report it as a failure.
            status: output.status.code().unwrap_or(-1),
        })
    }
}

/// The spawner and working directory of one Lua state, kept in its app data.
#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) spawner: Rc<dyn Spawner>,
    /// `None` means the process's current directory.
    pub(crate) working_dir: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            spawner: Rc::new(SystemSpawner),
            working_dir: None,
        }
    }
}

impl Config {
    /// The configuration of `lua`, or the default when none was set.
    pub(crate) fn of(lua: &Lua) -> Config {
        lua.app_data_ref::<Config>()
            .map(|c| c.clone())
            .unwrap_or_default()
    }
}

/// Whether `exe` names Windows' command interpreter.
fn is_cmd(exe: &str) -> bool {
    let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    name.eq_ignore_ascii_case("cmd.exe") || name.eq_ignore_ascii_case("cmd")
}

/// Whether `arg` looks like a Windows path (see the module docs).
fn is_path_like(arg: &str) -> bool {
    arg.contains('\\')
        && arg
            .chars()
            .all(|c| c.is_alphanumeric() || !c.is_ascii() || " ._-~:/+@#$%\\".contains(c))
}

/// `arg` with Windows path separators replaced, if it is path-like.
fn to_unix_path(arg: String) -> String {
    if is_path_like(&arg) {
        arg.replace('\\', "/")
    } else {
        arg
    }
}

/// Replaces the standard `io.open` with one that translates its file name like a command's
/// path arguments (see the module docs), then opens it with the standard function.
pub(super) fn wrap_io_open(lua: &Lua) -> mlua::Result<()> {
    let io: Table = lua.globals().get("io")?;
    let open: mlua::Function = io.get("open")?;
    let wrapped = lua.create_function(move |lua, mut args: mlua::MultiValue| {
        // A name that is not UTF-8 goes to the standard function as it is.
        let name = match args.front() {
            Some(Value::String(name)) => name.to_str().ok().map(|n| n.to_owned()),
            _ => None,
        };
        if let Some(name) = name {
            let name = to_unix_path(name);
            let path = match Config::of(lua).working_dir {
                Some(dir) if std::path::Path::new(&name).is_relative() => dir.join(name),
                _ => PathBuf::from(name),
            };
            args[0] = Value::String(lua.create_string(path.as_os_str().as_encoded_bytes())?);
        }
        open.call::<mlua::MultiValue>(args)
    })?;
    io.set("open", wrapped)
}

/// One step of a translated command line.
#[derive(Debug)]
enum Step {
    /// cmd's `cd`: later steps run in this directory, relative to the current one.
    Cd(Option<String>),
    /// cmd's `mkdir`: creates these directories.
    Mkdir(Vec<String>),
    /// Starts a process.
    Run { program: String, args: Vec<String> },
}

/// Turns one `&&`-separated part of a cmd command line into a step.
fn cmd_step(mut tokens: Vec<String>) -> Option<Step> {
    if tokens.is_empty() {
        return None;
    }
    let program = tokens.remove(0);
    let name = program.to_ascii_lowercase();
    Some(match name.as_str() {
        "cd" | "chdir" => {
            if tokens.first().is_some_and(|t| t.eq_ignore_ascii_case("/d")) {
                tokens.remove(0);
            }
            Step::Cd(tokens.into_iter().next())
        }
        "mkdir" | "md" => Step::Mkdir(tokens),
        _ => Step::Run {
            program,
            args: tokens,
        },
    })
}

/// Turns the executable and arguments a module passed into the steps to run.
fn translate(exe: String, args: Vec<String>) -> Vec<Step> {
    let exe = to_unix_path(exe);
    let mut args: Vec<String> = args.into_iter().map(to_unix_path).collect();
    if is_cmd(&exe) && args.first().is_some_and(|a| a.eq_ignore_ascii_case("/c")) {
        args.remove(0);
        return args
            .split(|t| t == "&&")
            .filter_map(|part| cmd_step(part.to_vec()))
            .collect();
    }
    vec![Step::Run { program: exe, args }]
}

/// Runs `steps` in `dir` until one fails, collecting their output. Returns whether all ran
/// and succeeded, the collected output, and the last exit status.
fn execute(steps: Vec<Step>, mut dir: PathBuf, spawner: &dyn Spawner) -> (bool, Output) {
    let mut out = Output {
        stdout: Vec::new(),
        stderr: Vec::new(),
        status: 0,
    };
    for step in steps {
        match step {
            // `cd` without a directory prints it in cmd; nothing to change here.
            Step::Cd(None) => {}
            Step::Cd(Some(path)) => {
                let target = dir.join(path);
                if !target.is_dir() {
                    out.stderr
                        .extend_from_slice(b"The system cannot find the path specified.\r\n");
                    out.status = 1;
                    return (false, out);
                }
                dir = target;
            }
            Step::Mkdir(paths) => {
                for path in paths {
                    let target = dir.join(&path);
                    let error = if target.exists() {
                        Some(format!("A subdirectory or file {path} already exists.\r\n"))
                    } else {
                        std::fs::create_dir_all(&target)
                            .err()
                            .map(|e| format!("{path}: {e}\r\n"))
                    };
                    if let Some(error) = error {
                        out.stderr.extend_from_slice(error.as_bytes());
                        out.status = 1;
                    }
                }
                if out.status != 0 {
                    return (false, out);
                }
            }
            Step::Run { program, args } => {
                let command = Command {
                    program,
                    args,
                    current_dir: dir.clone(),
                };
                match spawner.run(&command) {
                    Ok(output) => {
                        out.stdout.extend_from_slice(&output.stdout);
                        out.stderr.extend_from_slice(&output.stderr);
                        out.status = output.status;
                        if output.status != 0 {
                            return (false, out);
                        }
                    }
                    // RunCommandLoop catches the failed start and returns 1, keeping what was
                    // read so far (processbody.inc:580-588 in FPC 3.2.2); the exit status stays
                    // unset in FMD2, -1 here.
                    Err(_) => {
                        out.status = -1;
                        return (false, out);
                    }
                }
            }
        }
    }
    (true, out)
}

/// `_runcommand` (baseunits/lua/LuaSubprocess.pas:29-62): runs the command and returns
/// `(ok, stdout, stderr, exit_status)`, where `ok` is true only when the process ran and exited
/// with status 0 (:52-56).
fn run_command(
    lua: &Lua,
    args: Variadic<Value>,
) -> mlua::Result<(bool, LuaString, LuaString, i32)> {
    let mut args = args
        .into_iter()
        .map(|a| Ok(String::from_utf8_lossy(&to_string_arg(lua, a)?).into_owned()))
        .collect::<mlua::Result<Vec<String>>>()?;
    let exe = if args.is_empty() {
        String::new()
    } else {
        args.remove(0)
    };
    let config = Config::of(lua);
    let dir = match config.working_dir {
        Some(dir) => dir,
        None => std::env::current_dir().map_err(mlua::Error::external)?,
    };
    let (ok, output) = execute(translate(exe, args), dir, config.spawner.as_ref());
    // lua_pushstring copies the output as C strings, so each ends at its first NUL (:58-59).
    let c_string = |bytes: &[u8]| {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        lua.create_string(&bytes[..end])
    };
    Ok((
        ok,
        c_string(&output.stdout)?,
        c_string(&output.stderr)?,
        output.status,
    ))
}

/// Opens the library (baseunits/lua/LuaSubprocess.pas:74-93).
pub(super) fn open(lua: &Lua) -> mlua::Result<Table> {
    // baseunits/lua/LuaSubprocess.pas:21-25: a TProcess object without methods
    // (luaSubprocessAddMetaTable at :83-87 adds none).
    let create = |lua: &Lua, _: Variadic<Value>| {
        let class = crate::LuaClass::new(Rc::new(std::cell::RefCell::new(())));
        Ok(Value::UserData(build_object(lua, class)?))
    };
    let mut functions = constructors(lua, create)?;
    functions.extend([
        // baseunits/lua/LuaSubprocess.pas:64-67 (_runcommand with default options)
        ("RunCommand", lua.create_function(run_command)?),
        // baseunits/lua/LuaSubprocess.pas:69-72: hiding the window means nothing on Linux.
        ("RunCommandHide", lua.create_function(run_command)?),
    ]);
    lib_table(lua, functions)
}
