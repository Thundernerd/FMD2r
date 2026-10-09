# T43: Test vectors from a real FMD2 install
Deps: none

## Goal
`EncryptString`/`DecryptString` (T11) and the importer's account and proxy-password decryption (T32) are tested against an OpenSSL reproduction of DCPcrypt's recipe, not against ciphertext FMD2 itself wrote (PR #37, "Gap"). Get real vectors.

## Scope (in/out)
In:
- Run FMD2 (the Windows release, under Wine if needed) with a throwaway data dir; set an account username/password on a module, a proxy username/password, and a few settings; collect `userdata/modules.json`, `userdata/settings.json` and `userdata/accounts.db` if present.
- Commit the resulting files (no real credentials) as fixtures, and add tests: `decrypt_string` recovers the plaintexts, `encrypt_string` reproduces FMD2's ciphertext for the same input, and `fmd2r import` imports the accounts and proxy credentials correctly.
- Record the FMD2 version and how the fixture was made in `fixtures/README.md`.

Out: changing the crypto unless the vectors show a mismatch (then fix it here).

## Seams under test
`fmd_lua::crypto::{encrypt_string, decrypt_string}`; `fmd-import`'s public `import()` over the fixture userdata dir.

## Acceptance criteria
- [ ] Fixtures come from a real FMD2 binary.
- [ ] Round trip and importer tests pass.

## FMD2 references
- `baseunits/uBaseUnit.pas:1556-1587` (`EncryptString`/`DecryptString`)
- `baseunits/WebsiteModules.pas:545-690` (`modules.json` reader/writer)
