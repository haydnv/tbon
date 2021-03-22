use std::convert::TryInto;

use destream::de;

use super::Type;

pub trait Element: Sized {
    fn size() -> usize {
        std::mem::size_of::<Self>()
    }

    fn dtype() -> Type;

    fn from_bytes(bytes: &[u8]) -> Self;

    fn parse(bytes: &[u8]) -> Result<Self, super::Error> {
        if bytes.len() == Self::size() {
            Ok(Self::from_bytes(bytes))
        } else {
            Err(de::Error::invalid_length(bytes.len(), Self::size()))
        }
    }
}

impl Element for bool {
    fn dtype() -> Type {
        Type::Bool
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        if bytes[0] == 1 {
            true
        } else if bytes[0] == 0 {
            false
        } else {
            panic!("invalid boolean: {}", bytes[0])
        }
    }

    fn parse(bytes: &[u8]) -> Result<Self, super::Error> {
        if bytes.len() == Self::size() {
            if bytes[0] == 0 || bytes[0] == 1 {
                Ok(Self::from_bytes(bytes))
            } else {
                Err(de::Error::invalid_value(bytes[0], "1 or 0 (true or false)"))
            }
        } else {
            Err(de::Error::invalid_length(bytes.len(), Self::size()))
        }
    }
}

impl Element for u8 {
    fn dtype() -> Type {
        Type::U8
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        bytes[0]
    }
}

impl Element for u16 {
    fn dtype() -> Type {
        Type::U16
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for u32 {
    fn dtype() -> Type {
        Type::U32
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for u64 {
    fn dtype() -> Type {
        Type::U64
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for i8 {
    fn dtype() -> Type {
        Type::I8
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for i16 {
    fn dtype() -> Type {
        Type::I16
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for i32 {
    fn dtype() -> Type {
        Type::I32
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for i64 {
    fn dtype() -> Type {
        Type::I64
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for f32 {
    fn dtype() -> Type {
        Type::F32
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for f64 {
    fn dtype() -> Type {
        Type::F64
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}
