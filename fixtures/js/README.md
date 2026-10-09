# `js/`: pages for the `ExecJS` comparison

Real pages that upstream modules hand to `fmd.duktape.ExecJS`.
`crates/fmd-lua/tests/duktape_reference.rs` runs the modules on them and checks every script
against Duktape 2.3.0 (`docs/duktape-differences.md`). The FanFox comparison uses the smoke
recording, `smoke/fanfox/pages`.

| Directory | What | Source |
|---|---|---|
| `acqqcom/` | `modules/acqqcom.lua` `getpagenumber` on `https://ac.qq.com/ComicView/index/id/531490/cid/1`: the chapter page and its chapter script | Recorded 2026-10-09 with `fmd2r module pages <url> --module f48e28dc51bb4abd813337e3ab08a849 --record fixtures/js/acqqcom` (format: `docs/fixtures.md`) |
| `readcomiconline/` | `modules/ReadComicOnline.lua` `GetPageNumber` on `/Comic/51/Issue-4?id=245214` | The site no longer resolves. The page is the Wayback Machine's capture of `https://rcostation.xyz/Comic/51/Issue-4?id=245214` from 2026-06-28 (`https://web.archive.org/web/20260628053923id_/…`, the original bytes), written by hand into the recorder's format under the URL the module requests |
| `cloudflare/` | Cloudflare IUAM ("I'm Under Attack Mode") challenge pages for `websitebypass/cloudflare.lua` `IUAMChallengeAnswer` | Captured in May 2020 by the cloudscraper project: `tests/fixtures/js_challenge-27-05-2020.html`, `js_challenge1_16_05_2020.html` and `js_challenge2_16_05_2020.html` from https://github.com/VeNoMouS/cloudscraper (commit 9ea528a8675f1bebd49ff853d142e94988a95178), MIT licensed (`cloudflare/LICENSE`). Cloudflare no longer serves this challenge, so none can be captured today |

The ac.qq.com chapter script is no longer one the module can unpack, because the site changed
how it is packed. The test runs that step again with the site's current marker; see its doc
comment.
