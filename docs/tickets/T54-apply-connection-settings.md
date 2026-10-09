# T54: Apply the connection settings to the HTTP client
Deps: none

## Goal
Found by the 2026-10-09 end-to-end verification (F4). With `connections.proxy` enabled (also after a restart), `POST /api/resolve` and a full chapter download made no connection through the proxy. `fmd_http::Client` has the setters (`set_default_user_agent`, `set_default_retry_count`, `set_default_timeout`, `set_default_proxy`, `crates/fmd-http/src/client.rs:158-176`), but they are only called from tests. The runtime client is built with `HttpClient::new()` (`crates/fmd-lua/src/pool/host_api.rs:46`, also `crates/fmd-server/src/module_updates.rs:68`) and keeps its built-in defaults; nothing copies `settings.connections` (`crates/fmd-core/src/settings/model.rs:69-110`: `retry_count`, `timeout_secs`, `user_agent`, `proxy`) into it. Only the proxy was verified, but the other three can't take effect either.

FMD2 applies these in `TMainForm.ApplyOptions` (`mangadownloader/forms/frmMain.pas:6169`, :6267-6295), on start and whenever options are saved. Retry count, timeout and proxy apply to every existing session as well (`SetDefault*AndApply`, `baseunits/httpsendthread.pas:332-392`); the user agent only to sessions created afterwards (`DefaultUserAgent`).

## Scope (in/out)
In:
- In `serve`, apply `settings.connections` (user agent, retry count, timeout, proxy) to the shared `fmd_http::Client` at startup and on every settings change, following the existing settings watchers in `crates/fmd-server/src/serve.rs` (the FlareSolverr one at :272, `xpath.backend` from T37).
- The same client (or the same settings) for the module updater's client (`module_updates.rs:68`) and the cover proxy, if they don't share it already.
- A disabled proxy maps to `None`.

Out: per-module overrides (already handled in `crates/fmd-core/src/settings/module_overrides.rs`). The settings model itself.

## Seams under test
Through `fmd-server`'s public `serve(ServeConfig)` on a temp data dir with a fixture module and a local stub site, plus a local HTTP proxy stub:
- With `connections.proxy` enabled at startup, a `POST /api/resolve` that needs a module request reaches the stub site through the proxy.
- After `PATCH /api/settings` enables (then disables) the proxy, without a restart, the next request goes through (then around) the proxy.
- A changed `connections.user_agent` is the UA the stub sees on a new session.

## Acceptance criteria
- [ ] No runtime `HttpClient` is left with built-in defaults when `settings.connections` says otherwise.
- [ ] Proxy, retry count and timeout changes take effect without a restart, as in FMD2.
- [ ] fmt, clippy `-D warnings`, `cargo test --workspace` pass.

## FMD2 references
- `mangadownloader/forms/frmMain.pas:6169` (`ApplyOptions`), :6267-6295
- `baseunits/httpsendthread.pas:332-392` (`SetDefaultProxyAndApply`, `SetDefaultTimeoutAndApply`, `SetDefaultRetryCountAndApply`)
