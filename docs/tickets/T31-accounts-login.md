# T31: Accounts and login
Deps: T14, T17

## Goal
Support modules with `AccountSupport`: store credentials encrypted at rest, run `OnLogin`/`OnAccountState` like FMD2, expose `MODULE.Account` with persisted state, and offer account management via API (and a minimal UI section).

## Scope (in/out)
In:
- Account storage via `AccountRepo` (T17): username, password, cookies, enabled, status (`asUnknown`, `asChecking`, `asValid`, `asInvalid`). Encryption at rest with a key in the data dir (e.g. XChaCha20-Poly1305), key generated on first run with 0600 permissions; document the threat model (protects DB copies/backups, not a compromised host).
- `MODULE.Account` (T06) backed by the repo: reads current values; writes from Lua (e.g. `MODULE.Account.Cookies = …`, `Status = asValid`) persist.
- Login flow in `fmd-core`: `AccountService::login(module_id)` runs `OnLogin` in a worker (HTTP global bound, account status set to `asChecking` during), then `OnAccountState` when defined; result status stored and emitted as an event. Automatic login before tasks/info calls for modules whose account is enabled but not valid (match when FMD2 triggers it).
- Endpoints: `GET /api/accounts`, `PUT /api/accounts/{module}` (username/password/enabled), `POST /api/accounts/{module}/login`, `DELETE /api/accounts/{module}`; passwords write-only in API responses.
- Minimal UI: an "Accounts" subsection in Settings (if T27 merged) or System page listing modules with account support, status chip, edit, login.

Out: importing FMD2 accounts (T32 uses `DecryptString` from T11 and this repo).

## Seams under test
- Public `fmd-core` `AccountService` with a fixture module:
```lua
function Login()
  if MODULE.Account.Username == 'u' and MODULE.Account.Password == 'p' then
    MODULE.Account.Cookies = 'sid=1'; MODULE.Account.Status = asValid; return true
  end
  MODULE.Account.Status = asInvalid; return false
end
```
  `login` with u/p → status Valid, cookies `sid=1` persisted (encrypted in DB: raw SQL read shows no `sid=1`); wrong password → Invalid.
- HTTP handlers via `oneshot`: `PUT` then `GET` omits the password; `POST …/login` returns status.

## Acceptance criteria
- [ ] Status constants and callback globals match `LuaWebsiteModules.pas:412-466`, `:832-838`.
- [ ] Credentials never logged and never returned by the API.
- [ ] Encryption at rest verified by test.

## FMD2 references
- `baseunits/lua/LuaWebsiteModules.pas:412-466` (`DoLogin`, `DoAccountState`, `DoCheckSite`), `:832-838` (`as*` constants), `:951-989` (AccountSupport getter/setter, Account metatable)
- `baseunits/WebsiteModules.pas:78-98` (`TAccountStatus`, `TWebsiteModuleAccount`), `:258-271` (`SetAccountSupport`), `:600-615`, `:665-675` (account fields in `modules.json`, encrypted with `EncryptString`)
- `mangadownloader/forms/frmAccountManager.pas`, `mangadownloader/forms/frmAccountSet.pas` (FMD2 account UI)
- `docs/LUA-REFERENCE.md:257-275`, `:697-796`, `:1355-1389` (account properties, login/account-state/check-site callbacks, account pattern)
