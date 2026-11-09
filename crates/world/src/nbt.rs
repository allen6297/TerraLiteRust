//! Big-endian NBT, the subset stored in Java-edition chunk files.
//!
//! Tag names in 1.8 chunks are ASCII. Strings that are not UTF-8 are rejected.

use crate::error::Error;

const MAX_DEPTH: usize = 32;
const MAX_COLLECTION: i32 = 1_000_000;
const MAX_BLOB: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    pub(crate) fn get<'a>(&'a self, key: &str) -> Option<&'a Tag> {
        let Tag::Compound(entries) = self else {
            return None;
        };
        entries
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, tag)| tag)
    }

    pub(crate) fn as_i8(&self) -> Option<i8> {
        match self {
            Tag::Byte(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_int(&self) -> Option<i32> {
        match self {
            Tag::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Tag::ByteArray(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_list(&self) -> Option<&[Tag]> {
        match self {
            Tag::List(value) => Some(value),
            _ => None,
        }
    }
}

pub(crate) fn read_root(data: &[u8]) -> Result<(String, Tag), Error> {
    let mut cursor = Cursor::new(data);
    let ty = cursor.u8()?;
    if ty != 10 {
        return Err(Error::Nbt("root is not a compound"));
    }
    let name = cursor.string()?;
    let entries = cursor.compound(1)?;
    Ok((name, Tag::Compound(entries)))
}

pub(crate) fn write_root(name: &str, tag: &Tag) -> Vec<u8> {
    let Tag::Compound(entries) = tag else {
        panic!("NBT root must be a compound");
    };
    let mut out = Vec::new();
    out.push(10);
    write_string(&mut out, name);
    write_compound_body(&mut out, entries);
    out
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn u8(&mut self) -> Result<u8, Error> {
        let byte = *self.data.get(self.pos).ok_or(Error::Nbt("truncated"))?;
        self.pos += 1;
        Ok(byte)
    }

    fn i8(&mut self) -> Result<i8, Error> {
        Ok(self.u8()? as i8)
    }

    fn bytes(&mut self, len: usize) -> Result<&'a [u8], Error> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or(Error::Nbt("length overflow"))?;
        if end > self.data.len() {
            return Err(Error::Nbt("truncated"));
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let bytes = self.bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    fn i16(&mut self) -> Result<i16, Error> {
        Ok(self.u16()? as i16)
    }

    fn i32(&mut self) -> Result<i32, Error> {
        let bytes = self.bytes(4)?;
        Ok(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn i64(&mut self) -> Result<i64, Error> {
        let bytes = self.bytes(8)?;
        Ok(i64::from_be_bytes(bytes.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_be_bytes(self.i32()?.to_be_bytes()))
    }

    fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_be_bytes(self.i64()?.to_be_bytes()))
    }

    fn string(&mut self) -> Result<String, Error> {
        let len = self.u16()? as usize;
        let bytes = self.bytes(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| Error::Nbt("string is not utf-8"))
    }

    fn compound(&mut self, depth: usize) -> Result<Vec<(String, Tag)>, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Nbt("nesting is too deep"));
        }
        let mut entries = Vec::new();
        loop {
            let ty = self.u8()?;
            if ty == 0 {
                break;
            }
            let name = self.string()?;
            entries.push((name, self.payload(ty, depth)?));
        }
        Ok(entries)
    }

    fn payload(&mut self, ty: u8, depth: usize) -> Result<Tag, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Nbt("nesting is too deep"));
        }
        Ok(match ty {
            1 => Tag::Byte(self.i8()?),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(self.f32()?),
            6 => Tag::Double(self.f64()?),
            7 => Tag::ByteArray(self.byte_array()?),
            8 => Tag::String(self.string()?),
            9 => self.list(depth)?,
            10 => Tag::Compound(self.compound(depth + 1)?),
            11 => Tag::IntArray(self.int_array()?),
            12 => Tag::LongArray(self.long_array()?),
            _ => return Err(Error::Nbt("unknown tag type")),
        })
    }

    fn byte_array(&mut self) -> Result<Vec<u8>, Error> {
        let len = self.collection_len()?;
        if len > MAX_BLOB {
            return Err(Error::Nbt("byte array is too large"));
        }
        Ok(self.bytes(len)?.to_vec())
    }

    fn int_array(&mut self) -> Result<Vec<i32>, Error> {
        let len = self.collection_len()?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(self.i32()?);
        }
        Ok(values)
    }

    fn long_array(&mut self) -> Result<Vec<i64>, Error> {
        let len = self.collection_len()?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(self.i64()?);
        }
        Ok(values)
    }

    fn list(&mut self, depth: usize) -> Result<Tag, Error> {
        let elem = self.u8()?;
        let len = self.i32()?;
        if len < 0 || len > MAX_COLLECTION {
            return Err(Error::Nbt("list length"));
        }
        if elem == 0 && len > 0 {
            return Err(Error::Nbt("non-empty list of end tags"));
        }
        let mut items = Vec::with_capacity(len as usize);
        for _ in 0..len {
            items.push(self.payload(elem, depth + 1)?);
        }
        Ok(Tag::List(items))
    }

    fn collection_len(&mut self) -> Result<usize, Error> {
        let len = self.i32()?;
        if len < 0 || len > MAX_COLLECTION {
            return Err(Error::Nbt("array length"));
        }
        Ok(len as usize)
    }
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    assert!(bytes.len() <= u16::MAX as usize, "NBT string is too long");
    out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn write_compound_body(out: &mut Vec<u8>, entries: &[(String, Tag)]) {
    for (name, tag) in entries {
        out.push(tag_type(tag));
        write_string(out, name);
        write_payload(out, tag);
    }
    out.push(0);
}

fn write_payload(out: &mut Vec<u8>, tag: &Tag) {
    match tag {
        Tag::Byte(value) => out.push(*value as u8),
        Tag::Short(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::Int(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::Long(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::Float(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::Double(value) => out.extend_from_slice(&value.to_be_bytes()),
        Tag::ByteArray(value) => write_byte_array(out, value),
        Tag::String(value) => write_string(out, value),
        Tag::List(items) => {
            let elem = items.first().map(tag_type).unwrap_or(0);
            out.push(elem);
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for item in items {
                assert_eq!(tag_type(item), elem, "NBT list tags must share a type");
                write_payload(out, item);
            }
        }
        Tag::Compound(entries) => write_compound_body(out, entries),
        Tag::IntArray(values) => {
            out.extend_from_slice(&(values.len() as i32).to_be_bytes());
            for value in values {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        Tag::LongArray(values) => {
            out.extend_from_slice(&(values.len() as i32).to_be_bytes());
            for value in values {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
}

fn write_byte_array(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as i32).to_be_bytes());
    out.extend_from_slice(value);
}

fn tag_type(tag: &Tag) -> u8 {
    match tag {
        Tag::Byte(_) => 1,
        Tag::Short(_) => 2,
        Tag::Int(_) => 3,
        Tag::Long(_) => 4,
        Tag::Float(_) => 5,
        Tag::Double(_) => 6,
        Tag::ByteArray(_) => 7,
        Tag::String(_) => 8,
        Tag::List(_) => 9,
        Tag::Compound(_) => 10,
        Tag::IntArray(_) => 11,
        Tag::LongArray(_) => 12,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_named_int() {
        // Unnamed root compound holding a single int named "x" with value 1.
        let tag = Tag::Compound(vec![("x".into(), Tag::Int(1))]);
        let bytes = write_root("", &tag);
        assert_eq!(
            bytes,
            [
                0x0A, 0x00, 0x00, 0x03, 0x00, 0x01, b'x', 0x00, 0x00, 0x00, 0x01, 0x00,
            ]
        );
        let (name, read) = read_root(&bytes).unwrap();
        assert_eq!(name, "");
        assert_eq!(read, tag);
    }

    #[test]
    fn roundtrip_nested_tags() {
        let tag = Tag::Compound(vec![
            ("B".into(), Tag::ByteArray(vec![1, 2, 255])),
            ("L".into(), Tag::List(vec![Tag::Int(3), Tag::Int(-1)])),
            ("S".into(), Tag::String("hi".into())),
        ]);
        let bytes = write_root("root", &tag);
        let (name, read) = read_root(&bytes).unwrap();
        assert_eq!(name, "root");
        assert_eq!(read, tag);
    }

    #[test]
    fn truncated_root_is_an_error() {
        assert!(read_root(&[0x0A, 0x00]).is_err());
    }
}
