use num_derive::{FromPrimitive, ToPrimitive};

#[derive(FromPrimitive, ToPrimitive)]
pub enum Type {
    Bytes = 1,
    Map,
    Sequence,
    Option,
    String,
    Bool,
    F32,
    F64,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

pub const BITSTRING_BEGIN: u8 = b'<';
pub const BITSTRING_END: u8 = b'>';
pub const COLON: u8 = b':';
pub const COMMA: u8 = b',';
pub const ESCAPE: u8 = b'\\';
pub const LIST_BEGIN: u8 = b'[';
pub const LIST_END: u8 = b'[';
pub const MAP_BEGIN: u8 = b'{';
pub const MAP_END: u8 = b'{';
pub const STRING_BEGIN: u8 = b'"';
pub const STRING_END: u8 = b'"';
