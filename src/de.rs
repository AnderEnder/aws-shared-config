//! Reading a section into a type that implements `serde::Deserialize`.

use std::str::FromStr;

use serde::de::value::{MapDeserializer, StrDeserializer};
use serde::de::{self, Deserialize, Error as _, IntoDeserializer, Visitor};

use crate::document::Section;
use crate::error::SerdeError;
use crate::parse;

impl<'a> Section<'a> {
    /// Deserialize the section into `T`, usually a struct with a field per
    /// key. Values are text; they are parsed when a field asks for a number
    /// or a boolean. A field that is a struct or map reads sub-properties.
    /// Keys the type does not name are ignored, unless it denies unknown
    /// fields.
    ///
    /// ```
    /// use aws_shared_config::{Document, FileKind};
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct Settings {
    ///     region: String,
    ///     max_attempts: Option<u32>,
    /// }
    ///
    /// let doc = Document::parse("[profile dev]\nregion = eu-west-1\n", FileKind::Config)?;
    /// let settings: Settings = doc.profile("dev").unwrap().deserialize()?;
    /// assert_eq!(settings.region, "eu-west-1");
    /// assert_eq!(settings.max_attempts, None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn deserialize<T: Deserialize<'a>>(&self) -> Result<T, SerdeError> {
        let entries = self
            .iter()
            .map(|(key, value)| (key, Value::property(key, value)));
        T::deserialize(MapDeserializer::new(entries))
    }
}

/// A property or sub-property value, with its key for error messages.
#[derive(Clone, Copy)]
struct Value<'a> {
    key: &'a str,
    text: &'a str,
    /// Properties may hold sub-properties; sub-properties cannot nest.
    may_nest: bool,
}

impl<'a> Value<'a> {
    fn property(key: &'a str, text: &'a str) -> Self {
        Self {
            key,
            text,
            may_nest: true,
        }
    }

    fn sub_properties(&self) -> Option<impl Iterator<Item = (&'a str, Value<'a>)>> {
        let nested = self.may_nest && (self.text.is_empty() || self.text.starts_with('\n'));
        nested.then(|| {
            parse::sub_properties(self.text).map(|(key, text)| {
                let value = Value {
                    key,
                    text,
                    may_nest: false,
                };
                (key, value)
            })
        })
    }

    fn invalid(&self, expected: &str) -> SerdeError {
        SerdeError::custom(format_args!(
            "`{}`: expected {expected}, found `{}`",
            self.key, self.text
        ))
    }

    fn parse<T: FromStr>(&self, expected: &str) -> Result<T, SerdeError> {
        self.text.parse().map_err(|_| self.invalid(expected))
    }
}

impl<'de> IntoDeserializer<'de, SerdeError> for Value<'de> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

macro_rules! deserialize_parsed {
    ($($method:ident => $visit:ident: $ty:ty, $expected:literal;)*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
            visitor.$visit(self.parse::<$ty>($expected)?)
        }
    )*};
}

impl<'de> de::Deserializer<'de> for Value<'de> {
    type Error = SerdeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        match self.sub_properties() {
            Some(entries) if !self.text.is_empty() => {
                visitor.visit_map(MapDeserializer::new(entries))
            }
            _ => visitor.visit_borrowed_str(self.text),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        match self.text.to_ascii_lowercase().as_str() {
            "true" => visitor.visit_bool(true),
            "false" => visitor.visit_bool(false),
            _ => Err(self.invalid("`true` or `false`")),
        }
    }

    deserialize_parsed! {
        deserialize_i8 => visit_i8: i8, "an integer";
        deserialize_i16 => visit_i16: i16, "an integer";
        deserialize_i32 => visit_i32: i32, "an integer";
        deserialize_i64 => visit_i64: i64, "an integer";
        deserialize_i128 => visit_i128: i128, "an integer";
        deserialize_u8 => visit_u8: u8, "a non-negative integer";
        deserialize_u16 => visit_u16: u16, "a non-negative integer";
        deserialize_u32 => visit_u32: u32, "a non-negative integer";
        deserialize_u64 => visit_u64: u64, "a non-negative integer";
        deserialize_u128 => visit_u128: u128, "a non-negative integer";
        deserialize_f32 => visit_f32: f32, "a number";
        deserialize_f64 => visit_f64: f64, "a number";
        deserialize_char => visit_char: char, "a single character";
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        visitor.visit_borrowed_str(self.text)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        visitor.visit_borrowed_str(self.text)
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        visitor.visit_borrowed_str(self.text)
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        visitor.visit_borrowed_bytes(self.text.as_bytes())
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        visitor.visit_borrowed_bytes(self.text.as_bytes())
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        // A key that is present always has a value, possibly empty.
        visitor.visit_some(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        if self.text.is_empty() {
            visitor.visit_unit()
        } else {
            Err(self.invalid("an empty value"))
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, SerdeError> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, SerdeError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, SerdeError> {
        Err(self.invalid("a single value (lists are not supported)"))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, SerdeError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, SerdeError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        match self.sub_properties() {
            Some(entries) => visitor.visit_map(MapDeserializer::new(entries)),
            None => Err(self.invalid("sub-properties")),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, SerdeError> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, SerdeError> {
        // Unit variants, spelled as the value: `retry_mode = standard`.
        let variant: StrDeserializer<'_, SerdeError> = self.text.into_deserializer();
        visitor.visit_enum(variant)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, SerdeError> {
        visitor.visit_unit()
    }
}
