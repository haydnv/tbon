//! Decode a Rust data structure from a TBON-encoded stream.

use std::fmt;
use std::marker::PhantomData;

use bytes::{BufMut, Bytes, BytesMut};
use destream::{de, FromStream, Visitor};
use futures::stream::{Fuse, FusedStream, Stream, StreamExt, TryStreamExt};
use futures::FutureExt;
use num_traits::{FromPrimitive, ToPrimitive};

#[cfg(feature = "tokio-io")]
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};

use super::constants::*;
use super::Element;

const CHUNK_SIZE: usize = 4096;
const SNIPPET_LEN: usize = 10;

/// Methods common to any decodable [`Stream`]
#[trait_variant::make(Send)]
pub trait Read: Send + Unpin {
    /// Read the next chunk of [`Bytes`] in the [`Stream`], if any.
    async fn next(&mut self) -> Option<Result<Bytes, Error>>;

    /// Return `true` if there is no more content to be read from the [`Stream`].
    fn is_terminated(&self) -> bool;
}

/// A [`Stream`] to decode
pub struct SourceStream<S> {
    source: Fuse<S>,
}

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

/// A buffered reader of a decodable stream
#[cfg(feature = "tokio-io")]
pub struct SourceReader<R: AsyncRead> {
    reader: BufReader<R>,
    terminated: bool,
}

#[cfg(feature = "tokio-io")]
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

#[cfg(feature = "tokio-io")]
impl<R: AsyncRead> From<R> for SourceReader<R> {
    fn from(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            terminated: false,
        }
    }
}

/// An error encountered while decoding a TBON stream.
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

struct ArrayAccess<'a, S, T> {
    decoder: &'a mut Decoder<S>,
    dtype: PhantomData<T>,
    done: bool,
}

impl<'a, S: Read + 'a, T: Element> ArrayAccess<'a, S, T> {
    async fn new(decoder: &'a mut Decoder<S>) -> Result<ArrayAccess<'a, S, T>, Error> {
        let dtype = &[T::dtype().to_u8().unwrap()];

        decoder.expect_delimiter(ARRAY_DELIMIT).await?;
        decoder.expect_delimiter(dtype).await?;

        let done = decoder.maybe_delimiter(ARRAY_DELIMIT).await?;

        Ok(ArrayAccess {
            decoder,
            dtype: PhantomData,
            done,
        })
    }
}

impl<'a, S: Read + 'a, T: Element + Send> de::ArrayAccess<T> for ArrayAccess<'a, S, T> {
    type Error = Error;

    async fn buffer(&mut self, buffer: &mut [T]) -> Result<usize, Self::Error> {
        if self.done {
            return Ok(0);
        }

        let size = T::SIZE;
        let mut limit = buffer.len() * size;

        let mut i = 0;
        let mut escaped = false;

        while i < limit {
            while i >= self.decoder.buffer.len() && !self.decoder.source.is_terminated() {
                self.decoder.buffer().await?;
            }

            if i < self.decoder.buffer.len()
                && &self.decoder.buffer[i..i + 1] == ARRAY_DELIMIT
                && !escaped
            {
                break;
            }

            if escaped {
                escaped = false;
            } else if self.decoder.buffer[i] == ESCAPE[0] {
                escaped = true;
                limit += 1;
            }

            i += 1;
        }

        let mut escape = false;
        let mut escaped = BytesMut::with_capacity(i);
        for byte in self.decoder.buffer.drain(0..i) {
            let as_slice = std::slice::from_ref(&byte);

            if escape {
                escaped.put_u8(byte);
                escape = false;
            } else if as_slice == ESCAPE {
                escape = true;
            } else {
                escaped.put_u8(byte);
            }
        }

        let mut elements = 0;

        for bytes in escaped.chunks(size) {
            buffer[elements] = T::parse(bytes)?;
            elements += 1;
        }

        while self.decoder.buffer.is_empty() {
            if self.decoder.source.is_terminated() {
                return Err(Error::unexpected_end());
            } else {
                self.decoder.buffer().await?;
            }
        }

        if &self.decoder.buffer[0..1] == ARRAY_DELIMIT {
            self.done = true;
            // process the end delimiter
            self.decoder.buffer.remove(0);
        }

        self.decoder.buffer.shrink_to_fit();

        Ok(elements)
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
        if self.done {
            return Err(de::Error::custom(
                "called MapAccess::next_value but the map has already ended",
            ));
        }

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

/// A structure that decodes Rust values from a TBON stream.
pub struct Decoder<R> {
    source: R,
    buffer: Vec<u8>,
}

impl<R> Decoder<R> {
    fn contents(&self, max_len: usize) -> String {
        let len = Ord::min(self.buffer.len(), max_len);
        let mut chunks: Vec<String> = Vec::with_capacity(len);
        let mut chunk = Vec::with_capacity(len);
        let mut is_ascii = false;
        for c in &self.buffer[..len] {
            if is_ascii != c.is_ascii() {
                chunks.push(chunk.iter().collect());
                chunk.clear();
                is_ascii = c.is_ascii();
            }

            if c.is_ascii() {
                chunk.push(*c as char);
            } else {
                chunk.extend(format!(" {} ", c).as_bytes().iter().map(|c| *c as char))
            }
        }

        if !chunk.is_empty() {
            chunks.push(chunk.into_iter().collect());
        }

        chunks.join("")
    }
}

#[cfg(feature = "tokio-io")]
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
    /// Create a new [`Decoder`] from a source [`Stream`].
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
        let mut escaped = false;
        loop {
            while i >= self.buffer.len() && !self.source.is_terminated() {
                self.buffer().await?;
            }

            if i < self.buffer.len() && &self.buffer[i..i + 1] == end && !escaped {
                break;
            } else if self.source.is_terminated() {
                return Err(Error::unexpected_end());
            }

            if escaped {
                escaped = false;
            } else if self.buffer[i] == ESCAPE[0] {
                escaped = true;
            }

            i += 1;
        }

        let mut escape = false;
        let mut s = BytesMut::with_capacity(i);
        for byte in self.buffer.drain(0..i) {
            let as_slice = std::slice::from_ref(&byte);

            if escape {
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

    async fn ignore_string(
        &mut self,
        begin: &'static [u8],
        end: &'static [u8],
    ) -> Result<(), Error> {
        self.expect_delimiter(begin).await?;

        let mut i = 0;
        let mut escaped = false;
        loop {
            while i >= self.buffer.len() && !self.source.is_terminated() {
                self.buffer().await?;
            }

            if i < self.buffer.len() && &self.buffer[i..i + 1] == end && !escaped {
                self.buffer.drain(..i);
                break;
            } else if self.source.is_terminated() {
                return Err(Error::unexpected_end());
            }

            if escaped {
                escaped = false;
            } else if self.buffer[i] == ESCAPE[0] {
                escaped = true;
            }

            if i > CHUNK_SIZE {
                self.buffer.drain(..i);
                i = 0;
            } else {
                i += 1;
            }
        }

        self.buffer.remove(0); // process the end delimiter
        self.buffer.shrink_to_fit();
        Ok(())
    }

    async fn expect_delimiter(&mut self, delimiter: &[u8]) -> Result<(), Error> {
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
            fn char_to_string(c: u8) -> String {
                if c < ' ' as u8 {
                    c.to_string()
                } else {
                    (c as char).to_string()
                }
            }

            let actual = char_to_string(self.buffer[0]);
            let expected = char_to_string(delimiter[0]);

            let snippet = self.contents(SNIPPET_LEN);
            Err(de::Error::custom(format!(
                "unexpected delimiter {}, expected {} at {}",
                actual, expected, snippet
            )))
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
                LIST_BEGIN => {
                    self.ignore_string(LIST_BEGIN, LIST_END).await?;
                }
                MAP_BEGIN => {
                    self.ignore_string(MAP_BEGIN, MAP_END).await?;
                }
                STRING_DELIMIT => {
                    self.ignore_string(STRING_DELIMIT, STRING_DELIMIT).await?;
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
        while self.buffer.len() <= N::SIZE && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.len() <= N::SIZE {
            return Err(de::Error::invalid_length(
                self.buffer.len(),
                std::any::type_name::<N>(),
            ));
        }

        let dtype = self.buffer.remove(0);
        if Some(dtype) == N::dtype().to_u8() {
            // no-op
        } else if let Some(dtype) = Type::from_u8(dtype) {
            return Err(de::Error::invalid_type(dtype, N::dtype()));
        } else {
            return Err(de::Error::invalid_value(dtype, "a TBON type bit"));
        }

        let bytes: Vec<u8> = self.buffer.drain(0..N::SIZE).collect();
        N::parse(&bytes)
    }

    async fn parse_string(&mut self) -> Result<String, Error> {
        let s = self.buffer_string(STRING_DELIMIT, STRING_DELIMIT).await?;
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

impl<R: Read> de::Decoder for Decoder<R> {
    type Error = Error;

    async fn decode_any<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        while self.buffer.is_empty() && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.is_empty() {
            return Err(Error::unexpected_end());
        }

        fn type_from(bit: u8) -> Result<Type, Error> {
            Type::from_u8(bit)
                .ok_or_else(|| de::Error::custom(format!("invalid type bit: {}", bit)))
        }

        match &[self.buffer[0]] {
            ARRAY_DELIMIT => {
                while self.buffer.len() < 2 && !self.source.is_terminated() {
                    self.buffer().await?;
                }

                match type_from(self.buffer[1])? {
                    Type::Bool => self.decode_array_bool(visitor).await,
                    Type::F32 => self.decode_array_f32(visitor).await,
                    Type::F64 => self.decode_array_f64(visitor).await,
                    Type::I16 => self.decode_array_i16(visitor).await,
                    Type::I32 => self.decode_array_i32(visitor).await,
                    Type::I64 => self.decode_array_i64(visitor).await,
                    Type::U8 => self.decode_array_u8(visitor).await,
                    Type::U16 => self.decode_array_u16(visitor).await,
                    Type::U32 => self.decode_array_u32(visitor).await,
                    Type::U64 => self.decode_array_u64(visitor).await,
                    dtype => return Err(de::Error::invalid_type(dtype, "a supported array type")),
                }
            }
            LIST_BEGIN => self.decode_seq(visitor).await,
            MAP_BEGIN => self.decode_map(visitor).await,
            STRING_DELIMIT => self.decode_string(visitor).await,
            [dtype] => match type_from(*dtype)? {
                Type::None => self.decode_unit(visitor).await,
                Type::Bool => self.decode_bool(visitor).await,
                Type::F32 => self.decode_f32(visitor).await,
                Type::F64 => self.decode_f64(visitor).await,
                Type::I8 => self.decode_i8(visitor).await,
                Type::I16 => self.decode_i16(visitor).await,
                Type::I32 => self.decode_i32(visitor).await,
                Type::I64 => self.decode_i64(visitor).await,
                Type::U8 => self.decode_u8(visitor).await,
                Type::U16 => self.decode_u16(visitor).await,
                Type::U32 => self.decode_u32(visitor).await,
                Type::U64 => self.decode_u64(visitor).await,
            },
        }
    }

    async fn decode_bool<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let b = self.parse_element().await?;
        visitor.visit_bool(b)
    }

    async fn decode_bytes<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        self.decode_array_u8(visitor).await
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

    async fn decode_array_bool<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        // TODO: remove boxing when https://github.com/rust-lang/rust/issues/100013 is resolved
        visitor.visit_array_bool(access).boxed().await
    }

    async fn decode_array_i8<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_i8(access).boxed().await
    }

    async fn decode_array_i16<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_i16(access).boxed().await
    }

    async fn decode_array_i32<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_i32(access).boxed().await
    }

    async fn decode_array_i64<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_i64(access).boxed().await
    }

    async fn decode_array_u8<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_u8(access).boxed().await
    }

    async fn decode_array_u16<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_u16(access).boxed().await
    }

    async fn decode_array_u32<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_u32(access).boxed().await
    }

    async fn decode_array_u64<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_u64(access).boxed().await
    }

    async fn decode_array_f32<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_f32(access).boxed().await
    }

    async fn decode_array_f64<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = ArrayAccess::new(self).await?;
        visitor.visit_array_f64(access).boxed().await
    }

    async fn decode_string<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let s = self.parse_string().await?;
        visitor.visit_string(s)
    }

    async fn decode_option<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        while self.buffer.is_empty() && !self.source.is_terminated() {
            self.buffer().await?;
        }

        if self.buffer.is_empty() {
            return Err(Error::unexpected_end());
        }

        if Some(self.buffer[0]) == Type::None.to_u8() {
            self.buffer.remove(0);
            visitor.visit_none()
        } else {
            visitor.visit_some(self).await
        }
    }

    async fn decode_map<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = MapAccess::new(self, None).await?;
        visitor.visit_map(access).boxed().await
    }

    async fn decode_seq<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        let access = SeqAccess::new(self, None).await?;
        visitor.visit_seq(access).boxed().await
    }

    async fn decode_tuple<V: Visitor>(
        &mut self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let access = SeqAccess::new(self, Some(len)).await?;
        visitor.visit_seq(access).boxed().await
    }

    async fn decode_unit<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        self.parse_unit().await?;
        visitor.visit_unit()
    }

    async fn decode_uuid<V: Visitor>(&mut self, visitor: V) -> Result<V::Value, Self::Error> {
        self.decode_array_u8(visitor).await
    }

    async fn decode_ignored_any<V: Visitor>(
        &mut self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.ignore_value().await?;
        visitor.visit_unit()
    }
}

/// Decode the given TBON-encoded stream of bytes into an instance of `T` using the given context.
pub async fn decode<S: Stream<Item = Bytes> + Send + Unpin, T: FromStream>(
    context: T::Context,
    source: S,
) -> Result<T, Error> {
    let mut decoder = Decoder::from_stream(source.map(Result::<Bytes, Error>::Ok));
    T::from_stream(context, &mut decoder).await
}

/// Decode the given TBON-encoded stream of bytes into an instance of `T` using the given context.
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

/// Decode the given TBON-encoded stream of bytes into an instance of `T` using the given context.
#[cfg(feature = "tokio-io")]
pub async fn read_from<R: AsyncReadExt + Send + Unpin, T: FromStream>(
    context: T::Context,
    source: R,
) -> Result<T, Error> {
    T::from_stream(context, &mut Decoder::from_reader(source)).await
}
