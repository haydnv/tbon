use std::fmt;

use async_trait::async_trait;
use bytes::{BufMut, Bytes, BytesMut};
use destream::{de, FromStream, Visitor};
use futures::stream::{Fuse, FusedStream, Stream, StreamExt, TryStreamExt};
use num_traits::{FromPrimitive, ToPrimitive};

#[cfg(tokio)]
use tokio_io::io::{AsyncRead, AsyncReadExt, BufReader};

use crate::constants::*;

mod element;

use element::Element;

#[async_trait]
pub trait Read: Send + Unpin {
    async fn next(&mut self) -> Option<Result<Bytes, Error>>;

    fn is_terminated(&self) -> bool;
}

pub struct SourceStream<S> {
    source: Fuse<S>,
}

#[async_trait]
impl<S: Stream<Item = Result<Bytes, Error>> + Send + Unpin> Read for SourceStream<S> {
    async fn next(&mut self) -> Option<Result<Bytes, Error>> {
        self.source.next().await
    }

    fn is_terminated(&self) -> bool {
        self.source.is_terminated()
    }
}

impl<S: Stream> From<S> for SourceStream<S> {
    fn from(source: S) -> Self {
        Self {
            source: source.fuse(),
        }
    }
}

#[cfg(tokio)]
pub struct SourceReader<R: AsyncRead> {
    reader: BufReader<R>,
    terminated: bool,
}

#[async_trait]
#[cfg(tokio)]
impl<R: AsyncRead + Send + Unpin> Read for SourceReader<R> {
    async fn next(&mut self) -> Option<Result<Bytes, Error>> {
        let mut chunk = Vec::new();
        match self.reader.read_buf(&mut chunk).await {
            Ok(0) => {
                self.terminated = true;
                Some(Ok(Bytes::from(chunk)))
            }
            Ok(size) => {
                debug_assert_eq!(chunk.len(), size);
                Some(Ok(Bytes::from(chunk)))
            }
            Err(cause) => Some(Err(de::Error::custom(format!("io error: {}", cause)))),
        }
    }

    fn is_terminated(&self) -> bool {
        self.terminated
    }
}

#[cfg(tokio)]
impl<R: AsyncRead> From<R> for SourceReader<R> {
    fn from(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            terminated: false,
        }
    }
}

/// An error encountered while decoding a JSON stream.
pub struct Error {
    message: String,
}

impl Error {
    fn invalid_utf8<I: fmt::Display>(info: I) -> Self {
        de::Error::custom(format!("invalid UTF-8: {}", info))
    }

    fn unexpected_end() -> Self {
        de::Error::custom("unexpected end of stream")
    }
}

impl std::error::Error for Error {}

impl de::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self {
            message: msg.to_string(),
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
        fmt::Display::fmt(&self.message, f)
    }
}

struct MapAccess<'a, S> {
    decoder: &'a mut Decoder<S>,
    size_hint: Option<usize>,
    done: bool,
}

impl<'a, S: Read + 'a> MapAccess<'a, S> {
    async fn new(
        decoder: &'a mut Decoder<S>,
        size_hint: Option<usize>,
    ) -> Result<MapAccess<'a, S>, Error> {
        decoder.expect_delimiter(MAP_BEGIN).await?;

        let done = decoder.maybe_delimiter(MAP_END).await?;

        Ok(MapAccess {
            decoder,
            size_hint,
            done,
        })
    }
}

#[async_trait]
impl<'a, S: Read + 'a> de::MapAccess for MapAccess<'a, S> {
    type Error = Error;

    async fn next_key<K: FromStream>(&mut self, context: K::Context) -> Result<Option<K>, Error> {
        if self.done {
            return Ok(None);
        }

        let key = K::from_stream(context, self.decoder).await?;

        Ok(Some(key))
    }

    async fn next_value<V: FromStream>(&mut self, context: V::Context) -> Result<V, Error> {
        let value = V::from_stream(context, self.decoder).await?;

        if self.decoder.maybe_delimiter(MAP_END).await? {
            self.done = true;
        }

        Ok(value)
    }

    fn size_hint(&self) -> Option<usize> {
        self.size_hint
    }
}

struct SeqAccess<'a, S> {
    decoder: &'a mut Decoder<S>,
    size_hint: Option<usize>,
    done: bool,
}

impl<'a, S: Read + 'a> SeqAccess<'a, S> {
    async fn new(
        decoder: &'a mut Decoder<S>,
        size_hint: Option<usize>,
    ) -> Result<SeqAccess<'a, S>, Error> {
        decoder.expect_delimiter(LIST_BEGIN).await?;

        let done = decoder.maybe_delimiter(LIST_END).await?;

        Ok(SeqAccess {
            decoder,
            size_hint,
            done,
        })
    }
}

#[async_trait]
impl<'a, S: Read + 'a> de::SeqAccess for SeqAccess<'a, S> {
    type Error = Error;

    async fn next_element<T: FromStream>(
        &mut self,
        context: T::Context,
    ) -> Result<Option<T>, Self::Error> {
        if self.done {
            return Ok(None);
        }

        let value = T::from_stream(context, self.decoder).await?;

        if self.decoder.maybe_delimiter(LIST_END).await? {
            self.done = true;
        }

        Ok(Some(value))
    }

    fn size_hint(&self) -> Option<usize> {
        self.size_hint
    }
}

/// A structure that decodes Rust values from a JSON stream.
pub struct Decoder<R> {
    source: R,
    buffer: Vec<u8>,
}

#[cfg(tokio)]
impl<A: AsyncRead> Decoder<A>
where
    SourceReader<A>: Read,
{
    pub fn from_reader(reader: A) -> Decoder<SourceReader<A>> {
        Decoder {
            source: SourceReader::from(reader),
            buffer: Vec::new(),
        }
    }
}

impl<S: Stream> Decoder<SourceStream<S>>
where
    SourceStream<S>: Read,
{
    pub fn from_stream(stream: S) -> Decoder<SourceStream<S>> {
        Decoder {
            source: SourceStream::from(stream),
            buffer: Vec::new(),
        }
    }
}

impl<R: Read> Decoder<R> {
    async fn buffer(&mut self) -> Result<(), Error> {
        if let Some(data) = self.source.next().await {
            self.buffer.extend(data?);
        }

        Ok(())
    }

    async fn buffer_string(
        &mut self,
        begin: &'static [u8],
        end: &'static [u8],
    ) -> Result<Bytes, Error> {
        self.expect_delimiter(begin).await?;

        let mut i = 0;
        loop {
            while i >= self.buffer.len() && !self.source.is_terminated() {
                self.buffer().await?;
            }

            if i < self.buffer.len()
                && &self.buffer[i..i + 1] == end
                && (i == 0 || &self.buffer[i - 1..i] != ESCAPE)
            {
                break;
            } else if self.source.is_terminated() {
                return Err(Error::unexpected_end());
            } else {
                i += 1;
            }
        }

        let mut escape = false;
        let mut s = BytesMut::with_capacity(i);
        for byte in self.buffer.drain(0..i) {
            let as_slice = std::slice::from_ref(&byte);

            if escape {
                if as_slice == begin || as_slice == end {
                    // no-op
                } else {
                    s.extend(ESCAPE);
                }

                s.put_u8(byte);
                escape = false;
            } else if as_slice == ESCAPE {
                escape = true;
            } else {
                s.put_u8(byte);
            }
        }

        self.buffer.remove(0); // process the end delimiter
        self.buffer.shrink_to_fit();
        Ok(s.into())
    }

    async fn expect_delimiter(&mut self, delimiter: &'static [u8]) -> Result<(), Error> {
        while self.buffer.is_empty() && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.is_empty() {
            return Err(Error::unexpected_end());
        }

        if &self.buffer[..1] == delimiter {
            self.buffer.remove(0);
            Ok(())
        } else {
            Err(de::Error::invalid_value(
                self.buffer[0] as char,
                &format!("{}", (delimiter[0] as char)),
            ))
        }
    }

    async fn ignore_value(&mut self) -> Result<(), Error> {
        while self.buffer.is_empty() && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.is_empty() {
            Ok(())
        } else {
            match &[self.buffer[0]] {
                BITSTRING_BEGIN => {
                    self.parse_bitstring().await?;
                }
                LIST_BEGIN => {
                    self.buffer_string(LIST_BEGIN, LIST_END).await?;
                }
                MAP_BEGIN => {
                    self.buffer_string(MAP_BEGIN, MAP_END).await?;
                }
                STRING_BEGIN => {
                    self.parse_string().await?;
                }
                &[dtype] => match Type::from_u8(dtype)
                    .ok_or_else(|| de::Error::invalid_type("unknown", "any supported type"))?
                {
                    Type::None => {
                        self.parse_unit().await?;
                    }
                    Type::Bool => {
                        self.parse_element::<bool>().await?;
                    }
                    Type::F32 => {
                        self.parse_element::<f32>().await?;
                    }
                    Type::F64 => {
                        self.parse_element::<f64>().await?;
                    }
                    Type::I8 => {
                        self.parse_element::<i8>().await?;
                    }
                    Type::I16 => {
                        self.parse_element::<i16>().await?;
                    }
                    Type::I32 => {
                        self.parse_element::<i32>().await?;
                    }
                    Type::I64 => {
                        self.parse_element::<i64>().await?;
                    }
                    Type::U8 => {
                        self.parse_element::<u8>().await?;
                    }
                    Type::U16 => {
                        self.parse_element::<u16>().await?;
                    }
                    Type::U32 => {
                        self.parse_element::<u32>().await?;
                    }
                    Type::U64 => {
                        self.parse_element::<u64>().await?;
                    }
                },
            };

            Ok(())
        }
    }

    async fn maybe_delimiter(&mut self, delimiter: &'static [u8]) -> Result<bool, Error> {
        while self.buffer.is_empty() && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.is_empty() {
            Ok(false)
        } else if &self.buffer[..1] == delimiter {
            self.buffer.remove(0);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn parse_element<N: Element>(&mut self) -> Result<N, Error> {
        while self.buffer.len() < N::size() && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.len() < N::size() {
            return Err(de::Error::invalid_length(
                self.buffer.len(),
                std::any::type_name::<N>(),
            ));
        }

        let bytes: Vec<u8> = self.buffer.drain(0..N::size()).collect();
        N::parse(&bytes)
    }

    async fn parse_bitstring(&mut self) -> Result<Bytes, Error> {
        self.buffer_string(BITSTRING_BEGIN, BITSTRING_END).await
    }

    async fn parse_string(&mut self) -> Result<String, Error> {
        let s = self.buffer_string(STRING_BEGIN, STRING_END).await?;
        String::from_utf8(s.to_vec()).map_err(Error::invalid_utf8)
    }

    async fn parse_unit(&mut self) -> Result<(), Error> {
        while self.buffer.is_empty() && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.is_empty() {
            return Err(Error::unexpected_end());
        }

        match self.buffer.remove(0) {
            byte if Some(byte) == Type::None.to_u8() => Ok(()),
            other => match Type::from_u8(other) {
                Some(dtype) => Err(de::Error::invalid_type(dtype, Type::None)),
                None => Err(de::Error::invalid_type("(unknown)", Type::None)),
            },
        }
    }
}

#[async_trait]
impl<R: Read> de::Decoder for Decoder<R> {
    type Error = Error;

    async fn decode_any<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        while self.buffer.is_empty() && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.is_empty() {
            return Err(Error::unexpected_end());
        }

        match &[self.buffer[0]] {
            BITSTRING_BEGIN => self.decode_byte_buf(visitor).await,
            LIST_BEGIN => self.decode_seq(visitor).await,
            MAP_BEGIN => self.decode_map(visitor).await,
            STRING_BEGIN => self.decode_string(visitor).await,
            [dtype] => {
                match Type::from_u8(*dtype).ok_or_else(|| de::Error::custom("unknown type"))? {
                    Type::None => self.decode_unit(visitor),
                    Type::Bool => self.decode_bool(visitor),
                    Type::F32 => self.decode_f32(visitor),
                    Type::F64 => self.decode_f64(visitor),
                    Type::I8 => self.decode_i8(visitor),
                    Type::I16 => self.decode_i16(visitor),
                    Type::I32 => self.decode_i32(visitor),
                    Type::I64 => self.decode_i64(visitor),
                    Type::U8 => self.decode_u8(visitor),
                    Type::U16 => self.decode_u16(visitor),
                    Type::U32 => self.decode_u32(visitor),
                    Type::U64 => self.decode_u64(visitor),
                }
                .await
            }
        }
    }

    async fn decode_bool<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let b = self.parse_element().await?;
        visitor.visit_bool(b)
    }

    async fn decode_i8<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let i = self.parse_element().await?;
        visitor.visit_i8(i)
    }

    async fn decode_i16<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let i = self.parse_element().await?;
        visitor.visit_i16(i)
    }

    async fn decode_i32<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let i = self.parse_element().await?;
        visitor.visit_i32(i)
    }

    async fn decode_i64<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let i = self.parse_element().await?;
        visitor.visit_i64(i)
    }

    async fn decode_u8<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let u = self.parse_element().await?;
        visitor.visit_u8(u)
    }

    async fn decode_u16<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let u = self.parse_element().await?;
        visitor.visit_u16(u)
    }

    async fn decode_u32<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let u = self.parse_element().await?;
        visitor.visit_u32(u)
    }

    async fn decode_u64<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let u = self.parse_element().await?;
        visitor.visit_u64(u)
    }

    async fn decode_f32<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let f = self.parse_element().await?;
        visitor.visit_f32(f)
    }

    async fn decode_f64<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let f = self.parse_element().await?;
        visitor.visit_f64(f)
    }

    async fn decode_string<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let s = self.parse_string().await?;
        visitor.visit_string(s)
    }

    async fn decode_byte_buf<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let buf = self.parse_bitstring().await?;
        visitor.visit_byte_buf(buf.to_vec())
    }

    async fn decode_option<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        while self.buffer.is_empty() && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.is_empty() {
            return Err(Error::unexpected_end());
        }

        if Some(self.buffer[0]) == Type::None.to_u8() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self).await
        }
    }

    async fn decode_seq<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = SeqAccess::new(self, None).await?;
        visitor.visit_seq(access).await
    }

    async fn decode_unit<V: Visitor>(
        &mut self,
        visitor: V,
    ) -> Result<<V as Visitor>::Value, Self::Error> {
        self.parse_unit().await?;
        visitor.visit_unit()
    }

    async fn decode_tuple<V: Visitor>(
        &mut self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let access = SeqAccess::new(self, Some(len)).await?;
        visitor.visit_seq(access).await
    }

    async fn decode_map<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = MapAccess::new(self, None).await?;
        visitor.visit_map(access).await
    }

    async fn decode_ignored_any<V: Visitor>(
        &mut self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.ignore_value().await?;
        visitor.visit_unit()
    }
}

/// Decode the given JSON-encoded stream of bytes into an instance of `T` using the given context.
pub async fn decode<S: Stream<Item = Bytes> + Send + Unpin, T: FromStream>(
    context: T::Context,
    source: S,
) -> Result<T, Error> {
    let mut decoder = Decoder::from_stream(source.map(Result::<Bytes, Error>::Ok));
    T::from_stream(context, &mut decoder).await
}

/// Decode the given JSON-encoded stream of bytes into an instance of `T` using the given context.
pub async fn try_decode<
    E: fmt::Display,
    S: Stream<Item = Result<Bytes, E>> + Send + Unpin,
    T: FromStream,
>(
    context: T::Context,
    source: S,
) -> Result<T, Error> {
    let mut decoder = Decoder::from_stream(source.map_err(|e| de::Error::custom(e)));
    T::from_stream(context, &mut decoder).await
}

/// Decode the given JSON-encoded stream of bytes into an instance of `T` using the given context.
#[cfg(tokio)]
pub async fn read_from<R: AsyncReadExt + Send + Unpin, T: FromStream>(
    context: T::Context,
    source: R,
) -> Result<T, Error> {
    T::from_stream(context, &mut Decoder::from_reader(source)).await
}
