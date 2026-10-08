# FMD2r

Rust port of FMD2 with a web UI. See `docs/plan.md` for the plan and `docs/tickets/` for the work.

The upstream Lua modules used by the tests live in `fixtures/lua`; see `fixtures/README.md`.

## Cloudflare-protected sites

After every module request FMD2r runs upstream's `lua/websitebypass/checkantibot.lua`; on a
Cloudflare or DDoS-Guard challenge it runs `websitebypass.lua` and keeps the cookies and user
agent it obtains in the module's settings. Cloudflare challenges need
[FlareSolverr](https://github.com/FlareSolverr/FlareSolverr):

```sh
docker compose -f compose.dev.yaml up -d
cargo run -p fmd2r -- serve --flaresolverr-url http://localhost:8191
```
