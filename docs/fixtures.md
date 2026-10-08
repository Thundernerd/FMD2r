# HTTP fixtures (record/replay)

`fmd2r module info` and `fmd2r module pages` can record every HTTP exchange a module makes and
replay them later without a network. The smoke tests (T16) and the XPath corpus (T35) are built
on this.

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

## Output

`module info` prints the `OnGetInfo` status and `MANGAINFO` fields as the callback left them, as
JSON. `module pages` prints the page number, page links and page container links after
`OnTaskStart`, `OnGetPageNumber` and `OnGetImageURL`, as JSON. `module init --json` lists the
modules sorted by ID and the load failures sorted by file. The output contains no timestamps
or other run-dependent values, so a replayed run prints exactly what the recorded run printed
and the output can be used in snapshot tests.
