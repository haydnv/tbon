//! Library for encoding Rust program data into a binary stream, and decoding that stream.
//!
//! Example:
//! ```
//! # use futures::executor::block_on;
//! let expected = ("one".to_string(), 2.0, vec![3, 4], vec![5u8]);
//! let stream = tbon::en::encode(&expected).unwrap();
//! let actual = block_on(tbon::de::try_decode((), stream)).unwrap();
//! assert_eq!(expected, actual);
//! ```

use element::Element;

mod constants;
mod element;

pub mod de;
pub mod en;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fmt;
    use std::iter::FromIterator;

    use async_trait::async_trait;
    use destream::{FromStream, IntoStream};
    use futures::{future, TryStreamExt};

    use rand::Rng;

    use super::de::*;
    use super::en::*;
    use num_traits::Signed;

    async fn run_test<'en, T>(value: T)
    where
        T: FromStream<Context = ()> + IntoStream<'en> + fmt::Debug + PartialEq + Clone + 'en,
    {
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
    async fn test_undefined_numbers() {
        async fn recode<'en, T>(value: T) -> T
        where
            T: FromStream<Context = ()> + IntoStream<'en> + fmt::Debug + PartialEq + Clone + 'en,
        {
            let encoded = encode(value.clone()).unwrap();
            try_decode((), encoded).await.unwrap()
        }

        assert!(recode(f32::NAN).await.is_nan());

        let inf = recode(f32::INFINITY).await;
        assert!(inf.is_infinite() && inf.is_positive());

        let inf = recode(f32::NEG_INFINITY).await;
        assert!(inf.is_infinite() && inf.is_negative());

        assert!(recode(f64::NAN).await.is_nan());

        let inf = recode(f64::INFINITY).await;
        assert!(inf.is_infinite() && inf.is_sign_positive());

        let inf = recode(f64::NEG_INFINITY).await;
        assert!(inf.is_infinite() && inf.is_sign_negative());
    }

    #[tokio::test]
    async fn test_strings() {
        run_test(String::from("hello world")).await;
        run_test(String::from("Привет, мир")).await;
        run_test(String::from("this is a \"string\" within a \\ string")).await;
        run_test(String::from("this \"string\" is \\\terminated by a \\")).await;
    }

    #[tokio::test]
    async fn test_compound() {
        let list = vec![String::from("hello"), String::from("world")];
        run_test(list).await;

        let mut map = HashMap::new();
        map.insert(-1i32, String::from("I'm a teapot"));
        map.insert(-1i32, String::from("\' \"\"     "));
        run_test(map).await;

        let mut map = HashMap::new();
        map.insert("one".to_string(), HashMap::new());
        map.insert(
            "two".to_string(),
            HashMap::from_iter(vec![("three".to_string(), 4f32)]),
        );
        run_test(map).await;
    }

    #[tokio::test]
    async fn test_tuple() {
        let tuple: (Vec<u8>, HashMap<u64, String>) = (Vec::new(), HashMap::new());
        run_test(tuple).await;

        let tuple = (
            true,
            -1i16,
            3.14,
            String::from(" hello \"world\""),
            (0..255).collect::<Vec<u8>>(),
        );
        run_test(tuple).await;

        let tuple: (bool, Vec<String>, Option<String>, Vec<String>, bool) =
            (true, vec![], None, vec![], false);

        run_test(tuple).await;
    }

    #[tokio::test]
    async fn test_array() {
        #[derive(Eq, PartialEq)]
        struct TestArray {
            data: Vec<i16>,
        }

        struct TestVisitor;

        #[async_trait]
        impl destream::de::Visitor for TestVisitor {
            type Value = TestArray;

            fn expecting() -> &'static str {
                "a TestArray"
            }

            async fn visit_array_i16<A: destream::de::ArrayAccess<i16>>(
                self,
                mut array: A,
            ) -> Result<Self::Value, A::Error> {
                let mut data = Vec::with_capacity(3);
                let mut buffer = [0; 100];
                loop {
                    let num_items = array.buffer(&mut buffer).await?;
                    if num_items > 0 {
                        data.extend(&buffer[..num_items]);
                    } else {
                        break;
                    }
                }

                Ok(TestArray { data })
            }
        }

        #[async_trait]
        impl FromStream for TestArray {
            type Context = ();

            async fn from_stream<D: destream::de::Decoder>(
                _: (),
                decoder: &mut D,
            ) -> Result<Self, D::Error> {
                decoder.decode_array_i16(TestVisitor).await
            }
        }

        impl<'en> destream::en::ToStream<'en> for TestArray {
            fn to_stream<E: destream::en::Encoder<'en>>(
                &'en self,
                encoder: E,
            ) -> Result<E::Ok, E::Error> {
                encoder.encode_array_i16(futures::stream::once(future::ready(self.data.to_vec())))
            }
        }

        impl fmt::Debug for TestArray {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                fmt::Debug::fmt(&self.data, f)
            }
        }

        let test = TestArray {
            data: (0..512).collect(),
        };

        let mut encoded = encode(&test).unwrap();
        let mut buf = Vec::new();
        while let Some(chunk) = encoded.try_next().await.unwrap() {
            buf.extend(chunk.to_vec());
        }

        let decoded: TestArray = try_decode((), encode(&test).unwrap()).await.unwrap();
        assert_eq!(test, decoded);
    }
}
