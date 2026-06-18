//! `.bru` text -> model. A hand-rolled block scanner.
//!
//! Deviation from the plan's `winnow` choice: the `.bru` format is line/block
//! oriented (named blocks of `key: value` lines, verbatim text bodies, and small
//! lists), and a direct scanner makes byte-exact capture of text blocks and the
//! `~`/`@`/quoted-key surface form clearer than a combinator grammar. winnow can
//! replace this later behind the same `parse`/`serialize` API without touching
//! callers.

use bru_core::{Annotation, Block, BlockContent, BruFile, Entry, Key, Value};
use thiserror::Error;

/// Block names whose body is verbatim text rather than a `key: value` dictionary.
const TEXT_BLOCKS: &[&str] = &[
    "body",
    "body:json",
    "body:text",
    "body:xml",
    "body:sparql",
    "body:graphql",
    "body:graphql:vars",
    "script:pre-request",
    "script:post-response",
    "tests",
    "docs",
    "example",
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("line {line}: block `{name}` is not terminated by a closing `}}`")]
    UnterminatedBlock { name: String, line: usize },
    #[error("line {line}: list block `{name}` is not terminated by a closing `]`")]
    UnterminatedList { name: String, line: usize },
    #[error("line {line}: expected `key: value`, found `{got}`")]
    MissingColon { got: String, line: usize },
    #[error("line {line}: unterminated quoted key")]
    UnterminatedQuotedKey { line: usize },
    #[error("line {line}: unterminated ''' block")]
    UnterminatedMultiline { line: usize },
    #[error("line {line}: unexpected text after ''' value: `{got}`")]
    UnexpectedTrailing { line: usize, got: String },
}

/// Parse `.bru` text into a [`BruFile`].
pub fn parse(input: &str) -> Result<BruFile, ParseError> {
    Parser { input, pos: 0 }.parse_file()
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    /// The current line (without its trailing `\n`), or `None` at EOF.
    fn peek_line(&self) -> Option<&'a str> {
        if self.pos >= self.input.len() {
            return None;
        }
        let rest = &self.input[self.pos..];
        let end = rest.find('\n').unwrap_or(rest.len());
        Some(&rest[..end])
    }

    fn advance_line(&mut self) {
        let rest = &self.input[self.pos..];
        match rest.find('\n') {
            Some(i) => self.pos += i + 1,
            None => self.pos = self.input.len(),
        }
    }

    /// 1-based line number at the current byte offset (for error messages).
    fn line_no(&self) -> usize {
        self.input[..self.pos]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1
    }

    fn parse_file(&mut self) -> Result<BruFile, ParseError> {
        let mut blocks = Vec::new();
        loop {
            self.skip_blank_lines();
            if self.peek_line().is_none() {
                break;
            }
            blocks.push(self.parse_block()?);
        }
        Ok(BruFile { blocks })
    }

    fn skip_blank_lines(&mut self) {
        while let Some(line) = self.peek_line() {
            if line.trim().is_empty() {
                self.advance_line();
            } else {
                break;
            }
        }
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        let header = self.peek_line().expect("non-empty header line");
        let brace = header.find('{');
        let bracket = header.find('[');
        let (delim_idx, is_list) = match (brace, bracket) {
            (Some(b), Some(k)) if k < b => (k, true),
            (Some(b), _) => (b, false),
            (None, Some(k)) => (k, true),
            (None, None) => {
                // No opener — treat the line as a malformed pair for a clearer error.
                let line = self.line_no();
                return Err(ParseError::MissingColon {
                    got: header.trim().to_string(),
                    line,
                });
            }
        };
        let name = header[..delim_idx].trim().to_string();
        self.advance_line(); // consume the `name {` / `name [` header line

        if is_list {
            self.parse_list_block(name)
        } else if TEXT_BLOCKS.contains(&name.as_str()) {
            self.parse_text_block(name)
        } else {
            self.parse_dict_block(name)
        }
    }

    /// Capture verbatim bytes until a line that is exactly `}`.
    fn parse_text_block(&mut self, name: String) -> Result<Block, ParseError> {
        let start = self.pos;
        loop {
            match self.peek_line() {
                None => {
                    return Err(ParseError::UnterminatedBlock {
                        name,
                        line: self.line_no(),
                    })
                }
                Some("}") => {
                    let raw = &self.input[start..self.pos];
                    // Drop the single newline that precedes the closing `}`.
                    let raw = raw.strip_suffix('\n').unwrap_or(raw);
                    let raw = raw.strip_suffix('\r').unwrap_or(raw);
                    self.advance_line(); // consume `}`
                    return Ok(Block {
                        name,
                        content: BlockContent::Text(raw.to_string()),
                    });
                }
                Some(_) => self.advance_line(),
            }
        }
    }

    /// Read list items until a line that is exactly `]` (blanks skipped, trailing
    /// commas stripped). Shared by `name [` blocks and `key: [` values; `name` is
    /// only used to label an unterminated-list error.
    fn read_list_items(&mut self, name: &str) -> Result<Vec<String>, ParseError> {
        let mut items = Vec::new();
        loop {
            match self.peek_line() {
                None => {
                    return Err(ParseError::UnterminatedList {
                        name: name.to_string(),
                        line: self.line_no(),
                    })
                }
                Some(line) if line.trim() == "]" => {
                    self.advance_line();
                    return Ok(items);
                }
                Some(line) if line.trim().is_empty() => self.advance_line(),
                Some(line) => {
                    // Bruno comma-separates list items; strip a trailing comma so
                    // a multi-item list round-trips (and real env secret lists import).
                    let item = line.trim().trim_end_matches(',').trim();
                    if !item.is_empty() {
                        items.push(item.to_string());
                    }
                    self.advance_line();
                }
            }
        }
    }

    fn parse_list_block(&mut self, name: String) -> Result<Block, ParseError> {
        let items = self.read_list_items(&name)?;
        Ok(Block { name, content: BlockContent::List(items) })
    }

    fn parse_dict_block(&mut self, name: String) -> Result<Block, ParseError> {
        let mut entries = Vec::new();
        let mut pending: Vec<Annotation> = Vec::new();
        loop {
            match self.peek_line() {
                None => {
                    return Err(ParseError::UnterminatedBlock {
                        name,
                        line: self.line_no(),
                    })
                }
                Some("}") => {
                    self.advance_line();
                    return Ok(Block {
                        name,
                        content: BlockContent::Dict(entries),
                    });
                }
                Some(line) if line.trim().is_empty() => self.advance_line(),
                Some(line) => {
                    if let Some(ann) = parse_annotation_line(line) {
                        pending.push(ann);
                        self.advance_line();
                    } else {
                        let entry = self.parse_pair(&name, std::mem::take(&mut pending))?;
                        entries.push(entry);
                    }
                }
            }
        }
    }

    /// Parse one `key: value` pair, consuming as many lines as the value needs.
    fn parse_pair(
        &mut self,
        block: &str,
        annotations: Vec<Annotation>,
    ) -> Result<Entry, ParseError> {
        let line_start = self.pos;
        let line = self.peek_line().expect("pair line");
        let trimmed = line.trim_start();
        let mut rest = trimmed;

        let disabled = rest.starts_with('~');
        if disabled {
            rest = &rest[1..];
        }
        let is_vars = block == "vars:pre-request" || block == "vars:post-response";
        let local = is_vars && rest.starts_with('@');
        if local {
            rest = &rest[1..];
        }

        let (key, after_key) = self.parse_key(rest)?;
        let after = after_key.trim_start();
        let value_str = match after.strip_prefix(':') {
            Some(v) => v,
            None => {
                return Err(ParseError::MissingColon {
                    got: trimmed.to_string(),
                    line: self.line_no(),
                })
            }
        };
        let vtrim = value_str.trim_start();

        let value = if vtrim.starts_with("'''") {
            self.parse_multiline_value(line_start, line, vtrim)?
        } else if vtrim.trim_end() == "[" {
            self.advance_line(); // consume `key: [`
            self.parse_list_value(block)?
        } else {
            self.advance_line();
            Value::Inline(vtrim.trim_end().to_string())
        };

        Ok(Entry {
            annotations,
            disabled,
            local,
            key,
            value,
        })
    }

    fn parse_key(&self, rest: &'a str) -> Result<(Key, &'a str), ParseError> {
        if let Some(after_quote) = rest.strip_prefix('"') {
            let mut key = String::new();
            let mut chars = after_quote.char_indices();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => {
                        // Only `\"` is an escape; a lone backslash stays literal.
                        if after_quote[i + 1..].starts_with('"') {
                            key.push('"');
                            chars.next();
                        } else {
                            key.push('\\');
                        }
                    }
                    '"' => {
                        let remainder = &after_quote[i + 1..];
                        return Ok((Key::Quoted(key), remainder));
                    }
                    _ => key.push(c),
                }
            }
            Err(ParseError::UnterminatedQuotedKey {
                line: self.line_no(),
            })
        } else {
            match rest.find(':') {
                Some(c) => Ok((Key::Bare(rest[..c].trim().to_string()), &rest[c..])),
                None => Err(ParseError::MissingColon {
                    got: rest.trim().to_string(),
                    line: self.line_no(),
                }),
            }
        }
    }

    fn parse_list_value(&mut self, block: &str) -> Result<Value, ParseError> {
        Ok(Value::List(self.read_list_items(block)?))
    }

    /// Capture a `''' ... '''` value verbatim (with optional trailing
    /// `@contentType(...)`), preserving the exact inner bytes for round-trip.
    fn parse_multiline_value(
        &mut self,
        line_start: usize,
        line: &'a str,
        vtrim: &'a str,
    ) -> Result<Value, ParseError> {
        // Byte offset of the opening ''' in the source.
        let open_off = line_start + (line.len() - vtrim.len());
        let inner_start = open_off + 3;
        // Bound the closing-''' search to THIS block: never search past the
        // block's `}` line. Otherwise a single unterminated ''' would silently
        // swallow everything up to the next ''' anywhere later in the file.
        let region = &self.input[inner_start..];
        let limit = block_close_offset(region);
        let close_rel = region[..limit]
            .find("'''")
            .ok_or(ParseError::UnterminatedMultiline {
                line: self.line_no(),
            })?;
        let inner = self.input[inner_start..inner_start + close_rel].to_string();
        let after_close = inner_start + close_rel + 3;

        // Whatever remains on the closing line after ''' may hold an @contentType;
        // anything else is malformed and must not be silently dropped.
        let tail_end = self.input[after_close..]
            .find('\n')
            .map(|i| after_close + i)
            .unwrap_or(self.input.len());
        let tail = self.input[after_close..tail_end].trim();
        let content_type = tail
            .strip_prefix("@contentType(")
            .and_then(|t| t.strip_suffix(')'))
            .map(|t| t.to_string());
        if content_type.is_none() && !tail.is_empty() {
            return Err(ParseError::UnexpectedTrailing {
                line: self.line_no(),
                got: tail.to_string(),
            });
        }

        // Advance the cursor past the closing line.
        self.pos = tail_end;
        if self.pos < self.input.len() {
            self.pos += 1; // consume the newline
        }

        Ok(Value::Multiline {
            text: inner,
            content_type,
        })
    }
}

/// Byte offset within `region` of the first line that is exactly `}` (a block
/// closer at column 0), or `region.len()` if there is none. Used to bound a
/// `'''` value's closing search to its own block.
fn block_close_offset(region: &str) -> usize {
    let mut pos = 0;
    loop {
        let line_end = region[pos..]
            .find('\n')
            .map(|i| pos + i)
            .unwrap_or(region.len());
        if &region[pos..line_end] == "}" {
            return pos;
        }
        if line_end >= region.len() {
            return region.len();
        }
        pos = line_end + 1;
    }
}

/// If `line` is a decorator line (`@name` / `@name(args)` with no `key:` after),
/// parse it; otherwise `None` (it is an ordinary, possibly `@local`, pair).
fn parse_annotation_line(line: &str) -> Option<Annotation> {
    let t = line.trim();
    let body = t.strip_prefix('@')?;
    let colon = body.find(':');
    let paren = body.find('(');
    // A `:` before any `(` means this is `@name: value` — a local var, not a decorator.
    let is_annotation = match (paren, colon) {
        (Some(p), Some(c)) => p < c,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => true,
    };
    if !is_annotation {
        return None;
    }
    match paren {
        None => Some(Annotation {
            name: body.trim().to_string(),
            value: None,
        }),
        Some(p) => {
            let name = body[..p].trim().to_string();
            let inner = body[p + 1..].strip_suffix(')').unwrap_or(&body[p + 1..]);
            let value = strip_arg_quotes(inner.trim());
            Some(Annotation {
                name,
                value: Some(value),
            })
        }
    }
}

/// Strip one matching pair of surrounding single or double quotes from an
/// annotation argument, mirroring Bruno's quoted-arg unwrapping.
fn strip_arg_quotes(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'))
    {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}
