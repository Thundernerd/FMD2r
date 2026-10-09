#!/usr/bin/env python3
"""T71 probe: match a sample of FMD2-DB list titles against MangaBaka and AniList.

Throwaway research code, not part of FMD2r. See docs/research/metadata-sources.md for the
results and docs/research/metadata-probe/README.md for how to rerun it.

    probe.py index    # MangaBaka dump (series.jsonl.zst) -> work/mangabaka.sqlite
    probe.py sample   # FMD2-DB dumps -> work/sample-71.json (50 seeded titles per list)
    probe.py match    # every route over the sample -> results-71.csv + summary-71.json
    probe.py review   # accepted matches side by side, for the wrong-match check by hand

`--seed 72` (or 73) on sample and match draws and matches a held-out sample: no titles in
common with the samples drawn before it.

Routes (each a separate column in results-<seed>.csv):
    dump      offline, against the local MangaBaka dump: site link, then cross-site IDs
              (MangaDex module only, one MangaDex API call per title), then title + authors.
    mb_api    MangaBaka's /v1/series/match, one call per title (title alone, as a user would).
    anilist   AniList GraphQL Page.media(search:), one call per title.
"""

import argparse
import hashlib
import json
import os
import random
import re
import sqlite3
import subprocess
import sys
import time
import unicodedata
import urllib.parse
import urllib.request
from difflib import SequenceMatcher

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.path.join(HERE, "work")
UA = "FMD2r-T71-probe/0.1 (research; https://github.com/Thundernerd/FMD2r)"

# FMD2 module id -> (name, root URL). All four have FMD2-DB dumps.
LISTS = {
    "d07c9c2425764da8ba056505f57cf40c": ("MangaDex", "https://mangadex.org"),
    "23eb3a472201427e8824ecdd5223bad7": ("MangaFire", "https://mangafire.to"),
    "7103ae6839ea46ec80cdfc2c4b37c803": ("AsuraScans", "https://asurascans.com"),
    "18f636ec7fdf47fabe95d940ad0b548f": ("WebToons", "https://www.webtoons.com"),
}
PER_LIST = 50
DEFAULT_SEED = 71
DB_URL = "https://raw.githubusercontent.com/dazedcat19/FMD2-DB/master/7z/{}.7z"
DUMP_URL = "https://api.mangabaka.org/v1/database/series.jsonl.zst"

REQUESTS = {}  # route -> {"total": n, "cached": n}


# --- normalisation ------------------------------------------------------------------------

BRACKETS = re.compile(r"[\(\[\{（【][^\)\]\}）】]*[\)\]\}）】]")


def norm(s):
    """Lower-case, strip accents and punctuation, collapse spaces."""
    s = unicodedata.normalize("NFKC", s or "").casefold()
    s = "".join(c for c in unicodedata.normalize("NFKD", s) if not unicodedata.combining(c))
    s = s.replace("&", " and ").replace("’", "").replace("'", "")
    s = re.sub(r"[^\w]+", " ", s)
    return " ".join(s.split())


def title_keys(title):
    """The normalised title, and the title without decorations such as (Official), [EN]."""
    keys = [norm(title)]
    bare = norm(BRACKETS.sub(" ", title or ""))
    if bare and bare not in keys:
        keys.append(bare)
    return [k for k in keys if k]


def person_keys(names):
    """Name sets, order-insensitive: 'Togawa Hanamaru' == 'Hanamaru Togawa'."""
    out = set()
    for n in re.split(r"[,;/、]| and ", names or ""):
        toks = norm(BRACKETS.sub(" ", n)).split()
        if toks:
            out.add("".join(sorted(toks)))
    return out


def people_agree(a, b):
    """Any shared person, allowing for romanisation differences (Hyun-woo / Hyeon-woo) and
    syllables in another order (Soboro / Boroso, Legobalbasseo / Balbasseolego)."""
    return any(same_person(x, y) for x in a for y in b)


def same_person(x, y):
    if x == y or SequenceMatcher(None, x, y).ratio() >= 0.75:
        return True
    return len(x) >= 4 and sorted(x) == sorted(y)


# --- HTTP ---------------------------------------------------------------------------------


def http_json(route, url, body=None, headers=None, min_interval=0.0, _last={}):
    """GET (or POST `body`) JSON, cached in work/http-cache so a rerun with changed matching
    rules sends nothing. REQUESTS counts what went over the network."""
    key = hashlib.sha256(json.dumps([url, body]).encode()).hexdigest()
    cache = os.path.join(WORK, "http-cache", key + ".json")
    if os.path.exists(cache):
        with open(cache) as f:
            return json.load(f)
    result = fetch_json(route, url, body, headers, min_interval, _last)
    os.makedirs(os.path.dirname(cache), exist_ok=True)
    with open(cache, "w") as f:
        json.dump(result, f)
    return result


def fetch_json(route, url, body, headers, min_interval, _last):
    wait = _last.get(route, 0) + min_interval - time.monotonic()
    if wait > 0:
        time.sleep(wait)
    h = {"User-Agent": UA, "Accept": "application/json"}
    h.update(headers or {})
    data = json.dumps(body).encode() if body is not None else None
    if data is not None:
        h["Content-Type"] = "application/json"
    for attempt in range(5):
        req = urllib.request.Request(url, data=data, headers=h)
        stat = REQUESTS.setdefault(route, {"total": 0, "cached": 0, "429": 0})
        stat["total"] += 1
        _last[route] = time.monotonic()
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                if r.headers.get("cf-cache-status", "").upper() == "HIT":
                    stat["cached"] += 1
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code == 429:
                stat["429"] += 1
                time.sleep(int(e.headers.get("Retry-After") or 30) + 1)
                continue
            if e.code == 404:
                return None
            raise
    raise RuntimeError(f"{route}: gave up on {url}")


# --- index --------------------------------------------------------------------------------


def cmd_index(args):
    os.makedirs(WORK, exist_ok=True)
    dump = args.dump or os.path.join(WORK, "series.jsonl.zst")
    if not os.path.exists(dump):
        print(f"downloading {DUMP_URL}", file=sys.stderr)
        subprocess.run(["curl", "-sSfL", "-o", dump, DUMP_URL], check=True)
    path = os.path.join(WORK, "mangabaka.sqlite")
    if os.path.exists(path):
        os.remove(path)
    db = sqlite3.connect(path)
    db.executescript(
        """
        CREATE TABLE series (id INTEGER PRIMARY KEY, title TEXT, type TEXT, people TEXT,
                             cover TEXT, cover_w INTEGER, cover_h INTEGER);
        CREATE TABLE titles (key TEXT, series INTEGER);
        CREATE TABLE links (key TEXT, series INTEGER);
        CREATE TABLE xids (site TEXT, xid TEXT, series INTEGER);
        """
    )
    merged = {}
    zst = subprocess.Popen(["zstd", "-dcf", dump], stdout=subprocess.PIPE)
    n = 0
    for line in zst.stdout:
        d = json.loads(line)
        sid = d["id"]
        if d.get("state") == "merged" and d.get("merged_with"):
            merged[sid] = d["merged_with"]
        elif d.get("state") != "active":
            continue
        names = [d.get("title"), d.get("native_title"), d.get("romanized_title")]
        names += [t.get("title") for t in d.get("titles") or []]
        for group in (d.get("secondary_titles") or {}).values():
            names += [t.get("title") for t in group or []]
        keys = {k for t in names if t for k in title_keys(t)}
        db.executemany("INSERT INTO titles VALUES (?, ?)", [(k, sid) for k in keys])
        for link in d.get("links") or []:
            key = link_key(link)
            if key:
                db.execute("INSERT INTO links VALUES (?, ?)", (key, sid))
        for site, src in (d.get("source") or {}).items():
            if src and src.get("id") is not None:
                db.execute("INSERT INTO xids VALUES (?, ?, ?)", (site, str(src["id"]), sid))
        if d.get("state") == "active":
            raw = ((d.get("cover") or {}).get("raw")) or {}
            people = sorted(person_keys(", ".join((d.get("authors") or []) + (d.get("artists") or []))))
            db.execute(
                "INSERT INTO series VALUES (?, ?, ?, ?, ?, ?, ?)",
                (sid, d.get("title"), d.get("type"), "|".join(people),
                 ((d.get("cover") or {}).get("x250") or {}).get("x1"), raw.get("width"), raw.get("height")),
            )
        n += 1
    if zst.wait() != 0:
        sys.exit("zstd failed")
    # A merged series' titles and IDs point at the series it was merged into.
    db.execute("CREATE TEMP TABLE merged (src INTEGER PRIMARY KEY, dst INTEGER)")
    db.executemany("INSERT INTO merged VALUES (?, ?)", merged.items())
    for table in ("titles", "links", "xids"):
        db.execute(f"UPDATE {table} SET series = m.dst FROM merged m WHERE {table}.series = m.src")
    db.executescript(
        """
        CREATE INDEX titles_key ON titles (key);
        CREATE INDEX links_key ON links (key);
        CREATE INDEX xids_key ON xids (site, xid);
        """
    )
    db.commit()
    print(f"indexed {n} series ({len(merged)} merged) into {path}")


def link_key(url):
    """A site-independent key for a series link, for the sites whose IDs are stable."""
    m = re.search(r"webtoons\.com/.*[?&]title_no=(\d+)", url)
    if m:
        return f"webtoons:{m.group(1)}"
    m = re.search(r"mangadex\.org/title/([0-9a-f-]{36})", url)
    if m:
        return f"mangadex:{m.group(1)}"
    return None


# --- sample -------------------------------------------------------------------------------


def cmd_sample(args):
    os.makedirs(WORK, exist_ok=True)
    rng = random.Random(args.seed)
    # A held-out sample leaves out the titles of the samples drawn before it.
    skip = set()
    for earlier in range(DEFAULT_SEED, args.seed):
        path = os.path.join(WORK, f"sample-{earlier}.json")
        if os.path.exists(path):
            with open(path) as f:
                skip |= {e["link"] for e in json.load(f)}
    sample = []
    for mid, (name, root) in LISTS.items():
        db_path = os.path.join(WORK, f"{mid}.db")
        if not os.path.exists(db_path):
            archive = os.path.join(WORK, f"{mid}.7z")
            subprocess.run(["curl", "-sSfL", "-o", archive, DB_URL.format(mid)], check=True)
            subprocess.run(["7z", "x", "-y", "-bso0", f"-o{WORK}", archive], check=True)
        db = sqlite3.connect(db_path)
        db.text_factory = lambda b: b.decode("utf-8", "replace")
        rows = db.execute(
            "SELECT link, title, alttitles, authors, artists FROM masterlist ORDER BY link"
        ).fetchall()
        rows = [r for r in rows if root + r[0] not in skip]
        for link, title, alt, authors, artists in rng.sample(rows, min(PER_LIST, len(rows))):
            sample.append({"module": name, "link": root + link, "title": title or "",
                           "alttitles": alt or "", "authors": authors or "", "artists": artists or ""})
    with open(os.path.join(WORK, f"sample-{args.seed}.json"), "w") as f:
        json.dump(sample, f, ensure_ascii=False, indent=1)
    print(f"sampled {len(sample)} titles from {len(LISTS)} lists")


# --- matching -----------------------------------------------------------------------------


def decide(entry, candidates):
    """Pick a candidate by title + people, the same rule for every route.

    candidates: [{"id", "title", "keys": set of normalised titles, "people": set, "cover"}].
    Returns (candidate or None, confidence) with confidence one of
    title+author, title-unique (accepted), or author-conflict, ambiguous, none (rejected).
    """
    want = {k for t in entry_titles(entry) for k in title_keys(t)}
    # Lists hold comics; a novel entry's cover is the novel's.
    hits = [c for c in candidates if c["keys"] & want and not c.get("novel")]
    if not hits:
        return None, "none"
    people = person_keys(entry["authors"] + ", " + entry["artists"])
    if people:
        agree = [c for c in hits if people_agree(c["people"], people)]
        if not agree:
            return None, "author-conflict"
        agree = narrow(agree, want)
        return (agree[0], "title+author") if len(agree) == 1 else (None, "ambiguous")
    hits = narrow(hits, want)
    if len(hits) == 1:
        return hits[0], "title-unique"
    return None, "ambiguous"


def narrow(cands, want):
    """Tie-break: prefer the candidates whose main title matches."""
    main = [c for c in cands if set(title_keys(c["title"])) & want]
    return main or cands


def entry_titles(entry):
    """The title and alt titles. Sites separate alt titles differently (MangaDex ", ",
    MangaFire "; ", Asura Scans " • "); split on the strongest one present, since titles
    themselves contain commas."""
    alts = entry["alttitles"]
    sep = next((s for s in ("\n", "•", ";") if s in alts), ",")
    return [entry["title"]] + [a.strip() for a in alts.split(sep) if a.strip()]


def dump_candidates(db, ids):
    out = []
    for sid in sorted(set(ids)):
        row = db.execute("SELECT id, title, people, cover, type FROM series WHERE id = ?", (sid,)).fetchone()
        if not row:
            continue
        keys = {k for (k,) in db.execute("SELECT key FROM titles WHERE series = ?", (sid,))}
        out.append({"id": row[0], "title": row[1], "keys": keys,
                    "people": set(filter(None, (row[2] or "").split("|"))), "cover": row[3],
                    "novel": row[4] == "novel"})
    return out


def match_dump(db, entry):
    key = link_key(entry["link"])
    if key:
        ids = [s for (s,) in db.execute("SELECT DISTINCT series FROM links WHERE key = ?", (key,))]
        if len(ids) == 1:
            return dump_candidates(db, ids)[0], "link"
    if entry["module"] == "MangaDex":
        found = match_mangadex_xids(db, entry)
        if found:
            return found, "cross-id"
    ids = set()
    for t in entry_titles(entry):
        for k in title_keys(t):
            ids.update(s for (s,) in db.execute("SELECT series FROM titles WHERE key = ?", (k,)))
    if len(ids) > 50:  # very generic titles ("Love", "Untitled"): too many to tell apart
        return None, "ambiguous"
    return decide(entry, dump_candidates(db, ids))


# MangaDex's attributes.links keys -> MangaBaka source names.
MD_LINKS = {"al": "anilist", "mu": "manga_updates", "mal": "my_anime_list", "kt": "kitsu", "ap": "anime_planet"}


def match_mangadex_xids(db, entry):
    uuid = entry["link"].rsplit("/", 1)[-1]
    r = http_json("mangadex", f"https://api.mangadex.org/manga/{uuid}", min_interval=0.25)
    links = (((r or {}).get("data") or {}).get("attributes") or {}).get("links") or {}
    ids = set()
    for k, site in MD_LINKS.items():
        if links.get(k):
            ids.update(s for (s,) in db.execute(
                "SELECT series FROM xids WHERE site = ? AND xid = ?", (site, str(links[k]))))
    entry["mangadex_links"] = {k: v for k, v in links.items() if k in MD_LINKS}
    if len(ids) == 1:
        return dump_candidates(db, ids)[0]
    return None


def match_mb_api(entry):
    q = BRACKETS.sub(" ", entry["title"]).strip() or entry["title"]
    url = "https://api.mangabaka.org/v1/series/match?" + urllib.parse.urlencode({"q": q, "limit": 10})
    r = http_json("mangabaka", url, min_interval=0.35)  # 180/min
    cands = []
    for d in (r or {}).get("data") or []:
        names = [d.get("title"), d.get("native_title"), d.get("romanized_title")]
        for group in (d.get("secondary_titles") or {}).values():
            names += [t.get("title") for t in group or []]
        cands.append({"id": d["id"], "title": d.get("title"),
                      "keys": {k for t in names if t for k in title_keys(t)},
                      "people": person_keys(", ".join((d.get("authors") or []) + (d.get("artists") or []))),
                      "cover": ((d.get("cover") or {}).get("x250") or {}).get("x1"),
                      "novel": d.get("type") == "novel"})
    return decide(entry, cands)


ANILIST_QUERY = """
query ($q: String) {
  Page(perPage: 10) {
    media(search: $q, type: MANGA) {
      id title { romaji english native } synonyms format
      coverImage { large }
      staff(perPage: 6) { nodes { name { full native } } }
    }
  }
}"""


def match_anilist(entry):
    q = BRACKETS.sub(" ", entry["title"]).strip() or entry["title"]
    r = http_json("anilist", "https://graphql.anilist.co", {"query": ANILIST_QUERY, "variables": {"q": q}},
                  min_interval=2.1)  # 30/min while degraded
    cands = []
    for m in ((r or {}).get("data") or {}).get("Page", {}).get("media") or []:
        names = list((m.get("title") or {}).values()) + (m.get("synonyms") or [])
        staff = [n["name"].get("full") or "" for n in (m.get("staff") or {}).get("nodes") or []]
        cands.append({"id": m["id"], "title": (m.get("title") or {}).get("romaji"),
                      "keys": {k for t in names if t for k in title_keys(t)},
                      "people": person_keys(", ".join(staff)),
                      "cover": (m.get("coverImage") or {}).get("large"),
                      "novel": m.get("format") == "NOVEL"})
    return decide(entry, cands)


ROUTES = {"dump": None, "mb_api": match_mb_api, "anilist": match_anilist}
ACCEPTED = {"link", "cross-id", "title+author", "title-unique"}


def cmd_match(args):
    with open(os.path.join(WORK, f"sample-{args.seed}.json")) as f:
        sample = json.load(f)
    db = sqlite3.connect(os.path.join(WORK, "mangabaka.sqlite"))
    routes = args.routes.split(",")
    rows = []
    for i, e in enumerate(sample):
        row = {"module": e["module"], "link": e["link"], "title": e["title"], "authors": e["authors"]}
        for route in routes:
            cand, conf = match_dump(db, e) if route == "dump" else ROUTES[route](e)
            row[f"{route}_conf"] = conf
            row[f"{route}_id"] = cand["id"] if cand else ""
            row[f"{route}_title"] = cand["title"] if cand else ""
            row[f"{route}_cover"] = (cand or {}).get("cover") or ""
        if e.get("mangadex_links") is not None:
            row["mangadex_links"] = json.dumps(e["mangadex_links"])
        rows.append(row)
        print(f"{i + 1}/{len(sample)} {e['module']}: {e['title'][:50]!r} "
              + " ".join(f"{r}={row[f'{r}_conf']}" for r in routes), file=sys.stderr)
    write_csv(os.path.join(HERE, f"results-{args.seed}.csv"), rows)
    summary = {"requests": REQUESTS, "routes": {}}
    for route in routes:
        per = {}
        for mod in [n for n, _ in LISTS.values()] + ["all"]:
            sel = [r for r in rows if mod in ("all", r["module"])]
            confs = {}
            for r in sel:
                confs[r[f"{route}_conf"]] = confs.get(r[f"{route}_conf"], 0) + 1
            accepted = sum(v for k, v in confs.items() if k in ACCEPTED)
            per[mod] = {"n": len(sel), "accepted": accepted, "by_confidence": confs}
        summary["routes"][route] = per
    with open(os.path.join(HERE, f"summary-{args.seed}.json"), "w") as f:
        json.dump(summary, f, indent=1)
    print(json.dumps(summary, indent=1))


def cmd_review(args):
    """Accepted matches side by side, for checking wrong matches by hand."""
    import csv

    with open(os.path.join(HERE, f"results-{args.seed}.csv")) as f:
        rows = list(csv.DictReader(f))
    for i, r in enumerate(rows):
        picks = {route: (r[f"{route}_conf"], r[f"{route}_title"], r[f"{route}_id"])
                 for route in ("dump", "mb_api", "anilist") if f"{route}_conf" in r}
        accepted = {k: v for k, v in picks.items() if v[0] in ACCEPTED}
        if not accepted:
            continue
        print(f"{i:3} {r['module']:10} {r['title'][:60]!r} [{r['authors'][:40]}]")
        for route, (conf, title, sid) in accepted.items():
            print(f"      {route:7} {conf:13} {sid:>7} {title[:70]!r}")


def write_csv(path, rows):
    import csv

    fields = []
    for r in rows:
        fields += [k for k in r if k not in fields]
    with open(path, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=fields)
        w.writeheader()
        w.writerows(rows)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("index")
    s.add_argument("--dump", help="an already downloaded series.jsonl.zst")
    s = sub.add_parser("sample")
    s.add_argument("--seed", type=int, default=DEFAULT_SEED)
    s = sub.add_parser("match")
    s.add_argument("--seed", type=int, default=DEFAULT_SEED)
    s.add_argument("--routes", default="dump,mb_api,anilist")
    s = sub.add_parser("review")
    s.add_argument("--seed", type=int, default=DEFAULT_SEED)
    args = p.parse_args()
    {"index": cmd_index, "sample": cmd_sample, "match": cmd_match, "review": cmd_review}[args.cmd](args)


if __name__ == "__main__":
    main()
