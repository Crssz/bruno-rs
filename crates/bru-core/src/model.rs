//! The lossless `.bru` document model. See the crate docs for the design rationale.

use indexmap::IndexMap;

/// A parsed `.bru` file: an ordered sequence of named blocks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BruFile {
    pub blocks: Vec<Block>,
}

impl BruFile {
    pub fn new() -> Self {
        Self::default()
    }

    /// First block with the given name, if any (block names are unique in
    /// practice for the singular blocks we query, e.g. `meta`).
    pub fn block(&self, name: &str) -> Option<&Block> {
        self.blocks.iter().find(|b| b.name == name)
    }

    /// Convenience: read a single value out of a dictionary block (e.g. the
    /// request name from `meta`). Returns the first enabled entry whose key
    /// matches. Used by the sidebar tree and, later, the semantic layer.
    pub fn dict_value(&self, block: &str, key: &str) -> Option<&str> {
        match self.block(block)?.content {
            BlockContent::Dict(ref entries) => entries
                .iter()
                .find(|e| e.key.name() == key)
                .map(|e| e.value.as_inline()),
            _ => None,
        }
    }
}

/// One top-level block, e.g. `meta { ... }`, `headers { ... }`, `body:json { ... }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The literal block name, e.g. `meta`, `params:query`, `auth:oauth2`,
    /// `body:json`, `get`. Colons are part of the name.
    pub name: String,
    pub content: BlockContent,
}

/// The three block shapes in the `.bru` grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockContent {
    /// `{ key: value ... }` — ordered key/value pairs.
    Dict(Vec<Entry>),
    /// `{ ...raw text... }` — verbatim body (json/text/xml/sparql/graphql/script/
    /// tests/docs/example). Stored exactly as it appeared between the braces.
    Text(String),
    /// `[ item ... ]` — bare-token list block (`vars:secret` in env files).
    List(Vec<String>),
}

/// One `key: value` pair inside a dictionary block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Decorator lines (`@name(...)`) that appeared immediately above the pair.
    pub annotations: Vec<Annotation>,
    /// `~` prefix in the source → this pair is disabled.
    pub disabled: bool,
    /// `@` prefix on a var name → local var (only meaningful in `vars:*` blocks).
    pub local: bool,
    pub key: Key,
    pub value: Value,
}

/// A dictionary key, preserving whether it was quoted in the source so the exact
/// surface form is reproduced on serialize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Emitted verbatim (no special chars).
    Bare(String),
    /// Emitted wrapped in `"..."` with internal `"` escaped as `\"`.
    Quoted(String),
}

impl Key {
    /// The unquoted, un-prefixed key name.
    pub fn name(&self) -> &str {
        match self {
            Key::Bare(s) | Key::Quoted(s) => s,
        }
    }
}

/// A dictionary value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Single-line value, trimmed (may be empty).
    Inline(String),
    /// `[ ... ]` list value (e.g. `meta.tags`).
    List(Vec<String>),
    /// `''' ... '''` multiline value, with optional trailing `@contentType(...)`.
    Multiline {
        text: String,
        content_type: Option<String>,
    },
}

impl Value {
    /// The inline string if this is an [`Value::Inline`], else `""`.
    pub fn as_inline(&self) -> &str {
        match self {
            Value::Inline(s) => s,
            _ => "",
        }
    }
}

/// A `@name` / `@name('value')` decorator attached to a pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub name: String,
    /// `None` for a bare flag annotation (`@name`); `Some` for `@name(value)`.
    pub value: Option<String>,
}

/// A reusable ordered string map for the future semantic layer. Re-exported so
/// downstream crates depend on one `indexmap` version. (Not used by the lossless
/// model itself, which preserves order via `Vec`.)
pub type OrderedMap = IndexMap<String, String>;
