use std::collections::VecDeque;
use std::fmt;
use std::mem;
use std::pin::Pin;

use destream::en;
use futures::future;
use futures::stream::{Stream, StreamExt};

use super::constants::*;

pub type ByteStream<'en> = Pin<Box<dyn Stream<Item = Result<Vec<u8>, Error>> + Send + Unpin + 'en>>;

pub struct Error {
    message: String,
}

impl en::Error for Error {
    fn custom<I: fmt::Display>(info: I) -> Self {
        Self {
            message: info.to_string(),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.message)
    }
}

pub struct MapEncoder<'en> {
    pending_key: Option<ByteStream<'en>>,
    entries: VecDeque<(ByteStream<'en>, ByteStream<'en>)>,
}

impl<'en> MapEncoder<'en> {
    #[inline]
    fn new(size_hint: Option<usize>) -> Self {
        let entries = if let Some(len) = size_hint {
            VecDeque::with_capacity(len)
        } else {
            VecDeque::new()
        };

        Self {
            pending_key: None,
            entries,
        }
    }
}

impl<'en> en::EncodeMap<'en> for MapEncoder<'en> {
    type Ok = ByteStream<'en>;
    type Error = Error;

    #[inline]
    fn encode_key<T: en::IntoStream<'en> + 'en>(&mut self, key: T) -> Result<(), Self::Error> {
        if self.pending_key.is_none() {
            self.pending_key = Some(key.into_stream(Encoder)?);
            Ok(())
        } else {
            Err(en::Error::custom(
                "You must call encode_value before calling encode_key again",
            ))
        }
    }

    #[inline]
    fn encode_value<T: en::IntoStream<'en> + 'en>(&mut self, value: T) -> Result<(), Self::Error> {
        if self.pending_key.is_none() {
            return Err(en::Error::custom(
                "You must call encode_key before encode_value",
            ));
        }

        let value = value.into_stream(Encoder)?;

        let mut key = None;
        mem::swap(&mut self.pending_key, &mut key);

        self.entries.push_back((key.unwrap(), value));
        Ok(())
    }

    fn end(mut self) -> Result<Self::Ok, Self::Error> {
        if self.pending_key.is_some() {
            return Err(en::Error::custom(
                "You must call encode_value after calling encode_key",
            ));
        }

        let mut encoded = delimiter(MAP_BEGIN);

        while let Some((key, value)) = self.entries.pop_front() {
            encoded = Box::pin(encoded.chain(key).chain(delimiter(COLON)).chain(value));

            if !self.entries.is_empty() {
                encoded = Box::pin(encoded.chain(delimiter(COMMA)));
            }
        }

        encoded = Box::pin(encoded.chain(delimiter(MAP_END)));
        Ok(encoded)
    }
}

pub struct SequenceEncoder<'en> {
    items: VecDeque<ByteStream<'en>>,
}

impl<'en> SequenceEncoder<'en> {
    #[inline]
    fn new(size_hint: Option<usize>) -> Self {
        let items = if let Some(len) = size_hint {
            VecDeque::with_capacity(len)
        } else {
            VecDeque::new()
        };

        Self { items }
    }

    #[inline]
    fn push(&mut self, value: ByteStream<'en>) {
        self.items.push_back(value);
    }

    fn encode(mut self) -> Result<ByteStream<'en>, Error> {
        let mut encoded = delimiter(LIST_BEGIN);

        while let Some(item) = self.items.pop_front() {
            encoded = Box::pin(encoded.chain(item));

            if !self.items.is_empty() {
                encoded = Box::pin(encoded.chain(delimiter(COMMA)));
            }
        }

        encoded = Box::pin(encoded.chain(delimiter(LIST_END)));
        Ok(encoded)
    }
}

impl<'en> en::EncodeSeq<'en> for SequenceEncoder<'en> {
    type Ok = ByteStream<'en>;
    type Error = Error;

    #[inline]
    fn encode_element<T: en::IntoStream<'en> + 'en>(
        &mut self,
        value: T,
    ) -> Result<(), Self::Error> {
        let encoded = value.into_stream(Encoder)?;
        self.push(encoded);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.encode()
    }
}

impl<'en> en::EncodeTuple<'en> for SequenceEncoder<'en> {
    type Ok = ByteStream<'en>;
    type Error = Error;

    #[inline]
    fn encode_element<T: en::IntoStream<'en> + 'en>(
        &mut self,
        value: T,
    ) -> Result<(), Self::Error> {
        let encoded = value.into_stream(Encoder)?;
        self.push(encoded);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.encode()
    }
}

pub struct Encoder;

impl<'en> en::Encoder<'en> for Encoder {
    type Ok = ByteStream<'en>;
    type Error = Error;
    type EncodeMap = MapEncoder<'en>;
    type EncodeSeq = SequenceEncoder<'en>;
    type EncodeTuple = SequenceEncoder<'en>;

    fn encode_bool(self, _v: bool) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_i8(self, _v: i8) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_i16(self, _v: i16) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_i32(self, _v: i32) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_i64(self, _v: i64) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_u8(self, _v: u8) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_u16(self, _v: u16) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_u32(self, _v: u32) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_u64(self, _v: u64) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_str(self, _v: &str) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_none(self) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_some<T: en::IntoStream<'en> + 'en>(self, _value: T) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn encode_unit(self) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    #[inline]
    fn encode_map(self, size_hint: Option<usize>) -> Result<Self::EncodeMap, Self::Error> {
        Ok(MapEncoder::new(size_hint))
    }

    #[inline]
    fn encode_map_stream<
        K: en::IntoStream<'en> + 'en,
        V: en::IntoStream<'en> + 'en,
        S: Stream<Item = Result<(K, V), Self::Error>> + Send + Unpin + 'en,
    >(
        self,
        _map: S,
    ) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    #[inline]
    fn encode_seq(self, size_hint: Option<usize>) -> Result<Self::EncodeSeq, Self::Error> {
        Ok(SequenceEncoder::new(size_hint))
    }

    #[inline]
    fn encode_seq_stream<
        T: en::IntoStream<'en> + 'en,
        S: Stream<Item = Result<T, Self::Error>> + Send + Unpin + 'en,
    >(
        self,
        _seq: S,
    ) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    #[inline]
    fn encode_tuple(self, len: usize) -> Result<Self::EncodeTuple, Self::Error> {
        Ok(SequenceEncoder::new(Some(len)))
    }
}

#[inline]
fn delimiter<'en>(byte: u8) -> ByteStream<'en> {
    let encoded = futures::stream::once(future::ready(Ok(vec![byte])));
    Box::pin(encoded)
}
