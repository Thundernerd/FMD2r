Two real FMD2-DB dumps (https://github.com/dazedcat19/FMD2-DB, `7z/<module id>.7z`, GPL-2.0, as
downloaded on 2026-10-08), each a 7z archive holding `<module id>.db` in FMD2's per-site schema
(baseunits/DBDataProcess.pas:143-153):

- `598672e8158d4fd781bea8d426534695.7z`: Gourmet Scans, 3 titles.
- `201234a2c811487c8542fb7ec2c92b20.7z`: 8 titles; some summaries hold Windows-1252 bytes that are
  not valid UTF-8.
