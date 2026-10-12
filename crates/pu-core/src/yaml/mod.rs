//! Single entry point for every YAML document pu-core reads or writes.
//!
//! All `.pu/` YAML (project config, global agent settings, agent/swarm/trigger/schedule
//! definitions, template frontmatter) goes through [`from_str`] and [`to_string`], so the
//! parser/emitter configuration lives in exactly one place.
//!
//! Backed by `serde-saphyr` (pure safe Rust). Options are chosen to keep the semantics
//! of the previous `serde_yaml_ng`/libyaml stack; `compat_tests` pins them.

use serde::{Deserialize, Serialize};

mod null_strings;

/// A YAML read or write failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    De(#[from] serde_saphyr::DeserializeError),
    #[error("{0}")]
    Ser(#[from] serde_saphyr::SerializeError),
}

fn de_options() -> serde_saphyr::Options {
    let mut options = serde_saphyr::Options::default();
    // YAML 1.2 core schema booleans only: `yes`/`no`/`on`/`off` stay strings.
    options.strict_booleans = true;
    // `<<` is an ordinary key, not a merge.
    options.merge_keys = serde_saphyr::MergeKeyPolicy::AsOrdinary;
    // Repeated mapping keys: the later entry wins (the old reader did this for maps).
    options.duplicate_keys = serde_saphyr::DuplicateKeyPolicy::LastWins;
    // `!!binary` on a string field is kept as written, not base64-decoded.
    options.ignore_binary_tag_for_string = true;
    // Single-line error messages (`... at line L, column C`), like before; no
    // multi-line source snippets.
    options.with_snippet = false;
    options
}

/// Parse a single YAML document into `T`.
pub fn from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    Ok(serde_saphyr::with_deserializer_from_str_with_options(
        input,
        de_options(),
        |de| T::deserialize(null_strings::NullAsEmptyString(de)),
    )?)
}

/// Serialize `value` as a YAML document (always newline-terminated).
pub fn to_string<T: Serialize>(value: &T) -> Result<String, Error> {
    Ok(serde_saphyr::to_string(value)?)
}

#[cfg(test)]
mod compat_tests;
#[cfg(test)]
mod legacy_fixtures;
