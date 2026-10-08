# T13: Remaining `fmd.*` libs and the `pb` C module
Deps: T03

## Goal
Implement the remaining host libraries modules `require`: `fmd.gzip`, `fmd.fileutil`, `fmd.logger`, `fmd.subprocess` (with Windows→Linux command translation), `fmd.imagepuzzle`, `fmd.mangafoxwatermark`, plus the `pb` (lua-protobuf) C module. `fmd.pcre2` is low priority (no upstream module uses it): implement only if time allows, else register a stub that raises a clear "not implemented" error.

## Scope (in/out)
In:
- **`fmd.gzip`**: `Inflate(data)` (check whether it handles raw deflate, zlib and gzip like `LuaGZip.pas`/`GZIPUtils.pas`).
- **`fmd.fileutil`**: `ExtractFileName`, `ExtractFileNameOnly`, `SerializeAndMaintainNames(...)` with FMD2 semantics (path separators: accept both `/` and `\`).
- **`fmd.logger`**: `Send`, `SendWarning`, `SendError` → tracing at info/warn/error with module context.
- **`fmd.subprocess`**: `New`/`Create`, `RunCommand(exe, args...)`, `RunCommandHide(exe, args...)` returning `(ok, stdout, stderr, exit_status)` exactly as `_runcommand`. Linux translation: `cmd.exe /c X args…` becomes a direct exec of `X args…`; backslashes in path-like arguments become `/`; relative paths resolve against the FMD2r working dir (so `lua\websitebypass\cloudflare.py` works). Never pass through a shell.
- **`fmd.imagepuzzle`**: `Create(hor, ver)`, `DeScramble(in_stream, out_stream)`, `Matrix[i]`, `Flips[i]`, `HorBlock`, `VerBlock` properties, reproducing `ImagePuzzle.pas` (block math, flips, output format) with the `image` crate.
- **`fmd.mangafoxwatermark`**: `LoadTemplate(dir)` and `RemoveWatermark(file)` per `MangaFoxWatermark.pas` (template images in `lua/extras/mangafoxtemplate`).
- **`pb`**: compile lua-protobuf (`pb.c`, https://github.com/starwing/lua-protobuf; FMD2 ships it as `pb.dll`) with `cc` against the vendored Lua 5.4 headers that `mlua` uses, and register `luaopen_pb` in `package.preload['pb']`, so `lua/utils/protoc.lua` works. Pin the version and record it.

Out: `fmd.crypto` (T11), `fmd.duktape` (T12), `fmd.strings` (T04), `fmd.env` (T06).

## Seams under test
Lua snippets via the `fmd-lua` runtime:
```lua
local fu = require 'fmd.fileutil'
assert(fu.ExtractFileName('a\\b\\c.jpg') == 'c.jpg' and fu.ExtractFileNameOnly('a/b/c.jpg') == 'c')
local ok, out, err, code = require('fmd.subprocess').RunCommandHide('cmd.exe', '/c', 'echo', 'hi')
assert(ok and out:match('hi') and code == 0)                       -- translated on Linux
assert(require('fmd.gzip').Inflate(GZ_FIXTURE) == 'hello')
local pb = require 'pb'; local protoc = require 'utils.protoc'   -- protoc.lua loads pb
assert(protoc:load('syntax="proto3"; message M { int32 a = 1; }'))
assert(pb.decode('M', pb.encode('M', {a = 5})).a == 5)
```
- `imagepuzzle`: descramble a small fixture image with a known matrix and compare pixels to an expected PNG produced from the Pascal algorithm (derive by hand for a 2×2 grid).
- `nodejs.lua` / `cloudflare.lua` call paths: assert the translated argv (via an injectable process-spawner seam) for `cmd.exe /c node …` and `python lua\websitebypass\cloudflare.py`.

## Acceptance criteria
- [ ] Each lib exposes exactly the names in its Pascal method table.
- [ ] Subprocess never invokes a shell; translation rules documented and tested.
- [ ] `pb` builds via `cc` in CI on Linux and `utils/protoc.lua` + `lua/modules/MangaPlus.lua`'s `require 'pb'` path works.
- [ ] `fmd.pcre2` either implemented (`exec`, `find`, `match`, `gmatch`, `gsub`) or stubbed with a clear error.
- [ ] Doc comments cite the Pascal lines.

## FMD2 references
- `baseunits/lua/LuaGZip.pas:14-58`, `baseunits/GZIPUtils.pas`
- `baseunits/lua/LuaFileUtil.pas:14-46`
- `baseunits/lua/LuaLogger.pas:15-46`
- `baseunits/lua/LuaSubprocess.pas:20-94` (`_runcommand` at :29-62: argv build, return tuple)
- `baseunits/lua/LuaImagePuzzle.pas:21-107`, `baseunits/ImagePuzzle.pas:46-306` (`DeScramble` at :136)
- `baseunits/lua/LuaMangaFox.pas:15-44`, `baseunits/modules/MangaFoxWatermark.pas`, `lua/extras/mangafoxtemplate/`
- `baseunits/lua/LuaPCRE2.pas:92-252`, `baseunits/pcre2.pas`
- `dist/x86_64-win64/pb.dll` (FMD2 ships lua-protobuf as a C module), `lua/utils/protoc.lua:993` (`pcall(require, "pb")`), `lua/modules/MangaPlus.lua` (user)
- `lua/utils/nodejs.lua:41` (`RunCommandHide("cmd.exe", "/c", ...)`), `lua/websitebypass/cloudflare.lua:344` (`lua\websitebypass\cloudflare.py`)
- `docs/LUA-REFERENCE.md:1170-1218` (fileutil, logger, subprocess, mangafoxwatermark)
