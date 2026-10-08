//! The smoke list (`fixtures/smoke/list.toml`) replayed offline: every entry's `module info` and
//! `module pages` runs on its recorded HTTP traffic and prints its committed snapshot.
// Integration tests may panic (CODING_STANDARDS.md); clippy only exempts `#[test]` fns, not helpers.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use fmd_smoke::Smoke;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn smoke() -> Smoke {
    Smoke {
        fmd2r: assert_cmd::cargo::cargo_bin("fmd2r"),
        lua_dir: fixtures().join("lua"),
        dir: fixtures().join("smoke"),
    }
}

#[test]
fn every_smoke_entry_replays_to_its_snapshots() {
    let smoke = smoke();
    let list = smoke.list().unwrap();
    // In parallel: some modules sleep between requests (FanFox waits 2s per page request,
    // FanFox.lua:131), and a replay keeps those waits.
    let results: Vec<_> = std::thread::scope(|scope| {
        let runs: Vec<_> = list
            .entries
            .iter()
            .map(|entry| scope.spawn(|| smoke.replay(entry)))
            .collect();
        runs.into_iter().map(|run| run.join().unwrap()).collect()
    });
    let failures: Vec<String> = results
        .iter()
        .flat_map(|result| {
            result
                .failures("replay")
                .map(|failure| format!("{}: {failure}", result.name))
        })
        .collect();
    println!("{} smoke entries replayed", list.entries.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
