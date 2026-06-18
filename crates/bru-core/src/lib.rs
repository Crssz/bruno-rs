//! bru-core — the UI-agnostic domain model for bruno-rs.
//!
//! # Lossless block model (M0)
//!
//! A `.bru` file is modeled as an ordered list of named [`Block`]s. Each block
//! is either a *dictionary* (ordered key/value [`Entry`]s), a verbatim *text*
//! block (JSON/script/docs bodies, captured byte-for-byte), or a *list* block
//! (the `vars:secret` array in env files).
//!
//! The model is deliberately generic rather than a typed mirror of every Bruno
//! field. This is what makes byte-stable round-trip robust: `bru-lang` preserves
//! the parse order and the surface form (quoted vs bare keys, `~`/`@` prefixes,
//! verbatim bodies) and replays them, so `serialize(parse(x)) == x` holds for any
//! canonical Bruno file — including ones whose block/field order Bruno's own
//! serializer would rewrite.
//!
//! Typed, semantic access (method, url, headers, auth, body) is layered on top of
//! this model in later milestones (when `bru-http`/`bru-app` need it); it is a
//! *view*, never the serialization source of truth.

mod assert;
mod collection;
mod interp;
mod model;
mod request;

pub use assert::{
    eval_response_expr, evaluate as evaluate_assertions, AssertOutcome, ResponseFacts,
};
pub use collection::{CollectionTree, Folder, RequestItem};
pub use interp::interpolate;
pub use model::{Annotation, Block, BlockContent, BruFile, Entry, Key, Value, HTTP_VERBS};
pub use request::{ApiKeyPlacement, Assertion, Auth, Body, KeyVal, OAuth2, Request, Var};
