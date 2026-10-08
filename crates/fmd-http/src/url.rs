//! URL normalisation and splitting, ported from `httpsendthread.pas`.

/// Trims leading `:`, `/` and blanks, URL-encodes unless already encoded, and adds
/// `https://` when there is no scheme (baseunits/httpsendthread.pas:613-616).
pub(crate) fn normalize(url: &str) -> String {
    let url = url
        .trim_start_matches(|c: char| c == ':' || c == '/' || c <= ' ')
        .trim_end_matches(|c: char| c <= ' ');
    let url = maybe_encode_url(url);
    if url.is_empty() || url.contains("://") {
        url
    } else {
        format!("https://{url}")
    }
}

/// `MaybeEncodeURL` (baseunits/httpsendthread.pas:394-400): `EncodeURL` unless
/// `DecodeURL` would shorten the string, i.e. unless it already holds an escape.
fn maybe_encode_url(url: &str) -> String {
    let url = url.trim();
    if decode_url_shrinks(url.as_bytes()) {
        return url.to_string();
    }
    let mut out = String::with_capacity(url.len());
    for &b in url.as_bytes() {
        // `URLSpecialChar` (baseunits/synapse/synacode.pas:93-94).
        if b <= 0x20 || b >= 0x7f || b"<>\"%{}|\\^[]`".contains(&b) {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

/// Whether Synapse's `DecodeTriplet(s, '%')` (baseunits/synapse/synacode.pas:403-483)
/// returns a shorter string: some `%` is followed by two hex digits or a line break, or
/// has fewer than two characters after it.
fn decode_url_shrinks(s: &[u8]) -> bool {
    s.iter().enumerate().any(|(i, &b)| {
        b == b'%'
            && match (s.get(i + 1), s.get(i + 2)) {
                (Some(b'\r' | b'\n'), _) => true,
                (Some(h), Some(l)) => h.is_ascii_hexdigit() && l.is_ascii_hexdigit(),
                _ => true,
            }
    })
}

/// `poschar` (baseunits/httpsendthread.pas:179-189): 1-based position of `c` in `s` from
/// `offset`, or 0; stops at the first byte in `escape`.
fn poschar(c: u8, s: &[u8], offset: usize, escape: &[u8]) -> usize {
    for i in offset.max(1)..=s.len() {
        let b = s[i - 1];
        if escape.contains(&b) {
            break;
        }
        if b == c {
            return i;
        }
    }
    0
}

/// `SplitURL` (baseunits/httpsendthread.pas:191-276) with protocol and port included:
/// splits a URL into `(host, path)` where host is `proto://host[:port]` (`https://` when
/// the URL has no scheme) and path starts with `/`. Either may be empty. A host is only
/// recognised when it contains a dot or comes with a scheme or port.
pub(crate) fn split_url(url: &str) -> (String, String) {
    fn cleanuri(u: &mut Vec<u8>) {
        while u.first().is_some_and(|b| matches!(b, b'.' | b':' | b'/')) {
            u.remove(0);
        }
    }
    let mut iurl = url.trim().as_bytes().to_vec();
    if iurl.is_empty() {
        return (String::new(), String::new());
    }
    let mut ihost = Vec::new();
    let mut iproto = Vec::new();
    let mut iport = Vec::new();
    if iurl[0] == b'/' {
        if iurl.len() == 1 {
            return (String::new(), String::new());
        } else if iurl[1] != b'/' {
            return (String::new(), String::from_utf8_lossy(&iurl).into_owned());
        }
    }
    let mut p = poschar(b':', &iurl, 1, b"/");
    if p != 0 && p + 2 < iurl.len() && &iurl[p..p + 2] == b"//" {
        iproto = iurl[..p - 1].to_vec();
        iurl.drain(..p + 2);
        p = poschar(b':', &iurl, 1, b"/");
    }
    if p != 0 && p < iurl.len() && iurl[p].is_ascii_digit() {
        // Pascal's `for q := p+1 to Length` leaves q at the first non-digit, or at
        // Length when the loop runs out; `if q = Length then Inc(q)`.
        let mut q = p + 1;
        while q < iurl.len() && iurl[q - 1].is_ascii_digit() {
            q += 1;
        }
        if q == iurl.len() {
            q += 1;
        }
        iport = iurl[p..q - 1].to_vec();
        iurl.drain(p - 1..q - 1);
    }
    cleanuri(&mut iurl);
    let p = poschar(b'.', &iurl, 1, b"/");
    let q = poschar(b'/', &iurl, 1, b"");
    if p != 0 && p < iurl.len() {
        if p < q {
            ihost = iurl[..q - 1].to_vec();
            iurl.drain(..q - 1);
            cleanuri(&mut iurl);
        } else if q == 0 {
            let q = poschar(b'.', &iurl, p + 1, b"");
            if q != 0 && q < iurl.len() {
                ihost = std::mem::take(&mut iurl);
            }
        }
    }
    if ihost.is_empty() && !iurl.is_empty() && (!iproto.is_empty() || !iport.is_empty()) {
        ihost = std::mem::take(&mut iurl);
    }
    let mut host = String::from_utf8_lossy(&ihost).into_owned();
    if !host.is_empty() {
        host = if iproto.is_empty() {
            format!("https://{host}")
        } else {
            format!("{}://{host}", String::from_utf8_lossy(&iproto))
        };
        if !iport.is_empty() {
            host = format!("{host}:{}", String::from_utf8_lossy(&iport));
        }
    }
    let path = if iurl.is_empty() {
        String::new()
    } else {
        format!("/{}", String::from_utf8_lossy(&iurl))
    };
    (host, path)
}
