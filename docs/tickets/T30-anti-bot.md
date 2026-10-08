# T30: Anti-bot: post-request hook, websitebypass flow, cookie/UA persistence, FlareSolverr
Deps: T10, T12, T13

## Goal
Reproduce FMD2's anti-bot handling so Cloudflare/DDoS-Guard-protected sites work: after every HTTP request run upstream's `checkantibot.lua`, and when it fires run `websitebypass.lua` (which `require`s the Cloudflare/DDoS-Guard bypass scripts and may call FlareSolverr), then persist the obtained cookies/UA in the module's settings and retry.

## Scope (in/out)
In:
- Post-request hook in the HTTP layer (T09/T10), applied to every module request:
  1. In a dedicated, per-process Lua state (cached), call `____CheckAntiBot(HTTP)` from `lua/websitebypass/checkantibot.lua` with the just-completed request's `HTTP` object. Must be cheap (runs for every request).
  2. If true: serialise per module (try-enter a per-module bypass guard; if another thread is already bypassing, wait for it and then simply retry the request).
  3. Clear the module's stored settings cookies, then in a Lua state set up like a module callback (with `HTTP`, `MODULE`, `fmd.*` libs) call `____WebsiteBypass(METHOD, URL)` from `websitebypass.lua`.
  4. On success: mark the module's HTTP settings enabled, store the cookies (joined with `;`) and the user agent into module settings (persisted via `ModuleSettingsRepo`), and if `MODULE.Storage['reload']` contains `true`, re-issue the original request; return its result.
- `websitebypass_config.json` honoured as upstream expects (FlareSolverr IP/port, webdriver off); FMD2r writes/overrides it from settings (`flaresolverr_url`) on startup.
- Docker compose (dev file now; T33 finalises): FlareSolverr sidecar service and config pointing at it.
- Subprocess translation (T13) must make `cloudflare.lua`'s Python/webdriver path work when enabled (python3 present in Docker).

Out: Docker image itself (T33).

## Seams under test
- Lua-level through the `fmd-lua` runtime + `fmd-http` with a stub server and the real upstream `websitebypass/*.lua` files from the fixture corpus:
  - Stub returns 503 + `Server: cloudflare` + `text/html` → `____CheckAntiBot` true → bypass invoked (replace `websitebypass.lua` with a fixture script in a temp lua dir that sets a cookie and returns true) → module settings now hold that cookie and UA; with `MODULE.Storage['reload'] = 'true'` the original GET is retried and returns the 200 body.
  - 200 responses never call the bypass (count calls).
  - Two threads hitting the challenge concurrently → bypass runs once.
- A FlareSolverr-protocol stub (`POST /v1` with `cmd: request.get`) used by the real `cloudflare.lua` in one integration test (if the script's flow allows it offline).

## Acceptance criteria
- [ ] Hook order, guard, cookie/UA persistence and reload retry match `LuaWebsiteBypass.pas:142-212`.
- [ ] CheckAntiBot cost measured (micro-bench) and acceptable (< 50 µs typical).
- [ ] FlareSolverr sidecar in compose with working default config.

## FMD2 references
- `baseunits/lua/LuaWebsiteBypass.pas:45-88` (init: load checkantibot/websitebypass), `:89-117` (`CheckAntiBotActive`), `:118-141` (`WebsiteBypassGetAnswer`), `:142-212` (`WebsiteBypassRequest`: guard, clear cookies, store cookies/UA, `Storage['reload']` retry), `:213` (`TWebsiteBypass.Create`)
- `baseunits/WebsiteModules.pas:272-283` (`WebsiteBypassHTTPRequest`, `MergeHTTPCookiesFromSetting`)
- `baseunits/httpsendthread.pas:487-495` (`InternalHTTPRequest`: where the hook runs)
- `lua/websitebypass/checkantibot.lua`, `lua/websitebypass/websitebypass.lua`, `lua/websitebypass/cloudflare.lua`, `lua/websitebypass/ddos-guard.lua`, `lua/websitebypass/cloudflare.py`, `lua/websitebypass/websitebypass_config.json`
