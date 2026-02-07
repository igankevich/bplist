use alloc::vec::Vec;

use paste::paste;
use serde::Serialize;
use serde::ser::SerializeMap;
use serde::ser::SerializeSeq;

use crate::Infallible;
use crate::Trailer;

type Result<T> = core::result::Result<T, Infallible>;

fn bytes_needed(value: usize) -> u8 {
    if u8::try_from(value).is_ok() {
        return 1;
    }
    if u16::try_from(value).is_ok() {
        return 2;
    }
    if u32::try_from(value).is_ok() {
        return 4;
    }
    8
}

fn write_varint(value: usize, bytes_needed: u8, output: &mut Vec<u8>) {
    match bytes_needed {
        1 => {
            assert!(value <= usize::from(u8::MAX), "{value}");
            output.extend_from_slice(&(value as u8).to_be_bytes())
        }
        2 => {
            assert!(value <= usize::from(u16::MAX), "{value}");
            output.extend_from_slice(&(value as u16).to_be_bytes())
        }
        4 => {
            assert!(value <= u32::MAX as usize, "{value}");
            output.extend_from_slice(&(value as u32).to_be_bytes())
        }
        8 => output.extend_from_slice(&(value as u64).to_be_bytes()),
        _ => unreachable!(),
    }
}

/// Serialize value as a property list.
///
/// Returns the serialized property list.
pub fn to_vec<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    let mut ser = Serializer::new();
    value
        .serialize(&mut ser)
        .expect("Serialization is infallible");
    ser.finish()
}

/// Property list serializer.
pub struct Serializer {
    output: Vec<u8>,
    offsets: Vec<usize>,
    object_ref_size: u8,
}

impl Serializer {
    /// Create new serializer with the default object reference size (8).
    pub fn new() -> Self {
        Self::with_object_ref_size(8)
    }

    /// Create new serializer with the specified object reference size.
    ///
    /// Object reference is an object index. If the object being serialized has less than
    /// `u16::MAX` fields in total (inclusing array and map elements), then you can set it to 2
    /// bytes to reduce the serialized object size. Valid values are 1, 2, 4 or 8 bytes.
    pub fn with_object_ref_size(object_ref_size: u8) -> Self {
        assert!([1, 2, 4, 8].contains(&object_ref_size));
        let mut output = Vec::with_capacity(4096);
        output.extend_from_slice(b"bplist00");
        Self {
            output,
            offsets: Vec::new(),
            object_ref_size,
        }
    }

    /// Finish serializing.
    ///
    /// This function writes offset table and trailer.
    ///
    /// Returns the resulting serialized property list.
    pub fn finish(mut self) -> Vec<u8> {
        let max_offset = self.offsets.iter().copied().max().unwrap_or(0);
        let offset_int_size = bytes_needed(max_offset);
        let offset_table_offset = self.output.len();
        let trailer = Trailer {
            num_objects: self.offsets.len() as u64,
            top_object: 0,
            offset_table_offset: offset_table_offset as u64,
            offset_int_size,
            object_ref_size: self.object_ref_size,
        };
        for offset in self.offsets.iter().copied() {
            write_varint(offset, offset_int_size, &mut self.output);
        }
        trailer.write(&mut self.output);
        self.output
    }

    fn write_count(&mut self, mask: u8, count: usize) {
        let p: u8 = if count < 0b1111 { count as u8 } else { 0b1111 };
        self.output.push(mask | p);
        if p == 0b1111 {
            self.write_large_count(count);
        }
    }

    fn write_large_count(&mut self, count: usize) {
        if let Ok(count) = u8::try_from(count) {
            self.output.push(0b0001_0000_u8);
            self.output.extend_from_slice(&count.to_be_bytes());
            return;
        }
        if let Ok(count) = u16::try_from(count) {
            self.output.push(0b0001_0000_u8 | 1_u8);
            self.output.extend_from_slice(&count.to_be_bytes());
            return;
        }
        if let Ok(count) = u32::try_from(count) {
            self.output.push(0b0001_0000_u8 | 2_u8);
            self.output.extend_from_slice(&count.to_be_bytes());
            return;
        }
        let count = count as u64;
        self.output.push(0b0001_0000_u8 | 3_u8);
        self.output.extend_from_slice(&count.to_be_bytes());
    }

    fn write_object_index(&mut self, index: usize) {
        write_varint(index, self.object_ref_size, &mut self.output);
    }
}

impl<'a> serde::ser::Serializer for &'a mut Serializer {
    type Ok = ();
    type Error = Infallible;

    type SerializeSeq = SeqSerializer<'a>;
    type SerializeTuple = SeqSerializer<'a>;
    type SerializeTupleStruct = SeqSerializer<'a>;
    type SerializeTupleVariant = Unimplemented;
    type SerializeMap = MapSerializer<'a>;
    type SerializeStruct = MapSerializer<'a>;
    type SerializeStructVariant = Unimplemented;

    fn serialize_bool(self, v: bool) -> Result<()> {
        self.offsets.push(self.output.len());
        self.output.push(if v { 0b0000_1001 } else { 0b0000_1000 });
        Ok(())
    }

    serialize_integer! {
        // Signed.
        (i8 0_u8)
        (i16 1_u8)
        (i32 2_u8)
        (i64 3_u8)
        (i128 4_u8)
        // Unsigned.
        (u8 0_u8)
        (u16 1_u8)
        (u32 2_u8)
        (u64 3_u8)
        (u128 4_u8)
    }

    fn serialize_f32(self, v: f32) -> Result<()> {
        self.offsets.push(self.output.len());
        self.output.push(0b0010_0000_u8 | 2_u8);
        self.output.extend_from_slice(&v.to_be_bytes());
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<()> {
        self.offsets.push(self.output.len());
        self.output.push(0b0010_0000_u8 | 3_u8);
        self.output.extend_from_slice(&v.to_be_bytes());
        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<()> {
        self.serialize_u32(v.into())
    }

    fn serialize_str(self, v: &str) -> Result<()> {
        self.offsets.push(self.output.len());
        self.write_count(0b0111_0000, v.len());
        self.output.extend_from_slice(v.as_bytes());
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        self.offsets.push(self.output.len());
        self.write_count(0b0100_0000, v.len());
        self.output.extend_from_slice(v);
        Ok(())
    }

    fn serialize_none(self) -> Result<()> {
        self.serialize_unit()
    }

    fn serialize_some<T>(self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        self.serialize_unit()
    }

    fn serialize_unit(self) -> Result<()> {
        self.offsets.push(self.output.len());
        self.output.push(0b0000_0000_u8);
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<()> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _value: &T,
    ) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        // TODO dict
        self.serialize_str(variant)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq> {
        Ok(SeqSerializer::new(self, len))
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(len))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap> {
        Ok(MapSerializer::new(self, len))
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        self.serialize_map(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        unimplemented!()
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        unimplemented!()
    }
}

macro_rules! serialize_integer {
    ($(($int: ident $p: literal))+) => {
        paste! {
            $(
                fn [<serialize_ $int>](self, v: $int) -> Result<()> {
                    self.offsets.push(self.output.len());
                    self.output.push(0b0001_0000_u8 | $p);
                    self.output.extend_from_slice(&v.to_be_bytes());
                    Ok(())
                }
            )+
        }
    };
}

use serialize_integer;

#[doc(hidden)]
pub struct SeqSerializer<'a> {
    parent: &'a mut Serializer,
    index: usize,
    values: Vec<usize>,
}

impl<'a> SeqSerializer<'a> {
    fn new(parent: &'a mut Serializer, len: Option<usize>) -> Self {
        // Reserve offset so that top object is always 0.
        let index = parent.offsets.len();
        parent.offsets.push(usize::MAX);
        let capacity = len.unwrap_or(0);
        Self {
            parent,
            index,
            values: Vec::with_capacity(capacity),
        }
    }
}

impl<'a> SerializeSeq for SeqSerializer<'a> {
    type Ok = ();
    type Error = Infallible;

    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        self.values.push(self.parent.offsets.len());
        value.serialize(&mut *self.parent)
    }

    fn end(self) -> Result<()> {
        self.parent.offsets[self.index] = self.parent.output.len();
        self.parent.write_count(0b1010_0000_u8, self.values.len());
        for value in self.values.iter().copied() {
            self.parent.write_object_index(value);
        }
        Ok(())
    }
}

impl<'a> serde::ser::SerializeTuple for SeqSerializer<'a> {
    type Ok = ();
    type Error = Infallible;

    fn serialize_element<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<()> {
        SerializeSeq::end(self)
    }
}

impl<'a> serde::ser::SerializeTupleStruct for SeqSerializer<'a> {
    type Ok = ();
    type Error = Infallible;

    fn serialize_field<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<()> {
        SerializeSeq::end(self)
    }
}

#[doc(hidden)]
pub struct MapSerializer<'a> {
    parent: &'a mut Serializer,
    index: usize,
    keys: Vec<usize>,
    values: Vec<usize>,
}

impl<'a> MapSerializer<'a> {
    fn new(parent: &'a mut Serializer, len: Option<usize>) -> Self {
        // Reserve offset so that top object is always 0.
        let index = parent.offsets.len();
        parent.offsets.push(usize::MAX);
        let capacity = len.unwrap_or(0);
        Self {
            parent,
            index,
            keys: Vec::with_capacity(capacity),
            values: Vec::with_capacity(capacity),
        }
    }
}

impl<'a> SerializeMap for MapSerializer<'a> {
    type Ok = ();
    type Error = Infallible;

    fn serialize_key<T>(&mut self, key: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        self.keys.push(self.parent.offsets.len());
        key.serialize(&mut *self.parent)
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        self.values.push(self.parent.offsets.len());
        value.serialize(&mut *self.parent)
    }

    fn end(self) -> Result<()> {
        self.parent.offsets[self.index] = self.parent.output.len();
        self.parent.write_count(0b1101_0000_u8, self.keys.len());
        for key in self.keys.iter().copied() {
            self.parent.write_object_index(key);
        }
        for value in self.values.iter().copied() {
            self.parent.write_object_index(value);
        }
        Ok(())
    }
}

impl<'a> serde::ser::SerializeStruct for MapSerializer<'a> {
    type Ok = ();
    type Error = Infallible;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        SerializeMap::serialize_key(self, key)?;
        SerializeMap::serialize_value(self, value)
    }

    fn end(self) -> Result<()> {
        SerializeMap::end(self)
    }
}

#[doc(hidden)]
pub struct Unimplemented;

impl serde::ser::SerializeTupleVariant for Unimplemented {
    type Ok = ();
    type Error = Infallible;

    fn serialize_field<T>(&mut self, _value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        unimplemented!()
    }

    fn end(self) -> Result<()> {
        unimplemented!()
    }
}

impl serde::ser::SerializeStructVariant for Unimplemented {
    type Ok = ();
    type Error = Infallible;

    fn serialize_field<T>(&mut self, _key: &'static str, _value: &T) -> Result<()>
    where
        T: ?Sized + Serialize,
    {
        unimplemented!()
    }

    fn end(self) -> Result<()> {
        unimplemented!()
    }
}
