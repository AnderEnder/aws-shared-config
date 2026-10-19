//! Writing a type that implements `serde::Serialize` into a section, in place.

use serde::ser::{self, Error as _, Impossible, Serialize};

use crate::document::SectionMut;
use crate::error::SerdeError;

impl SectionMut<'_> {
    /// Write the fields of `value` into the section, keeping everything else.
    ///
    /// `value` must serialize as a struct or map. Each field sets its key,
    /// replacing the old value in place so the line keeps its comment;
    /// `None` removes the key; and a struct or map field sets sub-properties.
    /// Keys `value` does not mention are left alone, as are comments and
    /// blank lines. Nothing changes if any field cannot be written.
    ///
    /// ```
    /// use aws_shared_config::{Document, FileKind};
    /// use serde::Serialize;
    ///
    /// #[derive(Serialize)]
    /// struct Settings<'a> {
    ///     region: &'a str,
    ///     output: Option<&'a str>,
    /// }
    ///
    /// let text = "[profile dev]\nregion = us-east-1 # main region\noutput = json\n";
    /// let mut doc = Document::parse(text, FileKind::Config)?;
    /// doc.profile_mut("dev").serialize(&Settings { region: "eu-west-1", output: None })?;
    /// assert_eq!(doc.to_string(), "[profile dev]\nregion = eu-west-1 # main region\n");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn serialize<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerdeError> {
        let Update::Map(updates) = value.serialize(ValueSerializer { depth: 0 })? else {
            return Err(SerdeError::custom(
                "a section can only be written from a struct or map",
            ));
        };

        // Work on a copy so that a failure leaves the document unchanged.
        let mut doc = self.document().clone();
        let mut section = doc.section_mut(self.id().clone());
        for (key, update) in updates {
            match update {
                Update::Set(value) => {
                    section.set(&key, &value)?;
                }
                Update::Remove => {
                    section.remove(&key);
                }
                Update::Map(sub_properties) => {
                    for (sub_key, update) in sub_properties {
                        match update {
                            Update::Set(value) => {
                                section.set_sub(&key, &sub_key, &value)?;
                            }
                            Update::Remove => {
                                section.remove_sub(&key, &sub_key);
                            }
                            Update::Map(_) => unreachable!("sub-properties do not nest"),
                        }
                    }
                }
            }
        }
        *self.document() = doc;
        Ok(())
    }
}

/// What serializing a value asks of the section.
enum Update {
    Set(String),
    Remove,
    Map(Vec<(String, Update)>),
}

/// Serializes a value at a given depth: 0 for the section itself, 1 for a
/// property, 2 for a sub-property. Only depths 0 and 1 may be maps.
struct ValueSerializer {
    depth: u8,
}

fn unsupported(what: &str) -> SerdeError {
    SerdeError::custom(format_args!("{what} cannot be written to a profile file"))
}

macro_rules! serialize_display {
    ($($method:ident: $ty:ty),*) => {$(
        fn $method(self, value: $ty) -> Result<Update, SerdeError> {
            Ok(Update::Set(value.to_string()))
        }
    )*};
}

impl ser::Serializer for ValueSerializer {
    type Ok = Update;
    type Error = SerdeError;
    type SerializeSeq = Impossible<Update, SerdeError>;
    type SerializeTuple = Impossible<Update, SerdeError>;
    type SerializeTupleStruct = Impossible<Update, SerdeError>;
    type SerializeTupleVariant = Impossible<Update, SerdeError>;
    type SerializeMap = Fields;
    type SerializeStruct = Fields;
    type SerializeStructVariant = Impossible<Update, SerdeError>;

    serialize_display! {
        serialize_bool: bool, serialize_char: char, serialize_str: &str,
        serialize_i8: i8, serialize_i16: i16, serialize_i32: i32, serialize_i64: i64,
        serialize_i128: i128, serialize_u8: u8, serialize_u16: u16, serialize_u32: u32,
        serialize_u64: u64, serialize_u128: u128, serialize_f32: f32, serialize_f64: f64
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Update, SerdeError> {
        std::str::from_utf8(value)
            .map(|text| Update::Set(text.to_owned()))
            .map_err(|_| unsupported("non-UTF-8 bytes"))
    }

    fn serialize_none(self) -> Result<Update, SerdeError> {
        Ok(Update::Remove)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Update, SerdeError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Update, SerdeError> {
        Err(unsupported("a unit value"))
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Update, SerdeError> {
        Err(unsupported(name))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Update, SerdeError> {
        Ok(Update::Set(variant.to_owned()))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Update, SerdeError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _value: &T,
    ) -> Result<Update, SerdeError> {
        Err(unsupported(variant))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, SerdeError> {
        Err(unsupported("a list"))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, SerdeError> {
        Err(unsupported("a tuple"))
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, SerdeError> {
        Err(unsupported(name))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, SerdeError> {
        Err(unsupported(variant))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Fields, SerdeError> {
        Fields::new(self.depth)
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Fields, SerdeError> {
        Fields::new(self.depth)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, SerdeError> {
        Err(unsupported(variant))
    }
}

/// The fields of a section (depth 0) or of a property's sub-properties
/// (depth 1).
struct Fields {
    field_depth: u8,
    entries: Vec<(String, Update)>,
    key: Option<String>,
}

impl Fields {
    fn new(depth: u8) -> Result<Self, SerdeError> {
        if depth > 1 {
            return Err(unsupported("a sub-property with sub-properties"));
        }
        Ok(Self {
            field_depth: depth + 1,
            entries: Vec::new(),
            key: None,
        })
    }

    fn push<T: Serialize + ?Sized>(&mut self, key: String, value: &T) -> Result<(), SerdeError> {
        let update = value.serialize(ValueSerializer {
            depth: self.field_depth,
        })?;
        self.entries.push((key, update));
        Ok(())
    }
}

impl ser::SerializeStruct for Fields {
    type Ok = Update;
    type Error = SerdeError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), SerdeError> {
        self.push(key.to_owned(), value)
    }

    fn end(self) -> Result<Update, SerdeError> {
        Ok(Update::Map(self.entries))
    }
}

impl ser::SerializeMap for Fields {
    type Ok = Update;
    type Error = SerdeError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), SerdeError> {
        // Keys are never maps, so the deepest level only accepts scalars.
        match key.serialize(ValueSerializer { depth: 2 })? {
            Update::Set(key) => self.key = Some(key),
            _ => return Err(SerdeError::custom("map keys must be strings")),
        }
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SerdeError> {
        let key = self
            .key
            .take()
            .ok_or_else(|| SerdeError::custom("a map value came before its key"))?;
        self.push(key, value)
    }

    fn end(self) -> Result<Update, SerdeError> {
        Ok(Update::Map(self.entries))
    }
}
