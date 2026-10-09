# HTTP fixtures (record/replay)

`fmd2r module info` and `fmd2r module pages` can record every HTTP exchange a module makes and
replay them later without a network. The smoke list (`fixtures/smoke`, see `fixtures/README.md`)
and the XPath corpus (T35) are built on this.

```sh
# Run against the live site and record what it sends and receives.
fmd2r module info https://example.com/manga/1 --lua-dir lua --record fixtures/http/example

# Run offline from the recording; fails if the module makes a request that wasn't recorded.
fmd2r module info https://example.com/manga/1 --lua-dir lua --replay fixtures/http/example
```

The transports behind the flags are `fmd_http::RecordingTransport` and
`fmd_http::ReplayTransport`. Both sit below the Synapse layer (redirects, retries, cookies,
decoding), so a replay runs the same client code as a live run.

## Directory layout

```
<dir>/
  index.json               every exchange, in the order the requests were sent
  exchanges/0001.json      one file per exchange
  bodies/0001.request      request body, raw bytes (only when non-empty)
  bodies/0001.response     response body, raw bytes, decoded (only when non-empty)
```

`--record` replaces `index.json`, `exchanges/` and `bodies/` in the directory and leaves
anything else there alone. Exchanges are numbered when their request is sent.

### `index.json`

```json
{
  "format": 1,
  "exchanges": [
    { "id": "0001", "method": "GET", "url": "https://example.com/manga/1", "status": 200 }
  ]
}
```

`status` is `null` for an exchange that failed at the transport level (connection refused,
timeout, TLS error, ...).

### `exchanges/<id>.json`

```json
{
  "request": {
    "method": "POST",
    "url": "https://example.com/api",
    "headers": [["User-Agent", "Mozilla/5.0 ..."], ["Content-Type", "application/x-www-form-urlencoded; charset=UTF-8"]],
    "body": "bodies/0002.request"
  },
  "response": {
    "status": 200,
    "reason": "OK",
    "headers": [["content-type", "text/html"], ["content-encoding", "br"]],
    "body": "bodies/0002.response",
    "decoded_from": "br"
  }
}
```

A failed exchange has `"error": "<transport error message>"` instead of `"response"`.

Headers are `[name, value]` pairs in wire order. `body` is a path relative to the fixture
directory, or `null` for an empty body. Bodies are stored as raw bytes, so binary content (images,
protobuf, encrypted payloads) round-trips unchanged.

## Decoded bodies

Response bodies are stored **decoded**. When a response has a `Content-Encoding` (gzip, deflate,
br or zstd), the recorder decodes the body the way the HTTP client does after an exchange
(`baseunits/httpsendthread.pas:681-710`) and stores the result. `decoded_from` holds the original
`Content-Encoding` value, and the recorded headers still include it.

On replay, the `Content-Encoding` header is left out of a response whose body was decoded, so the
client doesn't decode it a second time. A module therefore sees no `Content-Encoding` in
`HTTP.Headers` during a replay. A body that failed to decode is stored as received; the client
keeps such a body unchanged as well, so the module sees the same bytes either way.

## Matching

A request is answered by the recorded exchanges with the same method, URL (after the client's
normalisation) and body bytes. Request headers are ignored by default, because they hold values
that change between runs (cookies, user agents). `--match-header NAME` (repeatable) makes a
header part of the match: the names are compared ignoring ASCII case and the values with
surrounding blanks trimmed.

When several recorded exchanges match the same request, they answer in recording order. Once
they're used up, the last one answers every further repeat.

A request with no matching exchange fails like a network error, so `HTTP.GET` returns false.
The command then exits non-zero and prints `replay: no recorded exchange for <METHOD> <URL>` for
each such request.

## Subprocesses

`--record` also records every process a module starts through `fmd.subprocess` (after the
Windows command translation, `lua/utils/nodejs.lua`'s `node` and `npm` runs for instance), and
`--replay` answers them from the recording without starting anything. A module that runs node
fetches pages from inside that process, past the recorded HTTP; with its processes replayed it
runs offline too. The transports behind this are `fmd_lua::subprocess::RecordingSpawner` and
`ReplaySpawner`.

The calls are kept in `<dir>/subprocess.json`, written only when a process ran (`--record`
removes the one a previous recording left):

```json
{
  "format": 1,
  "calls": [
    { "program": "node", "args": ["-v"], "output": { "stdout": "v24.18.0\n", "stderr": "", "status": 0 } }
  ]
}
```

`stdout` and `stderr` are strings when they are UTF-8, else arrays of bytes. A process that
could not start has `"error": "<message>"` instead of `"output"`. A call is answered by the
recorded calls with the same program and arguments (not the directory it runs in), in
recording order, the last one repeating once they're used up, like HTTP exchanges. A call with
no recording fails to start, so the module sees the program as missing; the command then exits
non-zero and prints `replay: no recorded call for <PROGRAM ARGS...>` for each.

Files a process writes for the module to read (nodejs.lua's `tmp_cookies.json`) are not
recorded; the module goes on without them, as when the process writes none.

## Output

`module info` prints the `OnGetInfo` status and `MANGAINFO` fields as the callback left them, as
JSON. FMD2's later cleanup of those fields (`GetInfoFromURL`, `baseunits/uData.pas:111-206`) is
not applied. `module pages` prints the page number, page links and page container links after
`OnTaskStart`, `OnGetPageNumber` and `OnGetImageURL`, as JSON, with each callback's result
(`task_start` and `get_page_number`, `null` when it did not run; `get_image_url`, one per call).
It exits non-zero after printing when no page link resolved (every link is FMD2's unresolved `W`,
or a `DynamicPageLink` module found no pages). A `false` from `OnGetPageNumber` alone is not a
failure: FMD2 ignores it, and modules such as MangaDex return it on chapters that work. `module init --json` lists the
modules sorted by ID and the load failures sorted by file. The output contains no timestamps
or other run-dependent values, so a replayed run prints exactly what the recorded run printed
and the output can be used in snapshot tests.
