use std::pin::Pin;
use std::task::{self, Poll};

use bytes::Bytes;
use destream::en::{self, IntoStream};
use futures::ready;
use futures::stream::{Fuse, FusedStream, Stream, StreamExt};
use futures::task::Context;
use pin_project::pin_project;

use crate::constants::*;

use super::{ByteStream, Encoder};

#[pin_project]
struct MapEntryStream<'en> {
    #[pin]
    key: Fuse<ByteStream<'en>>,

    #[pin]
    value: Fuse<ByteStream<'en>>,
}

impl<'en> MapEntryStream<'en> {
    fn new<K: IntoStream<'en>, V: IntoStream<'en>>(key: K, value: V) -> Result<Self, super::Error> {
        let key = key.into_stream(Encoder)?;
        let value = value.into_stream(Encoder)?;

        Ok(Self {
            key: key.fuse(),
            value: value.fuse(),
        })
    }
}

impl<'en> Stream for MapEntryStream<'en> {
    type Item = Result<Bytes, super::Error>;

    fn poll_next(self: Pin<&mut Self>, cxt: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        Poll::Ready(loop {
            if !this.key.is_terminated() {
                if let Some(result) = ready!(this.key.as_mut().poll_next(cxt)) {
                    break Some(result);
                }
            } else if !this.value.is_terminated() {
                if let Some(result) = ready!(this.value.as_mut().poll_next(cxt)) {
                    break Some(result);
                }
            } else {
                break None;
            }
        })
    }
}

impl<'en> FusedStream for MapEntryStream<'en> {
    fn is_terminated(&self) -> bool {
        self.key.is_terminated() && self.value.is_terminated()
    }
}

#[pin_project]
struct TBONEncodingStream<
    I: Stream<Item = Result<Bytes, super::Error>>,
    S: Stream<Item = Result<I, super::Error>>,
> {
    #[pin]
    source: Fuse<S>,

    next: Option<Pin<Box<I>>>,

    started: bool,
    finished: bool,

    start: &'static [u8; 1],
    end: &'static [u8; 1],
}

impl<I: Stream<Item = Result<Bytes, super::Error>>, S: Stream<Item = Result<I, super::Error>>>
    Stream for TBONEncodingStream<I, S>
{
    type Item = Result<Bytes, super::Error>;

    fn poll_next(self: Pin<&mut Self>, cxt: &mut task::Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        Poll::Ready(loop {
            match this.next {
                Some(next) => match ready!(next.as_mut().poll_next(cxt)) {
                    Some(result) => break Some(result),
                    None => *this.next = None,
                },
                None => match ready!(this.source.as_mut().poll_next(cxt)) {
                    Some(Ok(next)) => {
                        *this.next = Some(Box::pin(next));

                        if !*this.started {
                            *this.started = true;
                            break Some(Ok(Bytes::from_static(*this.start)));
                        }
                    }
                    Some(Err(cause)) => break Some(Err(en::Error::custom(cause))),
                    None if !*this.started => {
                        *this.started = true;
                        break Some(Ok(Bytes::from_static(*this.start)));
                    }
                    None if !*this.finished => {
                        *this.finished = true;
                        break Some(Ok(Bytes::from_static(*this.end)));
                    }
                    None => break None,
                },
            }
        })
    }
}

impl<I: Stream<Item = Result<Bytes, super::Error>>, S: Stream<Item = Result<I, super::Error>>>
    FusedStream for TBONEncodingStream<I, S>
{
    fn is_terminated(&self) -> bool {
        self.finished
    }
}

pub fn encode_list<'en, I: IntoStream<'en>, S: Stream<Item = I> + Send + Unpin + 'en>(
    seq: S,
) -> impl Stream<Item = Result<Bytes, super::Error>> + 'en {
    let source = seq.map(|item| item.into_stream(Encoder));

    TBONEncodingStream {
        source: source.fuse(),
        next: None,
        started: false,
        finished: false,
        start: LIST_BEGIN,
        end: LIST_END,
    }
}

pub fn encode_map<
    'en,
    K: IntoStream<'en>,
    V: IntoStream<'en>,
    S: Stream<Item = (K, V)> + Send + Unpin + 'en,
>(
    seq: S,
) -> impl Stream<Item = Result<Bytes, super::Error>> + Send + Unpin + 'en {
    let source = seq.map(|(key, value)| MapEntryStream::new(key, value));

    TBONEncodingStream {
        source: source.fuse(),
        next: None,
        started: false,
        finished: false,
        start: MAP_BEGIN,
        end: MAP_END,
    }
}
