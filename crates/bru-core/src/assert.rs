//! Declarative assertion evaluation against an HTTP response.
//!
//! The left-hand side is a Bruno expression (`res.status`, `res.body.user.name`,
//! `res.headers.content-type`, `res.responseTime`); the right-hand side is an
//! operator + expected value (`eq 200`, `contains ok`), defaulting to `eq`.
//! Full JS-expression LHS evaluation belongs to the scripting engine (later); M1
//! supports dotted/indexed paths, which cover the overwhelming common case.

use crate::request::Assertion;
use serde_json::Value as Json;

/// The response facts an assertion is checked against.
pub struct ResponseFacts<'a> {
    pub status: u16,
    pub headers: &'a [(String, String)],
    pub body_json: Option<&'a Json>,
    pub body_text: &'a str,
    pub response_time_ms: u128,
}

/// The result of checking one assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertOutcome {
    pub expr: String,
    pub operator: String,
    pub expected: String,
    pub actual: String,
    pub passed: bool,
}

/// Evaluate all enabled assertions against the response.
pub fn evaluate(assertions: &[Assertion], facts: &ResponseFacts) -> Vec<AssertOutcome> {
    assertions
        .iter()
        .filter(|a| a.enabled)
        .map(|a| {
            let actual = eval_response_expr(&a.expr, facts);
            let (operator, expected) = parse_operator(&a.value);
            let passed = compare(actual.as_deref(), operator, &expected);
            AssertOutcome {
                expr: a.expr.clone(),
                operator: operator.to_string(),
                expected,
                actual: actual.unwrap_or_else(|| "undefined".to_string()),
                passed,
            }
        })
        .collect()
}

/// Resolve a Bruno response expression (`res.status`, `res.body.user.name`,
/// `res.headers.x`, `res.responseTime`) to a string, or `None` if absent. Used
/// both by assertions and by post-response variable extraction.
pub fn eval_response_expr(expr: &str, facts: &ResponseFacts) -> Option<String> {
    let path = expr
        .strip_prefix("$res")
        .or_else(|| expr.strip_prefix("res"))
        .unwrap_or(expr)
        .trim_start_matches('.');

    let (head, rest) = match path.split_once('.') {
        Some((h, r)) => (h, Some(r)),
        None => (path, None),
    };

    match head {
        "status" => Some(facts.status.to_string()),
        "responseTime" => Some(facts.response_time_ms.to_string()),
        "headers" => {
            let key = rest?;
            facts
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v.clone())
        }
        "body" => match rest {
            None => facts
                .body_json
                .map(json_to_string)
                .or_else(|| Some(facts.body_text.to_string())),
            Some(p) => navigate(facts.body_json?, p).map(json_to_string),
        },
        _ => None,
    }
}

/// Navigate a JSON value by a dotted/indexed path like `user.roles[0].name`.
fn navigate<'a>(mut cur: &'a Json, path: &str) -> Option<&'a Json> {
    for raw in path.split('.') {
        let mut seg = raw;
        // A segment may carry trailing array indices: `roles[0][1]`.
        let (key, indices) = match seg.find('[') {
            Some(i) => {
                let (k, idx) = seg.split_at(i);
                seg = k;
                (seg, idx)
            }
            None => (seg, ""),
        };
        if !key.is_empty() {
            cur = cur.get(key)?;
        }
        let mut bracket = indices;
        while let Some(close) = bracket.find(']') {
            let n: usize = bracket.get(1..close)?.parse().ok()?;
            cur = cur.get(n)?;
            bracket = &bracket[close + 1..];
        }
    }
    Some(cur)
}

fn json_to_string(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Split a `value` cell into (operator, expected). Bare values mean `eq`.
fn parse_operator(value: &str) -> (&str, String) {
    const OPS: &[&str] = &[
        "eq",
        "neq",
        "gte",
        "lte",
        "gt",
        "lt",
        "contains",
        "notContains",
        "isNull",
        "isNotNull",
        "isEmpty",
        "isNotEmpty",
        "isDefined",
        "isUndefined",
        "isTrue",
        "isFalse",
    ];
    let trimmed = value.trim();
    for op in OPS {
        if let Some(rest) = trimmed.strip_prefix(op) {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return (op, rest.trim().to_string());
            }
        }
    }
    ("eq", trimmed.to_string())
}

fn compare(actual: Option<&str>, operator: &str, expected: &str) -> bool {
    // Unary operators first (they don't read `expected`).
    match operator {
        "isNull" => return matches!(actual, None | Some("null")),
        "isNotNull" => return !matches!(actual, None | Some("null")),
        "isUndefined" => return actual.is_none(),
        "isDefined" => return actual.is_some(),
        "isEmpty" => return matches!(actual, None | Some("")),
        "isNotEmpty" => return !matches!(actual, None | Some("")),
        "isTrue" => return actual == Some("true"),
        "isFalse" => return actual == Some("false"),
        _ => {}
    }
    let Some(actual) = actual else {
        return false;
    };
    match operator {
        // String comparison: `res.status` etc. already stringify cleanly, and
        // string `eq` avoids numeric-coercion surprises (`1e3` == `1000`,
        // `NaN` != `NaN`). Use gt/gte/lt/lte for numeric ordering.
        "eq" => actual == expected,
        "neq" => actual != expected,
        "contains" => actual.contains(expected),
        "notContains" => !actual.contains(expected),
        "gt" | "gte" | "lt" | "lte" => match (actual.parse::<f64>(), expected.parse::<f64>()) {
            (Ok(a), Ok(e)) => match operator {
                "gt" => a > e,
                "gte" => a >= e,
                "lt" => a < e,
                _ => a <= e,
            },
            _ => false,
        },
        _ => false,
    }
}
