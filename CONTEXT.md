# FMD2r

A Rust port of FMD2 (Free Manga Downloader 2) that finds, downloads and tracks manga from websites. Site support comes from Lua modules shared with upstream FMD2, and users work through a web interface.

## Language

**Upstream**:
The original FMD2 project and its community-maintained Lua module repository, which FMD2r stays compatible with.
_Avoid_: original, FMD2 (when meaning the module source)

**Module**:
A Lua script from upstream that teaches FMD2r how to work with one website (or a family of websites). FMD2r runs upstream modules unmodified.
_Avoid_: plugin, connector, extension, source

**Host API**:
The set of globals, objects and functions FMD2r exposes to modules, matching upstream FMD2 behaviour so modules run unmodified.
_Avoid_: Lua bindings, SDK

**Runs cleanly**:
A module loads, and its hooks run without Lua errors or calls to Host API functions FMD2r lacks. Every upstream module must meet this bar.

**Matches upstream**:
For the same inputs, a module running on FMD2r gives the same results it gives on upstream FMD2. Checked directly for a curated set of modules, and for the rest through Host API conformance.
_Avoid_: compatible (without saying which bar)
