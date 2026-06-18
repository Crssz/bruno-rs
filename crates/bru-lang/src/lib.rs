//! bru-lang — `.bru` text <-> model codec (lossless, byte-stable round-trip).
//!
//! The acceptance bar is `serialize(parse(x)) == x` over a corpus of real Bruno
//! files (see this crate's integration tests).

mod parse;
mod serialize;

pub use parse::{parse, ParseError};
pub use serialize::serialize;

/// Parse then re-serialize — the round-trip used by the golden-file tests.
pub fn round_trip(input: &str) -> Result<String, ParseError> {
    Ok(serialize(&parse(input)?))
}
