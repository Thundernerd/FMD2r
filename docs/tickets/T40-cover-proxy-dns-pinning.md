# T40: Cover proxy: pin the checked address (DNS rebinding)
Deps: none

## Goal
The cover proxy's SSRF guard resolves the host, checks the address, then `fmd-http`'s transport resolves the host again, so a DNS-rebinding host can pass the check and then connect to a private address (PR #43, "Known limits"). Connect to the address that was checked.

## Scope (in/out)
In:
- `fmd-http`: a way to send a request to a pre-resolved `SocketAddr` while keeping the URL's host for TLS SNI, certificate checks and the `Host` header (reqwest `resolve`/`resolve_to_addrs` per request or a per-session resolver override).
- `crates/fmd-server/src/covers`: resolve once, check every address, then pin the chosen one for the request and for each manually followed redirect hop.
- Keep the module's proxy settings working: when a proxy is configured the proxy resolves the host, so document that the guard then checks only the proxy target.

Out: SSRF rules for other endpoints.

## Seams under test
HTTP handler via `oneshot` with a stub resolver that answers a public address on the first lookup and `127.0.0.1` on the second: the request is refused or goes to the first address, never to loopback. Existing `tests/covers.rs` cases still pass.

## Acceptance criteria
- [ ] Rebinding test fails on current main and passes after the change.
- [ ] No extra DNS lookup per cover request.

## FMD2 references
- None (FMD2 has no proxy endpoint); see `crates/fmd-server/src/covers` docs.
