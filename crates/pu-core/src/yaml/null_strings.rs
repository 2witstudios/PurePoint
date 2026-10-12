//! Deserializer adapter that reads a YAML null (empty value, `~`, `null`) as an empty
//! string wherever a `String`/`&str` is requested.
//!
//! The previous libyaml-based reader accepted `key:` (no value) for any string field —
//! including inside `Vec<String>` and `HashMap<String, String>` — and produced `""`.
//! serde-saphyr rejects null for string targets, which would make hand-written files
//! that loaded before fail to load. This adapter wraps every nested deserializer so the
//! old leniency applies at any depth; all other requests pass straight through.

use std::fmt;

use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
};

/// Wraps a deserializer (and, transitively, everything it hands out).
pub(super) struct NullAsEmptyString<D>(pub D);

macro_rules! forward_wrapped {
    ($($method:ident ( $($arg:ident : $ty:ty),* );)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, $($arg: $ty,)* visitor: V) -> Result<V::Value, D::Error> {
                self.0.$method($($arg,)* Wrap(visitor))
            }
        )*
    };
}

/// Booleans and numbers are resolved from the scalar's own YAML type (plain `3` is an
/// integer, quoted `"3"` is a string) and then checked against the requested type, as
/// the previous reader did. serde-saphyr would otherwise coerce quoted scalars
/// (`"3"`, `"true"`) into numeric/bool fields.
macro_rules! any_typed {
    ($($method:ident),* $(,)?) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
                self.0.deserialize_any(Wrap(visitor))
            }
        )*
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for NullAsEmptyString<D> {
    type Error = D::Error;

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.0.deserialize_option(NullOr {
            visitor,
            owned: false,
        })
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.0.deserialize_option(NullOr {
            visitor,
            owned: true,
        })
    }

    any_typed! {
        deserialize_bool, deserialize_i8, deserialize_i16, deserialize_i32, deserialize_i64,
        deserialize_i128, deserialize_u8, deserialize_u16, deserialize_u32, deserialize_u64,
        deserialize_u128, deserialize_f32, deserialize_f64,
    }

    forward_wrapped! {
        deserialize_any();
        deserialize_char();
        deserialize_bytes();
        deserialize_byte_buf();
        deserialize_option();
        deserialize_unit();
        deserialize_unit_struct(name: &'static str);
        deserialize_newtype_struct(name: &'static str);
        deserialize_seq();
        deserialize_tuple(len: usize);
        deserialize_tuple_struct(name: &'static str, len: usize);
        deserialize_map();
        deserialize_struct(name: &'static str, fields: &'static [&'static str]);
        deserialize_enum(name: &'static str, variants: &'static [&'static str]);
        deserialize_identifier();
        deserialize_ignored_any();
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

/// String request: null → `""`, anything else → the real string deserialization.
struct NullOr<V> {
    visitor: V,
    owned: bool,
}

impl<'de, V: Visitor<'de>> Visitor<'de> for NullOr<V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.visitor.expecting(f)
    }

    fn visit_none<E: de::Error>(self) -> Result<V::Value, E> {
        self.visitor.visit_str("")
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        self.visitor.visit_str("")
    }

    fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<V::Value, D2::Error> {
        if self.owned {
            d.deserialize_string(self.visitor)
        } else {
            d.deserialize_str(self.visitor)
        }
    }
}

/// Visitor wrapper: re-wraps any nested deserializer / access object it receives.
struct Wrap<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for Wrap<V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.expecting(f)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<V::Value, E> {
        self.0.visit_bool(v)
    }
    fn visit_i8<E: de::Error>(self, v: i8) -> Result<V::Value, E> {
        self.0.visit_i8(v)
    }
    fn visit_i16<E: de::Error>(self, v: i16) -> Result<V::Value, E> {
        self.0.visit_i16(v)
    }
    fn visit_i32<E: de::Error>(self, v: i32) -> Result<V::Value, E> {
        self.0.visit_i32(v)
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<V::Value, E> {
        self.0.visit_i64(v)
    }
    fn visit_i128<E: de::Error>(self, v: i128) -> Result<V::Value, E> {
        self.0.visit_i128(v)
    }
    fn visit_u8<E: de::Error>(self, v: u8) -> Result<V::Value, E> {
        self.0.visit_u8(v)
    }
    fn visit_u16<E: de::Error>(self, v: u16) -> Result<V::Value, E> {
        self.0.visit_u16(v)
    }
    fn visit_u32<E: de::Error>(self, v: u32) -> Result<V::Value, E> {
        self.0.visit_u32(v)
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<V::Value, E> {
        self.0.visit_u64(v)
    }
    fn visit_u128<E: de::Error>(self, v: u128) -> Result<V::Value, E> {
        self.0.visit_u128(v)
    }
    fn visit_f32<E: de::Error>(self, v: f32) -> Result<V::Value, E> {
        self.0.visit_f32(v)
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<V::Value, E> {
        self.0.visit_f64(v)
    }
    fn visit_char<E: de::Error>(self, v: char) -> Result<V::Value, E> {
        self.0.visit_char(v)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<V::Value, E> {
        self.0.visit_str(v)
    }
    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<V::Value, E> {
        self.0.visit_borrowed_str(v)
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<V::Value, E> {
        self.0.visit_string(v)
    }
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<V::Value, E> {
        self.0.visit_bytes(v)
    }
    fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<V::Value, E> {
        self.0.visit_borrowed_bytes(v)
    }
    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<V::Value, E> {
        self.0.visit_byte_buf(v)
    }
    fn visit_none<E: de::Error>(self) -> Result<V::Value, E> {
        self.0.visit_none()
    }
    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        self.0.visit_unit()
    }

    fn visit_some<D2: Deserializer<'de>>(self, d: D2) -> Result<V::Value, D2::Error> {
        self.0.visit_some(NullAsEmptyString(d))
    }
    fn visit_newtype_struct<D2: Deserializer<'de>>(self, d: D2) -> Result<V::Value, D2::Error> {
        self.0.visit_newtype_struct(NullAsEmptyString(d))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<V::Value, A::Error> {
        self.0.visit_seq(Wrap(seq))
    }
    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<V::Value, A::Error> {
        self.0.visit_map(Wrap(map))
    }
    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        self.0.visit_enum(Wrap(data))
    }
}

/// Seed wrapper: the seed deserializes from a wrapped deserializer.
struct Seed<S>(S);

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<S> {
    type Value = S::Value;

    fn deserialize<D2: Deserializer<'de>>(self, d: D2) -> Result<S::Value, D2::Error> {
        self.0.deserialize(NullAsEmptyString(d))
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Wrap<A> {
    type Error = A::Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, A::Error> {
        self.0.next_element_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Wrap<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        self.0.next_key_seed(Seed(seed))
    }

    fn next_value_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value, A::Error> {
        self.0.next_value_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

impl<'de, A: EnumAccess<'de>> EnumAccess<'de> for Wrap<A> {
    type Error = A::Error;
    type Variant = Wrap<A::Variant>;

    fn variant_seed<T: DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<(T::Value, Self::Variant), A::Error> {
        self.0
            .variant_seed(Seed(seed))
            .map(|(value, variant)| (value, Wrap(variant)))
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Wrap<A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.0.unit_variant()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, A::Error> {
        self.0.newtype_variant_seed(Seed(seed))
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.0.tuple_variant(len, Wrap(visitor))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.0.struct_variant(fields, Wrap(visitor))
    }
}
