//! URL host extraction and Set-Cookie parsing / jar upsert. Pure.

use crate::CookieEntry;
/// Host of a URL (no scheme/path/userinfo/port).
pub fn host_of(u: &str) -> String {
    let s = u.split("://").nth(1).unwrap_or(u);
    let s = s.split('/').next().unwrap_or(s);
    let s = s.rsplit('@').next().unwrap_or(s);
    s.split(':').next().unwrap_or(s).to_string()
}

pub fn parse_set_cookie(header: &str, host: &str) -> Option<CookieEntry> {
    let mut parts = header.split(';');
    let (name, value) = parts.next()?.trim().split_once('=')?;
    let mut domain = host.to_string();
    let mut path = "/".to_string();
    for attr in parts {
        if let Some((k, v)) = attr.trim().split_once('=') {
            match k.trim().to_ascii_lowercase().as_str() {
                "domain" => domain = v.trim().trim_start_matches('.').to_string(),
                "path" => path = v.trim().to_string(),
                _ => {}
            }
        }
    }
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some(CookieEntry {
        domain,
        path,
        name: name.to_string(),
        value: value.trim().to_string(),
    })
}

pub fn upsert_cookie(jar: &mut Vec<CookieEntry>, c: CookieEntry) {
    if let Some(e) = jar
        .iter_mut()
        .find(|e| e.domain == c.domain && e.path == c.path && e.name == c.name)
    {
        e.value = c.value;
    } else {
        jar.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_of_strips_scheme_userinfo_port_path() {
        assert_eq!(host_of("https://example.com/a/b"), "example.com");
        assert_eq!(host_of("http://user:pass@host.com:8080/x"), "host.com");
        assert_eq!(host_of("example.com"), "example.com");
        assert_eq!(host_of("https://a.b.c"), "a.b.c");
    }

    #[test]
    fn parse_set_cookie_attrs() {
        let c = parse_set_cookie("sid=abc; Path=/api; Domain=.example.com", "fallback").unwrap();
        assert_eq!(c.name, "sid");
        assert_eq!(c.value, "abc");
        assert_eq!(c.path, "/api");
        assert_eq!(c.domain, "example.com");
    }

    #[test]
    fn parse_set_cookie_defaults_and_rejects() {
        let c = parse_set_cookie("token=xyz", "host.com").unwrap();
        assert_eq!(c.domain, "host.com");
        assert_eq!(c.path, "/");
        assert!(parse_set_cookie("=novalue", "h").is_none());
        assert!(parse_set_cookie("garbage", "h").is_none());
    }

    #[test]
    fn upsert_updates_in_place_or_appends() {
        let mut jar = Vec::new();
        upsert_cookie(
            &mut jar,
            parse_set_cookie("a=1; Domain=x.com", "x.com").unwrap(),
        );
        assert_eq!(jar.len(), 1);
        // Same (domain, path, name) updates the value in place.
        upsert_cookie(
            &mut jar,
            parse_set_cookie("a=2; Domain=x.com", "x.com").unwrap(),
        );
        assert_eq!(jar.len(), 1);
        assert_eq!(jar[0].value, "2");
        // A different name appends a new entry.
        upsert_cookie(
            &mut jar,
            parse_set_cookie("b=9; Domain=x.com", "x.com").unwrap(),
        );
        assert_eq!(jar.len(), 2);
        // Same name + domain but a different path is a distinct cookie (path is
        // part of the jar key), so it appends rather than overwriting.
        upsert_cookie(
            &mut jar,
            parse_set_cookie("a=3; Domain=x.com; Path=/sub", "x.com").unwrap(),
        );
        assert_eq!(jar.len(), 3);
    }
}
