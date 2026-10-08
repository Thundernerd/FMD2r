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
