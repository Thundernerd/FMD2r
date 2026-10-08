# xpath-fpc

`libfmdxpath.so`: FMD2's own XPath/XQuery engine (Benito van der Zander's *internettools*), configured exactly as
FMD2's `TXQueryEngineHTML` (`baseunits/XQueryEngineHTML.pas:384-400`), exported over the C ABI in
[`fmdxpath.h`](fmdxpath.h). The `fpc` backend of `fmd-xpath` (T08) binds it, so module XPath behaves bit for bit
like FMD2.

## Building

Needs `fpc` 3.2.2 (`apt install fpc`), `curl`, `tar` and binutils' `nm`. No Lazarus.

```sh
crates/xpath-fpc/build.sh            # -> crates/xpath-fpc/build/libfmdxpath.so
crates/xpath-fpc/build.sh OUT_DIR    # -> OUT_DIR/libfmdxpath.so
```

The script downloads the pinned internettools revision into `OUT_DIR/vendor` (once), compiles
`pascal/fmdxpath.lpr` with Lazarus' default package options (`-MObjFPC -Scghi`), and fails if the functions
declared in `fmdxpath.h` and the ones the library exports differ.

This directory is also a Cargo package, kept out of the root workspace because it needs `fpc`. Its `build.rs` runs
`build.sh` into `OUT_DIR`, or, when `FMDXPATH_LIB_DIR` names a directory holding a prebuilt `libfmdxpath.so`,
links that one and skips the build (outside `target/`, Cargo doesn't add that directory to `LD_LIBRARY_PATH` when
running other crates' tests and binaries, so set it yourself); `xpath_fpc::LIB_DIR` names the directory in use. `fmd-xpath`'s `fpc` feature
(and `fmd-lua`'s `xpath-fpc`, which turns it on) depends on it; with the feature off the workspace builds without
`fpc`. The ABI tests call the library through raw FFI:

```sh
cd crates/xpath-fpc && cargo test
```

CI (`xpath-fpc` job) installs `fpc`, runs fmt, clippy and the tests, then clippy and the tests of `fmd-xpath` and
`fmd-lua` with the backend on, and uploads `libfmdxpath.so` as the `libfmdxpath-linux-x86_64` artifact.

## Pinned internettools revision

[`Slasar41/internettools@a547b0f7c69d2be3b9d7236cdccee1b10580495d`](https://github.com/Slasar41/internettools/commit/a547b0f7c69d2be3b9d7236cdccee1b10580495d)
(2024-09-30), set in `build.sh`.

FMD2 doesn't pin internettools: it used to be a git submodule (last at `benibela/internettools@4f7490dd`, 2018),
and today `scripts/install_submodules.bat` clones this fork's default branch. The fork is
`benibela/internettools@ad4f29ad` (2024-08-25) plus one commit that adds the FLRE and PUCU regex sources that
internettools' default configuration (`USE_FLRE_WITH_CACHE`) needs and that FMD2's README tells builders to copy in.

## Behaviour worth knowing

These come from FMD2's engine and are kept on purpose (the tests pin them):

- `fx_value_to_string` on a node is trimmed: internettools' global `XQGlobalTrimNodes` defaults to true and FMD2
  never changes it. `trimText := False` still keeps the whitespace in the tree, so `fx_value_inner_html` and
  `fx_value_outer_html` show it. `fx_value_inner_text` normalizes whitespace itself.
- Float exceptions raise, as on FMD2's threads: `1e308 * 10` and `xs:float("1e40")` are empty values, and
  `xs:double("1e400")` is `5.0E-324`.

Where FMD2 would crash, the shim reports an error instead: the node accessors (`GetAttribute`, `InnerHTML`, ...) on a
non-node dereference nil in FMD2 (`baseunits/lua/LuaIXQValue.pas:43-72`); here they return an empty string and set
`fx_last_error`.

## Threads and global state

Any thread may call in. A document and the values evaluated from it must not be used from two threads at once.

- **Per document:** each `fx_doc` has its own `TXQueryEngine` and `TTreeParser`. Values hold a reference to their
  document (an atomic count), so documents and values can be freed in any order.
- **internettools globals:** the FLRE regex cache is process-wide and guarded by internettools' own critical
  section. The other globals (function and type registries, `XQGlobalTrimNodes`, ...) are set when the library loads
  and only read afterwards. internettools' per-thread caches (namespace cache, ...) are threadvars.
- **FPC threadvars:** FPC sets up a host thread on its first call. The thread keeps internettools' caches and the
  last error (`fx_last_error`) until it calls `fx_thread_exit()`; without that call, every exiting thread leaks its
  FPC heap chunk (32 KiB). FPC's own `DoneThread` is not used: it crashes on a thread FPC didn't create.
- **Float environment:** FMD2 evaluates on FPC threads, which unmask overflow, divide-by-zero and invalid
  operation. Every call switches the calling thread to that environment and restores the caller's on return.
  Because an FPC library installs no signal handlers, the library installs a process-wide `SIGFPE` handler when it
  loads: on a thread inside a call it raises the Pascal exception an FPC program would get; any other `SIGFPE` goes
  to the handler that was installed before (or the default action). See `pascal/fxfpu.pas`.
- **Not handled:** `SIGSEGV` and other faults still terminate the process, as they would for any C library. An FPC
  program would turn an access violation inside internettools into an exception; that only happens on an
  internettools bug.
