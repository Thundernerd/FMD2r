# Cloudflare and anti-bot handling in Docker

Research for [#6](https://github.com/Thundernerd/FMD2r/issues/6), part of map [#1](https://github.com/Thundernerd/FMD2r/issues/1). Feeds [#12 Design the anti-bot and deployment setup](https://github.com/Thundernerd/FMD2r/issues/12).

Upstream baseline: `dazedcat19/FMD2@ad3a5b63`. In this document, `FMD2:` paths point at that commit (`https://github.com/dazedcat19/FMD2/blob/ad3a5b63/<path>`).

## TL;DR

- **The anti-bot mechanism has three layers.**
  1. A Pascal hook wraps every module HTTP call.
  2. A tiny Lua detector (`____CheckAntiBot`) decides whether the response is a challenge.
  3. A Lua dispatcher (`____WebsiteBypass`) picks `cloudflare.lua` or `ddos-guard.lua`.

  After a successful solve, the HTTP object's cookies and user agent are copied into the module's persisted per-website settings.
- **DDoS-GUARD is pure Lua and needs only the Host API.**
- **Cloudflare has two paths.**
  - A legacy pure-Lua IUAM (`jschl`) solver that runs JS through `fmd.duktape`.
  - A "webdriver" path that shells out to `python cloudflare.py`, which only calls **FlareSolverr** over HTTP.
  - FMD2 itself contains no Selenium or undetected-chromedriver code. Those live inside FlareSolverr.
- **rookiepy is dead code in the baseline.** `cloudflare.py` still calls it as a fallback, but since upstream PR #2655 (Dec 2025) `cloudflare.lua` reads only `flaresolver_status_code`/`flaresolver_result` and ignores `rookiepy_result`. It also cannot work headless anyway (no desktop browser profile, no D-Bus secret service).
- **Node.js + Puppeteer is a second, separate browser path.** `lua/utils/nodejs.lua` drives a real headless Chrome through Puppeteer and can also call FlareSolverr. Three modules use it: Comix, RaijinScans, and GourmetScans through the Madara template.
- **Run unmodified, the upstream scripts are Windows-only.** `cloudflare.lua` uses backslash paths (`lua\websitebypass\...`) and the executable name `python`. `nodejs.lua` runs everything through `cmd.exe /c`. On Linux, both paths break unless the FMD2r Host API adds a Windows-compat shim (see [Linux blockers](#linux-blockers-in-unmodified-upstream-scripts)).
- **Recommended Docker shape:**
  - **FlareSolverr as a sidecar container**, reached by service name on a compose network and not published to the internet.
  - **The FMD2r image ships Python 3 with `requests`** for `cloudflare.py`.
  - **For the Node modules, either Node.js + Puppeteer + Chrome inside the FMD2r image** (heavy: needs Chrome shared libs and `SYS_ADMIN` or a non-root user for the sandbox) **or those three modules are declared unsupported in v1.**

## 1. End-to-end path

### 1.1 Pascal hook (every module HTTP request)

1. `TModuleContainer.PrepareHTTP` sets the module's HTTP object up.
   - It sets `AHTTP.OnHTTPRequest := @WebsiteBypassHTTPRequest`.
   - It sets `OnAfterSetHTTPCookies := @MergeHTTPCookiesFromSetting`.
   - If the module's settings are `Enabled`, it applies `Settings.HTTP.UserAgent` and the proxy. (`FMD2: baseunits/WebsiteModules.pas` L354-379)
2. `THTTPSendThread.GET/POST/HEAD/XHR` all go through `InternalHTTPRequest`. That calls `OnHTTPRequest` when it is set. (`FMD2: baseunits/httpsendthread.pas` L487-493, L720-747)
   - The raw `HTTPRequest`, which Lua sees as `HTTP.Request(method, url)`, does **not** go through the hook. (`FMD2: baseunits/lua/LuaHTTPSend.pas` L21-23, L130)
   - The bypass scripts rely on this: they use `HTTP.Request` so they don't recurse into themselves.
3. `WebsiteBypassRequest` (`FMD2: baseunits/lua/LuaWebsiteBypass.pas` L142-209) runs these steps:
   1. It sets `AllowServerErrorResponse := True`, so 5xx responses are not retried away by `DefaultHTTPRequest`, and performs the request.
   2. It calls `CheckAntiBotActive(AHTTP)`. This runs `____CheckAntiBot(HTTP)` in a **single shared Lua state**, loaded once at startup and serialised by a global critical section. A GC runs every 32 calls. (L45-75, L89-116)
   3. If the detector returns true, it takes a **per-module** `TryEnterCriticalsection(Guardian)`.
      - A thread that loses the race does **not** wait. It just re-issues the request once (L197-200).
      - The winner runs `____WebsiteBypass(METHOD, URL)` from `websitebypass.lua` in the module's own `TLuaHandler`, or in a fresh one if the HTTP object has none. `HTTP` is bound as a global (L171).
   4. Before solving, it clears the module's stored cookies: `m.Settings.HTTP.Cookies := ''`.
   5. On success, it does the following (L179-189):
      - sets `m.Settings.Enabled := True`;
      - sets `m.Settings.HTTP.Cookies := AHTTP.Cookies` (CRLF joined to `;`);
      - sets `m.Settings.HTTP.UserAgent := AHTTP.UserAgent`;
      - if the module's `Storage['reload']` contains `true`, resets the HTTP object and re-requests the URL.
4. **Persistence.** `Settings` is a published object (`FMD2: baseunits/WebsiteModulesSettings.pas` L31-44). It is serialised to `userdata/modules.json` together with the module's cookie-manager cookies (`FMD2: baseunits/WebsiteModules.pas` L557-690). So solved cookies and the user agent survive restarts.
5. **Reuse.** On every later request, `MergeHTTPCookiesFromSetting` merges `Settings.HTTP.Cookies` into the request cookies (`WebsiteModules.pas` L278-282; `httpsendthread.pas` `MergeCookies` L769-781). `PrepareHTTP` re-applies the stored user agent.
6. **Manual workaround.** These same two fields are what users fill in by hand under *Options → Websites → Advanced* ([upstream issue #1715, "Manual workaround"](https://github.com/dazedcat19/FMD2/issues/1715)). The FMD2r web UI needs an equivalent per-module editor for `Enabled`, `Cookies` and `UserAgent`. In a headless deployment, this is the universal fallback.

### 1.2 `checkantibot.lua` (detector)

- `FMD2: lua/websitebypass/checkantibot.lua` returns true when all of these hold:
  - the status code is 403, 429 or 503;
  - `Content-Type` contains `text/html`;
  - the `Server` header contains `cloudflare` or `ddos-guard`.
- It uses only `HTTP.ResultCode` and `HTTP.Headers.Values[...]`.
- It runs on every request, in a state with **no** module globals, created by `LuaNewBaseState`.

### 1.3 `websitebypass.lua` (dispatcher)

- `FMD2: lua/websitebypass/websitebypass.lua` switches on the `Server` header again and `require`s `websitebypass.cloudflare` or `websitebypass.ddos-guard`.
- It sets the global `LOGGER = require 'fmd.logger'`.
- It returns `bypass:bypass(METHOD, URL)`.

### 1.4 `ddos-guard.lua`: pure Lua

`FMD2: lua/websitebypass/ddos-guard.lua`:

1. If the body references `://check.ddos-guard.net/check.js`, it sets `HTTP.EnabledCookies = false` and POSTs to `https://check.ddos-guard.net/check.js`.
2. If the response sets a `__ddg*` cookie, it rewrites the cookie domain in `HTTP.Headers.Text` to the target host and calls `HTTP.ParseServerCookies()`.
3. It retries the original request with `HTTP.Request`.

Host API used:

- `HTTP.Document.ToString`
- `HTTP.EnabledCookies`
- `HTTP.Request`
- `HTTP.Headers.Text` (read/write)
- `HTTP.ParseServerCookies`
- `HTTP.RetryCount`
- `HTTP.Terminated`
- `HTTP.Reset`
- `LOGGER.SendWarning`

There is no external dependency, so this works headless as-is.

**Upstream bug:** the retry branch calls `self:sleepOrBreak(2000)`, but `sleepOrBreak` is only defined in `cloudflare.lua`. A failed first attempt therefore raises a Lua error, which the Pascal side logs as `WebsiteBypass.Error`. FMD2r should reproduce the error, not "fix" it, for drop-in compatibility.

### 1.5 `cloudflare.lua`

`FMD2: lua/websitebypass/cloudflare.lua`. Its `bypass()` function (L332-399) does the following:

1. It `require`s `fmd.env`, `fmd.duktape`, `fmd.crypto`, `fmd.subprocess` and `utils.json` into **globals**.
2. `load_config()` (L271-326) reads `lua\websitebypass\websitebypass_config.json`, a relative Windows path.
   - If the file is missing, it **writes** the defaults: `use_webdriver=false`, `testing=false`, `debug=false`, `flaresolverr_ip="localhost"`, `flaresolverr_port=8191`.
3. If `use_webdriver` is set, it probes FlareSolverr with `HTTP.GET('http://ip:port/')` and checks that `json(*)/msg == "FlareSolverr is ready!"` using `CreateTXQuery(...).XPath`.
   - This uses the **hooked** `HTTP.GET` on the same HTTP object, so it overwrites `HTTP.Document` and `HTTP.ResultCode` with FlareSolverr's 200 JSON. The IUAM branch below can then never match in the same run.
4. Up to `max(3, HTTP.RetryCount)` times, it calls `solveChallenge(URL)`:
   - **Webdriver path** (`use_webdriver`): `solveWithWebDriver` (L156-213) runs `subprocess.RunCommandHide('python', 'lua\websitebypass\cloudflare.py', rooturl, '--flaresolverr-ip', ip, '--flaresolverr-port', port [, '--testing'] [, '--debug'])`.
     - It parses stdout as JSON.
     - When `flaresolver_status_code == 200`, it decodes `flaresolver_result`, a JSON string holding a `{cookie_name: value, ..., user_agent: ...}` map, and calls `applyCookies`.
     - It returns `2` even when no cookies came back, meaning "reload".
   - **IUAM path**: if the status is 429/503 and the body has the legacy `challenge-form` with `__cf_chl_jschl_tk__`, `solveIUAMChallenge` (L31-154) runs:
     1. It builds a fake DOM prelude, appends the page's `<script>`, and runs it with `duktape.ExecJS`.
     2. It sleeps for the challenge timeout through `sleepOrBreak`, which uses the global `sleep(250)` and `HTTP.Terminated`.
     3. It POSTs the form with `crypto.EncodeURLElement`.
     4. It succeeds if `HTTP.Cookies.Values['cf_clearance'] ~= ''`.

     This is pure Lua plus the Duktape host module. It targets the old `jschl` IUAM page format, which this codebase has not updated since 2020 (see the #2655 commit message, "has not been update to 5 years").
5. `applyCookies` (L240-269) does the following:
   1. It calls `HTTP.Reset()` and `HTTP.ClearCookiesStorage()`.
   2. For each entry:
      - `user_agent` sets `HTTP.Headers['User-Agent']` and `HTTP.UserAgent`;
      - every other key sets `HTTP.Cookies.Values[k] = v` and also appends to the `cookie` and `Set-Cookie` request headers.
   3. Back in Pascal, `HTTP.Cookies` and `HTTP.UserAgent` are what get copied into `Settings.HTTP` (§1.1 step 3.5). That is how FlareSolverr's cookies and user agent reach later requests.
6. If the result is `2`, it re-requests. When FlareSolverr was in use and the response still contains "Attention Required! | Cloudflare" or "Enable JavaScript and cookies to continue", it sets `MODULE.Storage['reload'] = 'true'`, so Pascal retries once more.
   - This touches the `MODULE` global, so the bypass assumes it runs in a module's Lua handler.

Host API used:

- From `cloudflare.lua`:
  - `HTTP.*`: `GET`, `Request`, `Reset`, `Document`, `ResultCode`, `Headers.Values`, `Cookies.Values`, `UserAgent`, `MimeType`, `FollowRedirection`, `ClearCookiesStorage`, `RetryCount`, `Terminated`;
  - `MODULE.Storage`;
  - `CreateTXQuery` with an `XPath` `json(*)` extension;
  - `sleep`, `print`, `io.open`;
  - `fmd.duktape.ExecJS`, `fmd.crypto.EncodeURLElement`, `fmd.subprocess.RunCommandHide`, `fmd.logger`.
- Plus the `utils.json` Lua file from the module repo.

### 1.6 `cloudflare.py`: FlareSolverr client (+ dead rookiepy fallback)

`FMD2: lua/websitebypass/cloudflare.py`:

- **Imports.** It imports `requests`, which is **required**, and `rookiepy` inside `try`, which is **optional**. Everything else is stdlib.
- **`solve_flare`:**
  1. `GET http://ip:port/` and check `msg == 'FlareSolverr is ready!'`.
  2. Roughly once an hour, `sessions.list` and then `sessions.destroy` the fixed session id `e69e9ce7-…`. The hour is tracked in `temp_cloudflare.json` next to the script.
  3. `POST /v1` with `{"cmd":"request.get","session":<fixed id>,"url":...,"returnOnlyCookies":true}`, or `returnScreenshot` instead in debug mode.
     - FlareSolverr's `request.get` auto-creates a missing session: `SessionsStorage.get` calls `create(session_id)` ([FlareSolverr `src/sessions.py`](https://github.com/FlareSolverr/FlareSolverr/blob/master/src/sessions.py)).
  4. From `solution.cookies[]` (name/value) and `solution.userAgent` it builds the flat map.
- **`solve_rookie`.** It runs only if FlareSolverr returned non-200, including when FlareSolverr is unreachable (code 404). It reads cookies for the URL's host from the local Chrome/Edge/Firefox/Opera/Opera GX profiles and pairs them with a **hard-coded Windows user agent** per browser.
  - **Its result is put in `rookiepy_result`, which `cloudflare.lua` never reads.** The Lua side only consumes `flaresolver_*`.
  - Upstream PR #2655 (commit `27d61639`, "1. Remove rookiepy as its don't help with cloudflare … re-add rookiepy") removed the Lua consumer and only re-added the Python call.
  - Upstream's user guide ([#1715](https://github.com/dazedcat19/FMD2/issues/1715), "Method 2: rookiepy") is therefore stale against the baseline.
- **Debug/testing.** `--debug` and `--testing` write `debug/cloudflare.log`, `json_response.json` and `response_image.png` under the script's directory. `--testing` replays the cookies with `requests` to verify them.
- **Output.** It prints a single JSON object on stdout.

### 1.7 `close_webdrive.py`

- `FMD2: lua/websitebypass/close_webdrive.py` uses `psutil` to kill `chrome`/`chromium` processes whose command line contains `\AppData\Local\Temp`, plus `chromedriver.exe`.
- It is a Windows-only manual helper for FlareSolverr leaving orphaned Chrome processes, added in #2655.
- **Nothing in the repo invokes it** (`grep -rn close_webdrive` finds nothing).
- It is irrelevant for Docker. FlareSolverr's own container owns its Chrome.

### 1.8 `websitebypass_config.json`

`{"use_webdriver": false, "debug": false, "flaresolverr_port": 8191, "testing": false, "flaresolverr_ip": "localhost"}`.

It is read by `cloudflare.lua` through the backslash path and by `nodejs.lua` through the forward-slash path `lua/websitebypass/websitebypass_config.json` (`FMD2: lua/utils/nodejs.lua` L162-180).

- In FMD2 this is the **only** switch for enabling FlareSolverr. There is no GUI option.
- FMD2r must expose it, for example as an app setting that FMD2r writes into this file. Otherwise users have to edit a file inside the module tree, which the module updater may overwrite.

### 1.9 Modules that call bypass functions directly

- **No module `require`s `websitebypass.*`.** The only references outside `lua/websitebypass/` are in `utils/nodejs.lua`, which reads the config.
- Modules don't touch `rookiepy`, `cf_clearance`, `fmd.subprocess` or Python directly. The only `fmd.subprocess` users in `lua/` are `cloudflare.lua` and `utils/nodejs.lua`.
- A few modules just detect Cloudflare and give up with a message, so they need nothing extra:
  - `Comix.lua` L329;
  - `MangaDotNet.lua` L160;
  - `OniSaga.lua` L105 and L149.

  Each sets `MANGAINFO.Title = 'Cloudflare workaround is required'` or prints it.
- **Node.js browser users:**
  - `modules/Comix.lua` (L140, L376, L535) calls `run_html_load_with_js`;
  - `modules/RaijinScans.lua` (L203) calls `run_html_load_with_js`;
  - `templates/Madara.lua` (L169) calls `run_html_load` when `MODULE.Storage['fullpageload']` is set, which only `modules/GourmetScans.lua` sets (L15).

### 1.10 `lua/utils/nodejs.lua`: Puppeteer path

`FMD2: lua/utils/nodejs.lua`:

- **Process launch.** Every process goes through `fmd.subprocess.RunCommandHide("cmd.exe", "/c", ...)` (L39-49). Shell features are used: `cd dir && npm list puppeteer`, `mkdir`.
- **Setup:**
  1. It checks `node -v`.
  2. It creates `lua/utils/npm`. It converts `/` to `\` before running `mkdir` (L67-78).
  3. Unless `npm list` shows it, it runs `npm install puppeteer` there at runtime. That download also fetches Chrome for Testing into Puppeteer's cache, by default `~/.cache/puppeteer` ([Puppeteer `Configuration.cacheDirectory`](https://pptr.dev/api/puppeteer.configuration)).
- **Script run.** It writes `lua/utils/npm/tmp_scrpt.js` and runs `node lua/utils/npm/tmp_scrpt.js`. The JS script does the following:
  1. Launches headless Chrome with `--disable-extensions --disable-webgl --disable-webrtc --disable-background-networking --disable-blink-features=AutomationControlled` and **no `--no-sandbox`** (L261-270).
  2. Seeds the page with:
     - the current FMD2 `HTTP.UserAgent` and `cf_clearance`, which take priority;
     - or, failing that, `MODULE.Storage['puppeteer_cookies'|'puppeteer_ua']`.
  3. Navigates to the URL.
  4. If the page looks like Cloudflare (status 403/429/503 or title match) **and** `use_webdriver` is true, it POSTs FlareSolverr `/v1` `request.get` itself (Node `http`, `fsHost:fsPort`), injects the returned cookies and user agent, and re-navigates.
  5. Runs the module's JS.
  6. Dumps `page.cookies()` and the user agent to `lua/utils/npm/tmp_cookies.json`. Lua reads that file back into `MODULE.Storage`.
- **Cookie flow.** Cookies from this path stay in `MODULE.Storage` for later Puppeteer runs. They are **not** pushed back into `HTTP` or `Settings.HTTP`.
- **`run_js` / `isolatevm_js`.** These run arbitrary JS in Node's `vm` with a 5 s timeout, without a browser, but still need `puppeteer` installed because `install_required_modules` always installs it.

## 2. Linux blockers in unmodified upstream scripts

Modules must run unmodified, so FMD2r has to absorb these in the Host API or in the deployment. Each is a decision for #12 or #3 (Host API):

| # | Blocker | Where | Options |
|---|---|---|---|
| B1 | `io.open([[lua\websitebypass\websitebypass_config.json]])`. On Linux this opens or **creates a file literally named `lua\websitebypass\websitebypass_config.json` in the CWD**, so the shipped config is ignored and `nodejs.lua` reads a different file. | `cloudflare.lua` L272 | Normalise `\` to `/` in `io.open`/`io.*` paths that are relative to the app root, or pre-create the backslash-named file as a symlink. |
| B2 | Script path `lua\websitebypass\cloudflare.py` is passed to Python as an argument, so Python cannot find it. | `cloudflare.lua` L344 | The same normalisation, applied to `fmd.subprocess` arguments. |
| B3 | Executable `python`. Debian and Ubuntu images only ship `python3`, unless the `python-is-python3` package or a symlink adds `python`. | `cloudflare.lua` L343 | Image provides a `python` symlink. |
| B4 | `cmd.exe /c <args…>` | `nodejs.lua` L41 | `fmd.subprocess` maps `cmd.exe /c …` to `sh -c "<joined args>"`. `cd`, `&&`, `mkdir`, `node` and `npm` then behave equivalently. |
| B5 | `mkdir lua\utils\npm` creates a backslash-named directory, while later `cd lua/utils/npm` fails. | `nodejs.lua` L68 | Pre-create `lua/utils/npm` with Puppeteer already installed. The existence check `os.rename(install_dir, install_dir)` uses the backslash name, so either B1-style normalisation also covers `os.rename`, or the `mkdir` call goes through the B4 shim. Depending on how the shim quotes it, `sh` turns unquoted `lua\utils\npm` into `luautilsnpm`. The `mkdir` is harmless but noisy. |
| B6 | All relative paths (`lua/...`, `userdata/...`) assume the process CWD is the FMD2 install root. | everywhere | FMD2r runs Lua and subprocesses with CWD set to the directory that holds `lua/`. That directory must be writable: the scripts write `temp_cloudflare.json`, `debug/`, `npm/` and the config. |

A general rule, "treat `\` as a path separator in host-side file and subprocess APIs", solves B1, B2 and B5 together. It is safe on Linux because module paths never legitimately contain a backslash. This rule belongs in the Host API spec (#3).

## 3. What a headless Docker deployment needs

### 3.1 Per component

| Component | Runs where | Needs | Headless? |
|---|---|---|---|
| `checkantibot.lua`, `websitebypass.lua`, `ddos-guard.lua` | FMD2r Lua runtime | Host API only (§1.4) | Yes |
| `cloudflare.lua` IUAM branch | FMD2r Lua runtime | Host API + `fmd.duktape` | Yes, but it only handles the legacy `jschl` page |
| `cloudflare.lua` webdriver branch → `cloudflare.py` | Subprocess in the FMD2r container | `python` on PATH + `requests` (PyPI) + reachable FlareSolverr + B1-B3/B6 fixes | Yes |
| `rookiepy` fallback | Subprocess | `rookiepy` (PyPI, Rust wheels) | **No.** See §3.4. Its result is also ignored (§1.6). |
| `close_webdrive.py` | Manual only | `psutil` | Not applicable (Windows-only, never called) |
| FlareSolverr | **Sidecar container** | The official image bundles Chrome + undetected-chromedriver + Xvfb | Yes, designed for Docker |
| `utils/nodejs.lua` → Puppeteer | Subprocess in the FMD2r container | `node`, `npm`, the `puppeteer` package, Chrome for Testing **and its shared libs**, Chrome sandbox privileges, B4-B6 fixes | Yes (`headless: true`) |

### 3.2 Python

- **Required packages:** `requests` only, for `cloudflare.py`.
- **Don't ship:** `rookiepy` is useless in a container (§3.4) and dead in Lua (§1.6). `psutil` is only for `close_webdrive.py`.
  - Because `cloudflare.py` wraps `import rookiepy` in `try/except ImportError`, leaving it out is safe. It just reports "rookiepy is not installed" inside `rookiepy_result`, which nobody reads.
- **Python version:** any Python 3 that `requests` supports. Upstream says "Python 3.12 only" for rookiepy, which doesn't apply here.

### 3.3 FlareSolverr: sidecar, not in-image

FlareSolverr's README ([FlareSolverr/FlareSolverr](https://github.com/FlareSolverr/FlareSolverr), latest release v3.5.2, 2026-09-12) supports the sidecar choice:

- **How it works:** "it uses Selenium with the undetected-chromedriver to create a web browser (Chrome) … The HTML code and the cookies are sent back to the user". Selenium and undetected-chromedriver are therefore internal to FlareSolverr. FMD2 never talks WebDriver itself.
- **Recommended install:** "It is recommended to install using a Docker container because the project depends on an external browser that is already included within the image." Images: `ghcr.io/flaresolverr/flaresolverr`, `flaresolverr/flaresolverr`, for amd64/arm64/arm/v7/386.
- **Source installs are worse:** they need Python 3.11, Chrome or Chromium and, on Linux, Xvfb, and are x64-only. Bundling FlareSolverr into the FMD2r image would mean taking on all of that, so a sidecar is clearly better.
- **Security:** "DO NOT expose FlareSolverr to the internet, as it can be abused."
- **Resources:** "Web browsers consume a lot of memory … With each request a new browser is launched". Sessions are the mitigation, and `cloudflare.py` uses a fixed session that it recycles hourly.
- **User agent must match:** "If you want to use Cloudflare clearance cookie in your scripts, make sure you use the FlareSolverr User-Agent too." `applyCookies` + `Settings.HTTP.UserAgent` already do this (§1.5 step 5).
- **Useful env vars:** `LOG_LEVEL`, `TZ`, `LANG`, `PROXY_URL`/`PROXY_USERNAME`/`PROXY_PASSWORD`, `PORT` (default 8191), `HOST` (default 0.0.0.0).
- **Debian hosts:** need `libseccomp2` 2.5.x.

**Networking.**

- Put both services on one compose network.
- Set `flaresolverr_ip` to the service name (e.g. `flaresolverr`) and `flaresolverr_port` to `8191`. Python `requests`, Node `http.request` and FMD2r's own `HTTP.GET('http://'..ip..':'..port..'/')` all resolve DNS names, so a hostname works in the `ip` field.
- Don't publish 8191, or publish it only to `127.0.0.1`.
- **Egress parity (unverified assumption):** FlareSolverr and FMD2r should leave through the **same public IP and proxy**. Cloudflare's cookie reference ([cloudflare-cookies](https://developers.cloudflare.com/fundamentals/reference/policies-compliances/cloudflare-cookies/)) does not document whether `cf_clearance` is IP-bound, so treat this as a precaution, not a documented fact.
  - If a module uses a per-website proxy (`Settings.HTTP.Proxy`), FlareSolverr has no equivalent, because `cloudflare.py` never forwards a proxy.
  - A VPN sidecar setup should route both containers through it.

Minimal compose sketch:

```yaml
services:
  fmd2r:
    image: fmd2r
    volumes: ["./data:/data"]        # holds lua/ (module tree) + userdata/
    depends_on: [flaresolverr]
  flaresolverr:
    image: ghcr.io/flaresolverr/flaresolverr:latest
    environment: [LOG_LEVEL=info, TZ=Europe/Amsterdam]
    restart: unless-stopped          # no ports: → reachable only on the compose network
```

FMD2r then writes `{"use_webdriver": true, "flaresolverr_ip": "flaresolverr", "flaresolverr_port": 8191, ...}` into the module tree's `websitebypass_config.json`, at the path that B1 resolves to.

### 3.4 What cannot work headless

- **rookiepy (reading desktop browser cookies).**
  - rookie reads cookies from browser profile databases on the local machine.
  - On Linux, Chromium-family cookies are decrypted with a key fetched over the **D-Bus session bus** from the Secret Service (`org.freedesktop.secrets`, schemas `chrome_libsecret_os_crypt_password_v1/v2`) or KWallet (`org.kde.kwalletd5`) ([rookie `rookie-rs/src/linux/mod.rs`](https://github.com/thewh1teagle/rookie/blob/main/rookie-rs/src/linux/mod.rs)).
  - A server container has no browser profile, no user session bus and no keyring.
  - Even bind-mounting a desktop profile would yield the **host desktop's** browser user agent and IP context. `cloudflare.py` also substitutes a hard-coded *Windows* user agent.
  - It is also unreachable, because Lua ignores its output (§1.6).
  - **Verdict:** drop it. The headless equivalent is the manual cookie + user-agent paste in the web UI (§1.1 step 6).
- **Interactive challenges / captchas.**
  - FlareSolverr: "At this time none of the captcha solvers work … FlareSolverr will return the error `Captcha detected but no automatic solver is configured.`" (README, Captcha Solvers).
  - Turnstile/captcha-gated sites can only be handled by the manual cookie paste, from a browser on the same egress IP.
- **`close_webdrive.py`:** Windows process paths, never called.

### 3.5 Node.js / Puppeteer (three modules)

If v1 supports Comix, RaijinScans and GourmetScans, the FMD2r image needs:

- `node` + `npm` on PATH, plus the B4 `cmd.exe` shim.
- `puppeteer` **pre-installed** in `lua/utils/npm` (B5), so that runtime `npm install` doesn't need network access or a writable home directory. Puppeteer downloads Chrome for Testing to `PUPPETEER_CACHE_DIR` (default `~/.cache/puppeteer`). Alternatively, set `PUPPETEER_SKIP_DOWNLOAD` and point `PUPPETEER_EXECUTABLE_PATH` at a distro Chromium ([Configuration](https://pptr.dev/api/puppeteer.configuration)).
- **Chrome shared libraries.** "the bundled Chrome for Testing that Puppeteer installs is missing the necessary shared library dependencies" on Linux ([Troubleshooting](https://pptr.dev/troubleshooting)).
- **Sandbox privileges.** `nodejs.lua` does not pass `--no-sandbox`. The Chrome sandbox therefore has to work in the container. Puppeteer's own image "requires the `SYS_ADMIN` capability" and an init process (`--init`) ([Docker guide](https://pptr.dev/guides/docker)). Its Dockerfile also runs as a non-root `pptruser` ("Add user so we don't need --no-sandbox").
  - FMD2r must run module subprocesses as non-root and grant `SYS_ADMIN` (or an equivalent seccomp/userns setup).
  - Alternatively, the shim must inject `--no-sandbox`, which can't be done without modifying the generated JS. That makes it impractical.
- **Alternative:** declare these three modules "needs Node.js" and unsupported in the default image, perhaps with an opt-in `-full` image variant. The image-size and privilege cost is large for three modules.

Ties to #5 (JS engine / native library usage) and #12.

### 3.6 Host API surface this exercises (for #3)

- **HTTP object:**
  - methods: `Request` (unhooked), `GET` (hooked), `Reset`, `ClearCookiesStorage`, `ParseServerCookies`;
  - properties: `Document`, `ResultCode`, `Headers.Values`/`Headers.Text` (writable), `Cookies.Values`, `UserAgent`, `MimeType`, `FollowRedirection`, `EnabledCookies`, `RetryCount`, `Terminated`.
- **Module and globals:** `MODULE.Storage`, global `sleep`, `CreateTXQuery` with `json(*)`.
- **Host modules:** `fmd.duktape.ExecJS` (returns two values), `fmd.crypto.EncodeURLElement`, `fmd.subprocess.RunCommandHide` (returns `ok, stdout, stderr, exitcode`; `ok` is false on a non-zero exit — `FMD2: baseunits/lua/LuaSubprocess.pas` L28-62), `fmd.logger`, `fmd.env`.
- **Lua repo files:** `utils.json`.
- **Hook semantics to replicate:**
  - `AllowServerErrorResponse` during the first attempt;
  - the shared detector state;
  - per-module non-blocking `TryEnter` (losers retry once);
  - clearing `Settings.HTTP.Cookies` before solving;
  - copying cookies and user agent into persisted settings on success, and auto-enabling the module settings;
  - the `Storage['reload']` re-request.

## Sources

- **Upstream FMD2 at `ad3a5b63`:**
  - `baseunits/lua/LuaWebsiteBypass.pas`
  - `baseunits/httpsendthread.pas`
  - `baseunits/WebsiteModules.pas`
  - `baseunits/WebsiteModulesSettings.pas`
  - `baseunits/lua/LuaHTTPSend.pas`
  - `baseunits/lua/LuaSubprocess.pas`
  - `lua/websitebypass/*`
  - `lua/utils/nodejs.lua`
  - `lua/modules/{Comix,RaijinScans,GourmetScans,MangaDotNet,OniSaga}.lua`
  - `lua/templates/Madara.lua`
  - commit `27d61639` (PR #2655)
  - `changelog.txt` (2.0.32.0, 2.0.33.1)
- **Upstream user guide:** [dazedcat19/FMD2#1715](https://github.com/dazedcat19/FMD2/issues/1715)
- **FlareSolverr:**
  - [README](https://github.com/FlareSolverr/FlareSolverr) (v3.5.2)
  - [`src/sessions.py`](https://github.com/FlareSolverr/FlareSolverr/blob/master/src/sessions.py)
- **rookie / rookiepy:**
  - [README](https://github.com/thewh1teagle/rookie)
  - [`rookie-rs/src/linux/mod.rs`](https://github.com/thewh1teagle/rookie/blob/main/rookie-rs/src/linux/mod.rs)
- **Puppeteer:**
  - [Troubleshooting](https://pptr.dev/troubleshooting)
  - [Docker guide](https://pptr.dev/guides/docker)
  - [Configuration](https://pptr.dev/api/puppeteer.configuration)
- **Cloudflare:** [Cloudflare cookies reference](https://developers.cloudflare.com/fundamentals/reference/policies-compliances/cloudflare-cookies/)
