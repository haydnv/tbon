//! Library for encoding Rust program data into a binary stream, and decoding that stream.
//!
//! Example:
//! ```
//! # use bytes::Bytes;
//! # use futures::executor::block_on;
//! let expected = ("one".to_string(), 2.0, vec![3, 4], Bytes::from(vec![5u8]));
//! let stream = tbon::en::encode(&expected).unwrap();
//! let actual = block_on(tbon::de::try_decode((), stream)).unwrap();
//! assert_eq!(expected, actual);
//! ```
//!
//! *Important note*: TBON adds one byte to primitive values to record the value's type. In cases
//! where TBON must encode a large number of values of the same type, such as an n-dimensional array
//! of numbers, this adds 12-50% overhead to the size of the encoded data. However, encoding a
//! `Bytes` struct has a negligible overhead of only two bytes, regardless of the data size.
//!
//! To efficiently encode an n-dimensional array, it is recommended to use compression
//! (e.g. gzip) and/or implement [`destream::FromStream`] and [`destream::ToStream`] using `Bytes`.
//!

mod constants;

pub mod de;
pub mod en;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fmt;
    use std::iter::FromIterator;

    use bytes::Bytes;
    use destream::{FromStream, IntoStream};

    use rand::Rng;

    use super::de::*;
    use super::en::*;

    async fn run_test<
        'en,
        T: FromStream<Context = ()> + IntoStream<'en> + fmt::Debug + PartialEq + Clone + 'en,
    >(
        value: T,
    ) {
        let encoded = encode(value.clone()).unwrap();
        let decoded: T = try_decode((), encoded).await.unwrap();
        assert_eq!(decoded, value);
    }

    #[tokio::test]
    async fn test_primitives() {
        run_test(true).await;
        run_test(false).await;

        for u in 0..66000u64 {
            run_test(u).await;
        }

        for i in -66000..66000i64 {
            run_test(i).await;
        }

        for _ in 0..100000 {
            let f: f32 = rand::thread_rng().gen();
            run_test(f).await;
        }
    }

    #[tokio::test]
    async fn test_strings() {
        run_test(String::from("hello world")).await;
        run_test(String::from("Привет, мир")).await;
        run_test(String::from("this is a \"string\" within a \\ string")).await;
        run_test(String::from("this \"string\" is \\\terminated by a \\")).await;

        let bitstring = Bytes::from((0..255u8).collect::<Vec<u8>>());
        run_test(bitstring).await;
    }

    #[tokio::test]
    async fn test_compound() {
        let list = vec![String::from("hello"), String::from("world")];
        run_test(list).await;

        let tuple = (
            true,
            -1i16,
            3.14,
            String::from(" hello \"world\""),
            Bytes::from((0..255u8).collect::<Vec<u8>>()),
        );
        run_test(tuple).await;

        let mut map = HashMap::new();
        map.insert(-1i32, String::from("I'm a teapot"));
        map.insert(-1i32, String::from("\' \"\"     "));
        run_test(map).await;

        let tuple: (Vec<f32>, Vec<i32>) = (vec![], vec![1]);
        run_test(tuple).await;

        let mut map = HashMap::new();
        map.insert("one".to_string(), HashMap::new());
        map.insert(
            "two".to_string(),
            HashMap::from_iter(vec![("three".to_string(), 4f32)]),
        );
        run_test(map).await;
    }
}
