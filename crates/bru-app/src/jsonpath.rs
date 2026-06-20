//! A tiny JSONPath-ish resolver ($.a.b[0].c[*]) over `serde_json::Value`.
//! Ported from the iced client. Pure.
/// A step in a JSONPath-ish query.
pub enum PathStep {
    Key(String),
    Index(usize),
    Wild,
}

/// Tokenize a JSONPath-ish query (`$.a.b[0].c[*]`) into steps. Ported from iced.
pub fn json_path_tokens(q: &str) -> Vec<PathStep> {
    fn flush(buf: &mut String, steps: &mut Vec<PathStep>) {
        let s = buf.trim();
        if !s.is_empty() {
            steps.push(if s == "*" {
                PathStep::Wild
            } else {
                PathStep::Key(s.to_string())
            });
        }
        buf.clear();
    }
    let mut steps = Vec::new();
    let mut buf = String::new();
    let mut chars = q.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => flush(&mut buf, &mut steps),
            '[' => {
                flush(&mut buf, &mut steps);
                let mut inner = String::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    inner.push(d);
                }
                let inner = inner.trim().trim_matches(|c| c == '"' || c == '\'');
                if inner == "*" {
                    steps.push(PathStep::Wild);
                } else if let Ok(i) = inner.parse::<usize>() {
                    steps.push(PathStep::Index(i));
                } else if !inner.is_empty() {
                    steps.push(PathStep::Key(inner.to_string()));
                }
            }
            _ => buf.push(c),
        }
    }
    flush(&mut buf, &mut steps);
    steps
}

/// Resolve a JSONPath-ish query against a value (supports `.key`, `[i]`, `[*]`).
pub fn json_path(v: &serde_json::Value, query: &str) -> Option<serde_json::Value> {
    use serde_json::Value as J;
    let q = query.trim();
    let q = q.strip_prefix('$').unwrap_or(q);
    let mut cur: Vec<J> = vec![v.clone()];
    for step in json_path_tokens(q) {
        let mut next = Vec::new();
        for node in &cur {
            match (&step, node) {
                (PathStep::Key(k), J::Object(m)) => {
                    if let Some(child) = m.get(k) {
                        next.push(child.clone());
                    }
                }
                (PathStep::Index(i), J::Array(a)) => {
                    if let Some(child) = a.get(*i) {
                        next.push(child.clone());
                    }
                }
                (PathStep::Wild, J::Array(a)) => next.extend(a.iter().cloned()),
                (PathStep::Wild, J::Object(m)) => next.extend(m.values().cloned()),
                _ => {}
            }
        }
        cur = next;
        if cur.is_empty() {
            return None;
        }
    }
    match cur.len() {
        0 => None,
        1 => cur.into_iter().next(),
        _ => Some(J::Array(cur)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_keys_indices_and_wildcards() {
        let v = json!({"a": {"b": [10, 20, 30]}});
        assert_eq!(json_path(&v, "$.a.b[1]"), Some(json!(20)));
        assert_eq!(json_path(&v, "a.b[0]"), Some(json!(10)));
        assert_eq!(json_path(&v, "$.a.b[*]"), Some(json!([10, 20, 30])));
        assert_eq!(json_path(&v, "$"), Some(v.clone()));
    }

    #[test]
    fn missing_paths_return_none() {
        let v = json!({"a": {"b": [1]}});
        assert_eq!(json_path(&v, "$.a.x"), None);
        assert_eq!(json_path(&v, "$.a.b[9]"), None);
        assert_eq!(json_path(&v, "$.nope[*]"), None);
    }

    #[test]
    fn object_wildcard_collects_values() {
        let v = json!({"x": {"k": 1}, "y": {"k": 2}});
        assert_eq!(json_path(&v, "$.*.k"), Some(json!([1, 2])));
    }

    #[test]
    fn bracket_quoted_key() {
        let v = json!({"a b": 7});
        assert_eq!(json_path(&v, "$['a b']"), Some(json!(7)));
    }

    #[test]
    fn tokenizes_path() {
        let toks = json_path_tokens("a.b[0][*]");
        assert_eq!(toks.len(), 4);
        assert!(matches!(toks[0], PathStep::Key(ref k) if k == "a"));
        assert!(matches!(toks[2], PathStep::Index(0)));
        assert!(matches!(toks[3], PathStep::Wild));
    }
}
