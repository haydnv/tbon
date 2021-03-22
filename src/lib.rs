mod constants;

pub mod de;
pub mod en;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fmt;

    use bytes::Bytes;
    use destream::{FromStream, IntoStream};

    use rand::Rng;

    use super::*;

    async fn run_test<
        'en,
        T: FromStream<Context = ()> + IntoStream<'en> + fmt::Debug + PartialEq + Clone + 'en,
    >(
        value: T,
    ) {
        let encoded = en::encode(value.clone()).unwrap();
        let decoded: T = de::try_decode((), encoded).await.unwrap();
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
    }
}
