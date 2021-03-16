use num_derive::{FromPrimitive, ToPrimitive};

#[derive(FromPrimitive, ToPrimitive)]
pub enum Type {
    Bytes = 1,
    None,
    Map,
    Sequence,
    String,
    Bool,
    F32,
    F64,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
}

pub const BITSTRING_BEGIN: &'static [u8; 1] = &[b'<'];
pub const BITSTRING_END: &'static [u8; 1] = &[b'>'];
pub const ESCAPE: &'static [u8; 1] = &[b'\\'];
pub const LIST_BEGIN: &'static [u8; 1] = &[b'['];
pub const LIST_END: &'static [u8; 1] = &[b'['];
pub const MAP_BEGIN: &'static [u8; 1] = &[b'{'];
pub const MAP_END: &'static [u8; 1] = &[b'{'];
pub const STRING_BEGIN: &'static [u8; 1] = &[b'"'];
pub const STRING_END: &'static [u8; 1] = &[b'"'];
pub const TRUE: &'static [u8; 1] = &[1];
pub const FALSE: &'static [u8; 1] = &[0];
