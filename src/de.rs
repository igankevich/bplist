use paste::paste;
use serde::Deserialize;
use serde::de::DeserializeSeed;
use serde::de::IntoDeserializer;
use serde::de::Visitor;

use alloc::borrow::Cow;
use alloc::string::String;
use core::slice::ChunksExact;

use crate::InvalidPropertyList;
use crate::Trailer;

type Result<T> = core::result::Result<T, InvalidPropertyList>;

// 2001-01-01
const CORE_DATA_EPOCH: u64 = 978303600;

#[derive(Debug, Clone, Copy)]
enum Real {
    F32(f32),
    F64(f64),
}

impl From<Real> for f32 {
    fn from(other: Real) -> Self {
        match other {
            Real::F32(x) => x,
            Real::F64(x) => x as f32,
        }
    }
}

impl From<Real> for f64 {
    fn from(other: Real) -> Self {
        match other {
            Real::F32(x) => f64::from(x),
            Real::F64(x) => x,
        }
    }
}

fn u64_from_bytes(bytes: &[u8]) -> Result<u64> {
    match bytes {
        [b0] => Ok(u64::from(*b0)),
        [b0, b1] => Ok(u64::from(u16::from_be_bytes([*b0, *b1]))),
        [b0, b1, b2, b3] => Ok(u64::from(u32::from_be_bytes([*b0, *b1, *b2, *b3]))),
        [b0, b1, b2, b3, b4, b5, b6, b7] => {
            Ok(u64::from_be_bytes([*b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7]))
        }
        _ => Err(InvalidPropertyList),
    }
}

/// Deserialize object of type `T` from the provided slice.
///
/// `max_recursion` controls how many nesting levels an object being deserialized can have.
pub fn from_slice<'a, T: Deserialize<'a>>(data: &'a [u8], max_recursion: u32) -> Result<T> {
    T::deserialize(Deserializer::from_slice(data, max_recursion)?)
}

/// Property list deserializer.
pub struct Deserializer<'a> {
    // All data.
    data: &'a [u8],
    // Particular slice that we read currently.
    slice: &'a [u8],
    trailer: Trailer,
    level: u32,
}

impl<'a> Deserializer<'a> {
    /// Create new deserializer from the provided slice.
    ///
    /// `max_recursion` controls how many nesting levels an object being deserialized can have.
    pub fn from_slice(data: &'a [u8], max_recursion: u32) -> Result<Self> {
        if !data.starts_with(b"bplist") {
            return Err(InvalidPropertyList);
        }
        let trailer_offset = data
            .len()
            .checked_sub(Trailer::LEN)
            .ok_or(InvalidPropertyList)?;
        let (data, trailer_bytes) = data
            .split_at_checked(trailer_offset)
            .ok_or(InvalidPropertyList)?;
        let trailer = Trailer::parse(
            trailer_bytes
                .try_into()
                .expect("The length of the slice matches the length of the array"),
        )?;
        let top_object = trailer.top_object as usize;
        Self::for_object(data, trailer, top_object, max_recursion)
    }

    fn for_object(
        data: &'a [u8],
        trailer: Trailer,
        object_index: usize,
        level: u32,
    ) -> Result<Self> {
        let offset_table = data
            .get(trailer.offset_table_offset as usize..)
            .ok_or(InvalidPropertyList)?;
        let start = object_index * usize::from(trailer.offset_int_size);
        let end = start + usize::from(trailer.offset_int_size);
        let b = offset_table.get(start..end).ok_or(InvalidPropertyList)?;
        let object_offset = u64_from_bytes(b)?
            .try_into()
            .map_err(|_| InvalidPropertyList)?;
        let slice = data
            .get(object_offset..trailer.offset_table_offset as usize)
            .ok_or(InvalidPropertyList)?;
        Ok(Self {
            data,
            slice,
            trailer,
            level,
        })
    }

    fn parse_marker(&mut self) -> Result<u8> {
        self.skip_padding();
        let (b, rest) = self.slice.split_at_checked(1).ok_or(InvalidPropertyList)?;
        self.slice = rest;
        Ok(b[0])
    }

    fn skip_padding(&mut self) {
        while self.slice.first().copied() == Some(0b0000_1111) {
            self.slice = &self.slice[1..];
        }
    }

    fn parse_count(&mut self, p: u8) -> Result<usize> {
        if p == 0b1111 {
            let marker = self.parse_marker()?;
            let p = match (marker >> 4, marker & 0b1111) {
                (0b0001, p) => p,
                _ => return Err(InvalidPropertyList),
            };
            self.parse_uint(1_usize << p)
        } else {
            Ok(usize::from(p))
        }
    }

    fn parse_dict(mut self, p: u8) -> Result<DictParser<'a>> {
        let len = self.parse_count(p)?;
        let elem_len = usize::from(self.trailer.object_ref_size);
        let dict_end = len
            .checked_mul(elem_len)
            .and_then(|x| x.checked_mul(2))
            .ok_or(InvalidPropertyList)?;
        let (dict_bytes, rest) = self
            .slice
            .split_at_checked(dict_end)
            .ok_or(InvalidPropertyList)?;
        self.slice = rest;
        let (keys_bytes, values_bytes) = dict_bytes.split_at(len * elem_len);
        let keys = keys_bytes.chunks_exact(elem_len);
        let values = values_bytes.chunks_exact(elem_len);
        Ok(DictParser {
            keys,
            values,
            de: self,
        })
    }

    fn parse_seq(mut self, p: u8) -> Result<SeqParser<'a>> {
        let len = self.parse_count(p)?;
        let elem_len = usize::from(self.trailer.object_ref_size);
        let end = len.checked_mul(elem_len).ok_or(InvalidPropertyList)?;
        let (bytes, rest) = self
            .slice
            .split_at_checked(end)
            .ok_or(InvalidPropertyList)?;
        self.slice = rest;
        let values = bytes.chunks_exact(elem_len);
        Ok(SeqParser { values, de: self })
    }

    #[inline]
    fn parse_int<T: TryFrom<i8> + TryFrom<i16> + TryFrom<i32> + TryFrom<i64> + TryFrom<i128>>(
        &mut self,
        num_bytes: usize,
    ) -> Result<T> {
        let (bytes, rest) = self
            .slice
            .split_at_checked(num_bytes)
            .ok_or(InvalidPropertyList)?;
        self.slice = rest;
        int_from_bytes(bytes)
    }

    #[inline]
    fn parse_uint<T: TryFrom<u8> + TryFrom<u16> + TryFrom<u32> + TryFrom<u64> + TryFrom<u128>>(
        &mut self,
        num_bytes: usize,
    ) -> Result<T> {
        let (bytes, rest) = self
            .slice
            .split_at_checked(num_bytes)
            .ok_or(InvalidPropertyList)?;
        self.slice = rest;
        uint_from_bytes(bytes)
    }

    fn visit_uint<V>(mut self, p: u8, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'a>,
    {
        let num_bytes = 1_usize << usize::from(p);
        let (bytes, rest) = self
            .slice
            .split_at_checked(num_bytes)
            .ok_or(InvalidPropertyList)?;
        self.slice = rest;
        match bytes {
            [b0] => visitor.visit_u8(*b0),
            [b0, b1] => visitor.visit_u16(u16::from_be_bytes([*b0, *b1])),
            [b0, b1, b2, b3] => visitor.visit_u32(u32::from_be_bytes([*b0, *b1, *b2, *b3])),
            [b0, b1, b2, b3, b4, b5, b6, b7] => {
                visitor.visit_u64(u64::from_be_bytes([*b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7]))
            }
            #[rustfmt::skip]
            [b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14, b15] =>
                visitor.visit_u128(
                    u128::from_be_bytes([
                        *b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7, *b8,
                        *b9, *b10, *b11, *b12, *b13, *b14, *b15
                    ])
                ),
            _ => Err(InvalidPropertyList),
        }
    }

    #[inline]
    fn parse_real(&mut self, p: u8) -> Result<Real> {
        let len = 1_usize << usize::from(p);
        let (bytes, rest) = self
            .slice
            .split_at_checked(len)
            .ok_or(InvalidPropertyList)?;
        self.slice = rest;
        match bytes {
            [b0, b1, b2, b3] => Ok(Real::F32(f32::from_be_bytes([*b0, *b1, *b2, *b3]))),
            [b0, b1, b2, b3, b4, b5, b6, b7] => Ok(Real::F64(f64::from_be_bytes([
                *b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7,
            ]))),
            _ => Err(InvalidPropertyList),
        }
    }

    #[inline]
    fn parse_date(&mut self) -> Result<u64> {
        let (b, rest) = self.slice.split_at_checked(8).ok_or(InvalidPropertyList)?;
        self.slice = rest;
        let secs = f64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        let timestamp = CORE_DATA_EPOCH
            .checked_add((secs * 1e9) as u64)
            .ok_or(InvalidPropertyList)?;
        Ok(timestamp)
    }

    #[inline]
    fn parse_bytes(&mut self, p: u8) -> Result<&'a [u8]> {
        let len = self.parse_count(p)?;
        let (bytes, rest) = self
            .slice
            .split_at_checked(len)
            .ok_or(InvalidPropertyList)?;
        self.slice = rest;
        Ok(bytes)
    }

    #[inline]
    fn parse_uuid(&mut self) -> Result<&'a [u8; 16]> {
        let (uuid, rest) = self.slice.split_at_checked(16).ok_or(InvalidPropertyList)?;
        self.slice = rest;
        uuid.try_into().map_err(|_| InvalidPropertyList)
    }

    fn parse_utf8_string(&mut self, p: u8) -> Result<&'a str> {
        let len = self.parse_count(p)?;
        let (bytes, rest) = self
            .slice
            .split_at_checked(len)
            .ok_or(InvalidPropertyList)?;
        self.slice = rest;
        core::str::from_utf8(bytes).map_err(|_| InvalidPropertyList)
    }

    fn parse_utf16_string(&mut self, p: u8) -> Result<String> {
        let bytes = self.parse_bytes(2 * p)?;
        let (chunks, remainder) = bytes.as_chunks::<2>();
        if !remainder.is_empty() {
            return Err(InvalidPropertyList);
        }
        let s = char::decode_utf16(chunks.iter().copied().map(u16::from_be_bytes))
            .collect::<core::result::Result<String, _>>()
            .map_err(|_| InvalidPropertyList)?;
        Ok(s)
    }

    fn parse_url_with_base(&mut self) -> Result<String> {
        let marker = self.parse_marker()?;
        let base: Cow<'_, str> = match (marker >> 4, marker & 0b1111_u8) {
            (0b0101, p) | (0b0111, p) => self.parse_utf8_string(p)?.into(),
            (0b0110, p) => self.parse_utf16_string(p)?.into(),
            _ => return Err(InvalidPropertyList),
        };
        let marker = self.parse_marker()?;
        let path: Cow<'_, str> = match (marker >> 4, marker & 0b1111_u8) {
            (0b0101, p) | (0b0111, p) => self.parse_utf8_string(p)?.into(),
            (0b0110, p) => self.parse_utf16_string(p)?.into(),
            _ => return Err(InvalidPropertyList),
        };
        let mut full_url = base.into_owned();
        if !full_url.ends_with('/') && !path.starts_with('/') {
            full_url.push('/');
        }
        full_url.push_str(path.as_ref());
        Ok(full_url)
    }

    fn parse_full_url(&mut self) -> Result<Cow<'a, str>> {
        let marker = self.parse_marker()?;
        let url: Cow<'_, str> = match (marker >> 4, marker & 0b1111_u8) {
            (0b0101, p) | (0b0111, p) => self.parse_utf8_string(p)?.into(),
            (0b0110, p) => self.parse_utf16_string(p)?.into(),
            _ => return Err(InvalidPropertyList),
        };
        Ok(url)
    }
}

#[inline]
fn int_from_bytes<T: TryFrom<i8> + TryFrom<i16> + TryFrom<i32> + TryFrom<i64> + TryFrom<i128>>(
    bytes: &[u8],
) -> Result<T> {
    match bytes {
        [b0] => b0.cast_signed().try_into().map_err(|_| InvalidPropertyList),
        [b0, b1] => i16::from_be_bytes([*b0, *b1])
            .try_into()
            .map_err(|_| InvalidPropertyList),
        [b0, b1, b2, b3] => i32::from_be_bytes([*b0, *b1, *b2, *b3])
            .try_into()
            .map_err(|_| InvalidPropertyList),
        [b0, b1, b2, b3, b4, b5, b6, b7] => {
            i64::from_be_bytes([*b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7])
                .try_into()
                .map_err(|_| InvalidPropertyList)
        }
        #[rustfmt::skip]
        [b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14, b15] =>
            i128::from_be_bytes([
                *b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7, *b8, *b9, *b10, *b11, *b12, *b13, *b14, *b15
            ])
                .try_into()
                .map_err(|_| InvalidPropertyList),
        _ => Err(InvalidPropertyList),
    }
}

#[inline]
fn uint_from_bytes<T: TryFrom<u8> + TryFrom<u16> + TryFrom<u32> + TryFrom<u64> + TryFrom<u128>>(
    bytes: &[u8],
) -> Result<T> {
    match bytes {
        [b0] => (*b0).try_into().map_err(|_| InvalidPropertyList),
        [b0, b1] => u16::from_be_bytes([*b0, *b1])
            .try_into()
            .map_err(|_| InvalidPropertyList),
        [b0, b1, b2, b3] => u32::from_be_bytes([*b0, *b1, *b2, *b3])
            .try_into()
            .map_err(|_| InvalidPropertyList),
        [b0, b1, b2, b3, b4, b5, b6, b7] => {
            u64::from_be_bytes([*b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7])
                .try_into()
                .map_err(|_| InvalidPropertyList)
        }
        #[rustfmt::skip]
        [b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11, b12, b13, b14, b15] =>
            u128::from_be_bytes([
                *b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7, *b8, *b9, *b10, *b11, *b12, *b13, *b14, *b15
            ])
                .try_into()
                .map_err(|_| InvalidPropertyList),
        _ => Err(InvalidPropertyList),
    }
}

impl<'de> serde::Deserializer<'de> for Deserializer<'de> {
    type Error = InvalidPropertyList;

    fn deserialize_map<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.parse_marker()?;
        match (marker >> 4, marker & 0b1111_u8) {
            (0b1101, p) => visitor.visit_map(self.parse_dict(p)?),
            _ => Err(InvalidPropertyList),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_newtype_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.parse_marker()?;
        match (marker >> 4, marker & 0b1111_u8) {
            (0b1010..=0b1100, p) => Ok(visitor.visit_seq(self.parse_seq(p)?)?),
            _ => Err(InvalidPropertyList),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_unit<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.parse_marker()? {
            0b0000_0000 => Ok(visitor.visit_unit()?),
            _ => Err(InvalidPropertyList),
        }
    }

    fn deserialize_unit_struct<V>(self, _name: &'static str, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_enum<V>(
        mut self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.parse_marker()?;
        let cow_str: Cow<'_, str> = match (marker >> 4, marker & 0b1111_u8) {
            (0b0100, p) | (0b0101, p) | (0b0111, p) => self.parse_utf8_string(p)?.into(),
            (0b0110, p) => self.parse_utf16_string(p)?.into(),
            _ => return Err(InvalidPropertyList),
        };
        visitor.visit_enum(cow_str.into_deserializer())
    }

    fn deserialize_bool<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.parse_marker()?;
        match marker {
            0b0000_1000 => Ok(visitor.visit_bool(false)?),
            0b0000_1001 => Ok(visitor.visit_bool(true)?),
            _ => Err(InvalidPropertyList),
        }
    }

    deserialize_integer! {
        // Signed.
        (i8 parse_int)
        (i16 parse_int)
        (i32 parse_int)
        (i64 parse_int)
        (i128 parse_int)
        // Unsigned.
        (u8 parse_uint)
        (u16 parse_uint)
        (u32 parse_uint)
        (u64 parse_uint)
        (u128 parse_uint)
    }

    fn deserialize_char<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.parse_marker()?;
        let len = match (marker >> 4, marker & 0b1111_u8) {
            (0b0001, p) => 1_usize << p,
            (0b1000, p) => usize::from(p + 1),
            _ => return Err(InvalidPropertyList),
        };
        let ch = char::try_from(self.parse_uint::<u32>(len)?).map_err(|_| InvalidPropertyList)?;
        visitor.visit_char(ch)
    }

    fn deserialize_f32<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.parse_marker()?;
        match (marker >> 4, marker & 0b1111_u8) {
            (0b0010, p) => Ok(visitor.visit_f32(self.parse_real(p)?.into())?),
            _ => Err(InvalidPropertyList),
        }
    }

    fn deserialize_f64<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.parse_marker()?;
        match (marker >> 4, marker & 0b1111_u8) {
            (0b0010, p) => Ok(visitor.visit_f64(self.parse_real(p)?.into())?),
            _ => Err(InvalidPropertyList),
        }
    }

    fn deserialize_str<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.parse_marker()?;
        let cow_str: Cow<'_, str> = match (marker >> 4, marker & 0b1111_u8) {
            (0b0100, p) | (0b0101, p) | (0b0111, p) => self.parse_utf8_string(p)?.into(),
            (0b0110, p) => self.parse_utf16_string(p)?.into(),
            (0b0000, 0b1100) => self.parse_url_with_base()?.into(),
            (0b0000, 0b1101) => self.parse_full_url()?,
            _ => return Err(InvalidPropertyList),
        };
        match cow_str {
            Cow::Borrowed(s) => visitor.visit_borrowed_str(s),
            #[cfg(feature = "std")]
            Cow::Owned(s) => visitor.visit_string(s),
            #[cfg(not(feature = "std"))]
            Cow::Owned(s) => visitor.visit_str(s.as_str()),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.parse_marker()?;
        let bytes = match (marker >> 4, marker & 0b1111_u8) {
            (0b0000, 0b1110) => self.parse_uuid()?.as_slice(),
            (0b0100, p) | (0b0101, p) | (0b0111, p) | (0b0110, p) => self.parse_bytes(p)?,
            _ => return Err(InvalidPropertyList),
        };
        visitor.visit_borrowed_bytes(bytes)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        // TODO
        self.deserialize_any(visitor)
    }

    fn deserialize_any<V>(mut self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.skip_padding();
        let (b, rest) = self.slice.split_at_checked(1).ok_or(InvalidPropertyList)?;
        let marker = b[0];
        match marker {
            0b0000_0000 => self.deserialize_option(visitor),
            0b0000_1000 | 0b0000_1001 => self.deserialize_bool(visitor),
            0b0000_1100 | 0b0000_1101 => self.deserialize_str(visitor),
            0b0000_1110 => self.deserialize_bytes(visitor),
            0b0011_0011 => self.deserialize_u64(visitor),
            _ => match (marker >> 4, marker & 0b1111_u8) {
                (0b0001, p) => self.visit_uint(p, visitor),
                (0b1000, p) => self.visit_uint(p + 1, visitor),
                (0b0010, p) => {
                    self.slice = rest;
                    match self.parse_real(p)? {
                        Real::F32(x) => visitor.visit_f32(x),
                        Real::F64(x) => visitor.visit_f64(x),
                    }
                }
                (0b0100, _) => self.deserialize_bytes(visitor),
                (0b0101, _) | (0b0111, _) | (0b0110, _) => self.deserialize_str(visitor),
                (0b1010..=0b1100, _) => self.deserialize_seq(visitor),
                (0b1101, _) => self.deserialize_map(visitor),
                _ => Err(InvalidPropertyList),
            },
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        let marker = self.slice.first().copied().ok_or(InvalidPropertyList)?;
        match marker {
            0b0000_0000 => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }
}

macro_rules! deserialize_integer {
    ($(($int: ident $func: ident))+) => {
        paste! {
            $(
                fn [<deserialize_ $int>]<V>(mut self, visitor: V) -> Result<V::Value>
                where
                    V: Visitor<'de>,
                {
                    let marker = self.parse_marker()?;
                    let len = match (marker >> 4, marker & 0b1111_u8) {
                        (0b0001, p) => 1_usize << p,
                        (0b1000, p) => usize::from(p + 1),
                        (0b0011, 0b0011) => {
                            let value = self.parse_date()?
                                .try_into().map_err(|_| InvalidPropertyList)?;
                            return visitor.[<visit_ $int>](value);
                        }
                        _ => return Err(InvalidPropertyList),
                    };
                    visitor.[<visit_ $int>](self.$func(len)?)
                }
            )+
        }
    };
}

use deserialize_integer;

struct DictParser<'a> {
    keys: ChunksExact<'a, u8>,
    values: ChunksExact<'a, u8>,
    de: Deserializer<'a>,
}

impl<'de> serde::de::MapAccess<'de> for DictParser<'de> {
    type Error = InvalidPropertyList;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>>
    where
        K: DeserializeSeed<'de>,
    {
        let Some(key_bytes) = self.keys.next() else {
            return Ok(None);
        };
        let keyref = u64_from_bytes(key_bytes)?;
        if keyref >= self.de.trailer.num_objects {
            return Err(InvalidPropertyList);
        }
        let level = self.de.level.checked_sub(1).ok_or(InvalidPropertyList)?;
        let de = Deserializer::for_object(
            self.de.data,
            self.de.trailer.clone(),
            keyref as usize,
            level,
        )?;
        seed.deserialize(de).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value>
    where
        V: DeserializeSeed<'de>,
    {
        let value_bytes = self.values.next().ok_or(InvalidPropertyList)?;
        let objref = u64_from_bytes(value_bytes)?;
        if objref >= self.de.trailer.num_objects {
            return Err(InvalidPropertyList);
        }
        let level = self.de.level.checked_sub(1).ok_or(InvalidPropertyList)?;
        let de = Deserializer::for_object(
            self.de.data,
            self.de.trailer.clone(),
            objref as usize,
            level,
        )?;
        seed.deserialize(de)
    }
}

struct SeqParser<'a> {
    values: ChunksExact<'a, u8>,
    de: Deserializer<'a>,
}

impl<'de> serde::de::SeqAccess<'de> for SeqParser<'de> {
    type Error = InvalidPropertyList;

    fn next_element_seed<V>(&mut self, seed: V) -> Result<Option<V::Value>>
    where
        V: DeserializeSeed<'de>,
    {
        let Some(value_bytes) = self.values.next() else {
            return Ok(None);
        };
        let objref = u64_from_bytes(value_bytes)?;
        if objref >= self.de.trailer.num_objects {
            return Err(InvalidPropertyList);
        }
        let level = self.de.level.checked_sub(1).ok_or(InvalidPropertyList)?;
        let de = Deserializer::for_object(
            self.de.data,
            self.de.trailer.clone(),
            objref as usize,
            level,
        )?;
        seed.deserialize(de).map(Some)
    }
}
