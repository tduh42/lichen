//! Links and places in a message's text, for the apps to make tappable: `http` and `https`
//! addresses, and coordinates (a `geo:` URI, decimal degrees, degrees and minutes with a
//! hemisphere). Nothing is fetched: the apps open a link only when the user taps it, show no
//! preview, and show a place on their own map.
//!
//! The text comes from anyone: the scanner never panics and stays linear in its length.

/// What a span of text is.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// An address to open as it is written; `www.…` opens as `https://www.…`.
    Link(String),
    /// A place, WGS 84 degrees (north and east positive).
    Place { lat: f64, lon: f64 },
}

/// A span of a text, by byte offsets (`start..end`).
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub found: Found,
}

/// The links and places in `text`, in order, never overlapping.
pub fn find(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut offsets: Vec<usize> = text.char_indices().map(|(o, _)| o).collect();
    offsets.push(text.len());
    let at = |i: usize| offsets.get(i).copied().unwrap_or(text.len());
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let prev = i.checked_sub(1).and_then(|p| chars.get(p)).copied();
        let found = if prev.is_some_and(char::is_alphanumeric) {
            None
        } else {
            link(&chars, i)
                .or_else(|| geo(&chars, i))
                .or_else(|| prev.is_none_or(|p| !joins(p)).then(|| pair(&chars, i)).flatten())
        };
        match found {
            Some((end, found)) if end > i => {
                out.push(Span {
                    start: at(i),
                    end: at(end),
                    found,
                });
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}

/// Characters that may go on a number and make it part of something else ("v1.2345",
/// "1,45.1234", "-", "°").
fn joins(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(
            c,
            '.' | ',' | '-' | '+' | '_' | '/' | ':' | '°' | '\'' | '"' | '\u{2212}'
        )
}

/// What may be part of an address: anything but spaces, controls, the characters that
/// reorder text (SR-35) and the usual delimiters around a link.
fn in_url(c: char) -> bool {
    !(c.is_whitespace()
        || c.is_control()
        || matches!(
            c,
            '<' | '>'
                | '"'
                | '`'
                | '\u{061C}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        ))
}

/// Whether `chars[i..]` starts with `prefix`, ASCII case aside.
fn starts(chars: &[char], i: usize, prefix: &str) -> bool {
    let mut n = 0;
    for p in prefix.chars() {
        match chars.get(i + n) {
            Some(c) if c.eq_ignore_ascii_case(&p) => n += 1,
            _ => return false,
        }
    }
    true
}

/// Where an address starting at `i` ends: up to the first delimiter, without the
/// punctuation that ends a sentence and the closing brackets it does not open.
fn url_end(chars: &[char], i: usize, from: usize) -> usize {
    const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
    let mut j = from;
    while chars.get(j).copied().is_some_and(in_url) {
        j += 1;
    }
    // Open and close counts per kind of bracket, kept up to date while trimming.
    let mut counts = [(0usize, 0usize); 3];
    for c in chars.get(i..j).unwrap_or(&[]) {
        for (n, (open, close)) in counts.iter_mut().zip(PAIRS) {
            n.0 += usize::from(*c == open);
            n.1 += usize::from(*c == close);
        }
    }
    while j > from {
        let Some(&last) = chars.get(j - 1) else {
            break;
        };
        let unbalanced = counts
            .iter()
            .zip(PAIRS)
            .any(|(n, (_, close))| last == close && n.1 > n.0);
        if !(unbalanced
            || matches!(
                last,
                '.' | ',' | ':' | ';' | '!' | '?' | '\'' | '*' | '…' | '’' | '”' | '»'
            ))
        {
            break;
        }
        for (n, (open, close)) in counts.iter_mut().zip(PAIRS) {
            n.0 -= usize::from(last == open);
            n.1 -= usize::from(last == close);
        }
        j -= 1;
    }
    j
}

/// An `http`, `https` or `www.` address at `i`.
fn link(chars: &[char], i: usize) -> Option<(usize, Found)> {
    let (scheme, www) = if starts(chars, i, "https://") {
        (8, false)
    } else if starts(chars, i, "http://") {
        (7, false)
    } else if starts(chars, i, "www.") {
        (0, true)
    } else {
        return None;
    };
    let end = url_end(chars, i, i + scheme);
    let rest = chars.get(i + scheme..end)?;
    let host_len = rest
        .iter()
        .position(|c| matches!(c, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    let host = rest.get(..host_len)?;
    if !host.iter().any(|c| c.is_alphanumeric()) {
        return None;
    }
    if www {
        // "www.example.org", not "www." or "www.example".
        let after = host.get(4..)?;
        let dot = after.iter().rposition(|&c| c == '.')?;
        if dot == 0 || after.len() - dot < 3 {
            return None;
        }
    }
    // The scheme in lower case; `www.` opens over HTTPS.
    let scheme = if scheme == 7 { "http://" } else { "https://" };
    let url = format!("{scheme}{}", rest.iter().collect::<String>());
    Some((end, Found::Link(url)))
}

/// A decimal number at `j`: its value, where it ends, and its count of decimals.
fn number(chars: &[char], mut j: usize, max_int: usize) -> Option<(f64, usize, usize)> {
    let mut s = String::new();
    if let Some(&c) = chars.get(j)
        && matches!(c, '-' | '+' | '\u{2212}')
    {
        if c != '+' {
            s.push('-');
        }
        j += 1;
    }
    let int_start = j;
    while chars.get(j).is_some_and(char::is_ascii_digit) {
        s.extend(chars.get(j));
        j += 1;
    }
    if j == int_start || j - int_start > max_int {
        return None;
    }
    let mut decimals = 0;
    if chars.get(j) == Some(&'.') && chars.get(j + 1).is_some_and(char::is_ascii_digit) {
        s.push('.');
        j += 1;
        while chars.get(j).is_some_and(char::is_ascii_digit) {
            s.extend(chars.get(j));
            j += 1;
            decimals += 1;
        }
    }
    Some((s.parse().ok()?, j, decimals))
}

/// A `geo:` URI at `i` (RFC 5870): `geo:lat,lon[,alt][;params]`, and the `geo:0,0?q=lat,lon`
/// that maps apps share.
fn geo(chars: &[char], i: usize) -> Option<(usize, Found)> {
    if !starts(chars, i, "geo:") {
        return None;
    }
    let (mut lat, j, _) = number(chars, i + 4, 3)?;
    if chars.get(j) != Some(&',') {
        return None;
    }
    let (mut lon, j, _) = number(chars, j + 1, 3)?;
    let end = url_end(chars, i, j);
    if lat == 0.0 && lon == 0.0 {
        let rest = chars.get(j..end).unwrap_or(&[]);
        if let Some(q) = rest.windows(2).position(|w| w == ['q', '=']) {
            let q = j + q + 2;
            if let Some((a, k, _)) = number(chars, q, 3)
                && chars.get(k) == Some(&',')
                && let Some((b, _, _)) = number(chars, k + 1, 3)
            {
                (lat, lon) = (a, b);
            }
        }
    }
    valid(lat, lon).then_some((end, Found::Place { lat, lon }))
}

fn valid(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0
}

/// One coordinate: degrees, or degrees and minutes (and seconds), with a sign or a
/// hemisphere letter.
#[derive(Clone, Copy, Debug)]
struct Coord {
    value: f64,
    /// N, S, E or W (O, French "ouest", is W).
    hemisphere: Option<char>,
    /// Written with a degree sign or a hemisphere: it says what it is.
    marked: bool,
    decimals: usize,
    end: usize,
}

fn hemisphere(c: char) -> Option<char> {
    match c {
        'N' | 'S' | 'E' | 'W' => Some(c),
        'O' => Some('W'),
        _ => None,
    }
}

/// Skips at most one space.
fn space(chars: &[char], j: usize) -> usize {
    if chars.get(j).is_some_and(|c| *c == ' ' || *c == '\u{a0}') {
        j + 1
    } else {
        j
    }
}

fn coord(chars: &[char], i: usize) -> Option<Coord> {
    let mut j = i;
    // "N 45.1234", "N45°07.404'".
    let mut hemi = chars.get(j).copied().and_then(hemisphere);
    if hemi.is_some() {
        j = space(chars, j + 1);
        if !chars.get(j).is_some_and(char::is_ascii_digit) {
            return None;
        }
    }
    let (deg, mut j, decimals) = number(chars, j, 3)?;
    let signed = matches!(chars.get(i), Some('-' | '+' | '\u{2212}'));
    let mut value = deg.abs();
    let mut marked = hemi.is_some();
    let mut decimals = decimals;
    let k = space(chars, j);
    if matches!(chars.get(k), Some('°' | 'º')) {
        marked = true;
        j = k + 1;
        // Minutes, then seconds, after whole degrees.
        let m = space(chars, j);
        if decimals == 0
            && let Some((min, after, d)) = number(chars, m, 2)
            && min >= 0.0
            && min < 60.0
            && matches!(chars.get(after), Some('\'' | '′' | '’'))
        {
            value += min / 60.0;
            decimals = d;
            j = after + 1;
            let s = space(chars, j);
            if let Some((sec, after, d)) = number(chars, s, 2)
                && sec >= 0.0
                && sec < 60.0
            {
                let close = match (chars.get(after), chars.get(after + 1)) {
                    (Some('"' | '″' | '”'), _) => Some(after + 1),
                    (Some('\'' | '′' | '’'), Some('\'' | '′' | '’')) => Some(after + 2),
                    _ => None,
                };
                if let Some(close) = close {
                    value += sec / 3600.0;
                    decimals = d;
                    j = close;
                }
            }
        }
    }
    // "45.1234 N", "45°07'24.2\"N".
    if hemi.is_none() && !signed {
        let k = space(chars, j);
        if let Some(h) = chars.get(k).copied().and_then(hemisphere)
            && !chars.get(k + 1).copied().is_some_and(char::is_alphanumeric)
        {
            hemi = Some(h);
            marked = true;
            j = k + 1;
        }
    }
    if signed && hemi.is_some() {
        return None;
    }
    if deg < 0.0 || matches!(hemi, Some('S' | 'W')) {
        value = -value;
    }
    Some(Coord {
        value,
        hemisphere: hemi,
        marked,
        decimals,
        end: j,
    })
}

/// A latitude and a longitude at `i`: "45.1234, 5.6789", "45.1234 5.6789", "45.1234° N,
/// 5.6789° E", "N 45° 07.404' E 005° 40.733'", "45°07'24.2\"N 5°40'44.0\"E". Without a degree
/// sign or a hemisphere, both need three decimals at least (about 100 m), so that ordinary
/// numbers are left alone.
fn pair(chars: &[char], i: usize) -> Option<(usize, Found)> {
    let a = coord(chars, i)?;
    let mut j = a.end;
    while chars.get(j) == Some(&' ') {
        j += 1;
    }
    let comma = matches!(chars.get(j), Some(',' | ';'));
    if comma {
        j += 1;
        while chars.get(j) == Some(&' ') {
            j += 1;
        }
    }
    if j == a.end {
        return None;
    }
    let b = coord(chars, j)?;
    if chars
        .get(b.end)
        .is_some_and(|&c| c.is_alphanumeric() || matches!(c, '.' | '°' | '\'' | '"'))
        || (chars.get(b.end) == Some(&',')
            && chars.get(b.end + 1).is_some_and(char::is_ascii_digit))
    {
        return None;
    }
    for c in [a, b] {
        if !c.marked && c.decimals < 3 {
            return None;
        }
    }
    let lat_first = |h: Option<char>| matches!(h, None | Some('N' | 'S'));
    let lon_second = |h: Option<char>| matches!(h, None | Some('E' | 'W'));
    let (lat, lon) = if lat_first(a.hemisphere) && lon_second(b.hemisphere) {
        (a.value, b.value)
    } else if matches!(a.hemisphere, Some('E' | 'W')) && matches!(b.hemisphere, Some('N' | 'S'))
    {
        (b.value, a.value)
    } else {
        return None;
    };
    valid(lat, lon).then_some((b.end, Found::Place { lat, lon }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(text: &str) -> Vec<(&str, String)> {
        find(text)
            .into_iter()
            .filter_map(|s| match s.found {
                Found::Link(u) => Some((&text[s.start..s.end], u)),
                Found::Place { .. } => None,
            })
            .collect()
    }

    fn places(text: &str) -> Vec<(&str, f64, f64)> {
        find(text)
            .into_iter()
            .filter_map(|s| match s.found {
                Found::Place { lat, lon } => Some((&text[s.start..s.end], lat, lon)),
                Found::Link(_) => None,
            })
            .collect()
    }

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn links_are_found_as_written() {
        assert_eq!(
            links("see https://example.org/a?b=1#c now"),
            [(
                "https://example.org/a?b=1#c",
                "https://example.org/a?b=1#c".into()
            )]
        );
        assert_eq!(
            links("HTTP://Example.org"),
            [("HTTP://Example.org", "http://Example.org".into())]
        );
        assert_eq!(
            links("www.example.org/x"),
            [("www.example.org/x", "https://www.example.org/x".into())]
        );
        assert_eq!(
            links("two: http://a.fr and https://b.fr/"),
            [
                ("http://a.fr", "http://a.fr".into()),
                ("https://b.fr/", "https://b.fr/".into())
            ]
        );
        assert_eq!(
            links("https://fr.wikipedia.org/wiki/Écrins"),
            [(
                "https://fr.wikipedia.org/wiki/Écrins",
                "https://fr.wikipedia.org/wiki/Écrins".into()
            )]
        );
    }

    #[test]
    fn punctuation_around_a_link_stays_out() {
        assert_eq!(links("(https://a.fr/x)")[0].0, "https://a.fr/x");
        assert_eq!(
            links("https://en.wikipedia.org/wiki/Bar_(place)")[0].0,
            "https://en.wikipedia.org/wiki/Bar_(place)"
        );
        assert_eq!(links("Look: https://a.fr/x.")[0].0, "https://a.fr/x");
        assert_eq!(links("https://a.fr/x?!")[0].0, "https://a.fr/x");
        assert_eq!(links("<https://a.fr/x>")[0].0, "https://a.fr/x");
        assert_eq!(links("\"https://a.fr/x\"")[0].0, "https://a.fr/x");
        assert_eq!(links("« https://a.fr/x »")[0].0, "https://a.fr/x");
    }

    #[test]
    fn what_is_not_a_link() {
        for text in [
            "https://",
            "https:// example.org",
            "http://...",
            "www.",
            "www.example",
            "ftp://example.org",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "xhttps://example.org",
            "mailto:a@b.fr",
            "example.org",
        ] {
            assert!(links(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn a_link_stops_at_characters_that_reorder_text() {
        // SR-35: U+202E would show "https://a.fr/gpj.exe" reversed.
        assert_eq!(
            links("https://a.fr/x\u{202e}exe.jpg")[0].0,
            "https://a.fr/x"
        );
        assert_eq!(links("https://a.fr/x\u{200b}y")[0].0, "https://a.fr/x");
    }

    #[test]
    fn decimal_coordinates() {
        let p = places("We are at 45.1234, 5.6789 near the hut");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].0, "45.1234, 5.6789");
        assert!(near(p[0].1, 45.1234) && near(p[0].2, 5.6789));
        let p = places("-33.8568,151.2153");
        assert!(near(p[0].1, -33.8568) && near(p[0].2, 151.2153));
        let p = places("45.12345 5.67890");
        assert!(near(p[0].1, 45.12345) && near(p[0].2, 5.6789));
        let p = places("(45.1234; -5.6789).");
        assert_eq!(p[0].0, "45.1234; -5.6789");
        assert!(near(p[0].2, -5.6789));
    }

    #[test]
    fn coordinates_with_hemispheres() {
        let p = places("45.1234° N, 5.6789° E");
        assert_eq!(p[0].0, "45.1234° N, 5.6789° E");
        assert!(near(p[0].1, 45.1234) && near(p[0].2, 5.6789));
        let p = places("33.8568S 151.2153E");
        assert!(near(p[0].1, -33.8568) && near(p[0].2, 151.2153));
        let p = places("12.5 N 1.5 O");
        assert!(near(p[0].1, 12.5) && near(p[0].2, -1.5));
        // Longitude first, said so.
        let p = places("5.6789 E, 45.1234 N");
        assert!(near(p[0].1, 45.1234) && near(p[0].2, 5.6789));
    }

    #[test]
    fn degrees_minutes_seconds() {
        let p = places("45°07'24.2\"N 5°40'44.0\"E");
        assert_eq!(p[0].0, "45°07'24.2\"N 5°40'44.0\"E");
        assert!(near(p[0].1, 45.0 + 7.0 / 60.0 + 24.2 / 3600.0));
        assert!(near(p[0].2, 5.0 + 40.0 / 60.0 + 44.0 / 3600.0));
        let p = places("N 45° 07.404' E 005° 40.733'");
        assert!(near(p[0].1, 45.0 + 7.404 / 60.0));
        assert!(near(p[0].2, 5.0 + 40.733 / 60.0));
        let p = places("33°51′24″S 151°12′55″E");
        assert!(p[0].1 < -33.85 && p[0].2 > 151.2);
    }

    #[test]
    fn geo_uris() {
        let p = places("geo:45.1234,5.6789");
        assert_eq!(p[0].0, "geo:45.1234,5.6789");
        assert!(near(p[0].1, 45.1234) && near(p[0].2, 5.6789));
        let p = places("at geo:45.1,5.6,1200;u=35.");
        assert_eq!(p[0].0, "geo:45.1,5.6,1200;u=35");
        let p = places("geo:0,0?q=45.1234,5.6789(Hut)");
        assert!(near(p[0].1, 45.1234) && near(p[0].2, 5.6789));
        assert!(places("geo:91,5").is_empty());
        assert!(places("geo:45").is_empty());
    }

    #[test]
    fn ordinary_numbers_are_not_places() {
        for text in [
            "1.5, 2.5",
            "12.50, 3.25 euros",
            "v1.2345 2.3456",
            "95.1234, 5.6789",
            "45.1234, 195.6789",
            "45.1234,5.6789,1.2345",
            "1,45.1234, 5.6789",
            "45.1234N 5.6789N",
            "-45.1234 S, 5.6789 E",
            "45.1234, 5.6789x",
            "45.1234 Nice, 5.6789",
            "12:45.1234, 5.6789",
        ] {
            assert!(places(text).is_empty(), "{text}: {:?}", places(text));
        }
    }

    #[test]
    fn links_and_places_together() {
        let all = find("Hut: 45.1234, 5.6789, see https://a.fr/hut");
        assert_eq!(all.len(), 2);
        assert!(matches!(all[0].found, Found::Place { .. }));
        assert!(matches!(all[1].found, Found::Link(_)));
        // A link's coordinates belong to the link.
        let all = find("https://maps.example/?q=45.1234,5.6789");
        assert_eq!(all.len(), 1);
        assert!(matches!(all[0].found, Found::Link(_)));
    }

    #[test]
    fn nothing_breaks_on_odd_text() {
        for text in [
            "",
            "°",
            "N",
            "N 4",
            "45.",
            "45°",
            "45°07'",
            "45°07'24\"",
            "geo:",
            "geo:1,",
            "https://(((",
            "http://a.fr)))))",
            "--45.1234, 5.6789",
            "45.1234,\u{a0}5.6789",
            "\u{202e}https://a.fr",
        ] {
            let _ = find(text);
        }
        let long = "9".repeat(10_000) + ".1234, 5.6789";
        assert!(places(&long).is_empty());
    }
}
