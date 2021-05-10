use std::convert::TryInto;

use destream::de;

use super::constants::Type;

pub trait Element: Sized {
    const SIZE: usize;

    fn dtype() -> Type;

    fn from_bytes(bytes: &[u8]) -> Self;

    // TODO: use const generic Self::SIZE to return an array
    // fn to_bytes(&self) -> [u8; Self::SIZE];

    fn parse<E: de::Error>(bytes: &[u8]) -> Result<Self, E> {
        if bytes.len() == Self::SIZE {
            Ok(Self::from_bytes(bytes))
        } else {
            Err(de::Error::invalid_length(bytes.len(), Self::SIZE))
        }
    }
}

pub trait IntoBytes<const SIZE: usize>: Sized {
    fn into_bytes(self) -> [u8; SIZE];
}

impl Element for bool {
    const SIZE: usize = 1;

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

    fn parse<E: de::Error>(bytes: &[u8]) -> Result<Self, E> {
        if bytes.len() == Self::SIZE {
            if bytes[0] == 0 || bytes[0] == 1 {
                Ok(Self::from_bytes(bytes))
            } else {
                Err(de::Error::invalid_value(bytes[0], "1 or 0 (true or false)"))
            }
        } else {
            Err(de::Error::invalid_length(bytes.len(), Self::SIZE))
        }
    }
}

impl IntoBytes<1> for bool {
    fn into_bytes(self) -> [u8; 1] {
        if self {
            [1]
        } else {
            [0]
        }
    }
}

impl Element for u8 {
    const SIZE: usize = 1;

    fn dtype() -> Type {
        Type::U8
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        bytes[0]
    }
}

impl Element for u16 {
    const SIZE: usize = 2;

    fn dtype() -> Type {
        Type::U16
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for u32 {
    const SIZE: usize = 4;

    fn dtype() -> Type {
        Type::U32
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for u64 {
    const SIZE: usize = 8;

    fn dtype() -> Type {
        Type::U64
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for i8 {
    const SIZE: usize = 1;

    fn dtype() -> Type {
        Type::I8
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for i16 {
    const SIZE: usize = 2;

    fn dtype() -> Type {
        Type::I16
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for i32 {
    const SIZE: usize = 4;

    fn dtype() -> Type {
        Type::I32
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for i64 {
    const SIZE: usize = 8;

    fn dtype() -> Type {
        Type::I64
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for f32 {
    const SIZE: usize = 4;

    fn dtype() -> Type {
        Type::F32
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}

impl Element for f64 {
    const SIZE: usize = 8;

    fn dtype() -> Type {
        Type::F64
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self::from_be_bytes(bytes.try_into().unwrap())
    }
}
