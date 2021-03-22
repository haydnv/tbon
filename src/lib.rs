mod constants;

pub mod de;
pub mod en;

#[cfg(test)]
mod tests {
    use std::fmt;

    use destream::{FromStream, IntoStream};

    use rand::Rng;

    use super::*;

    async fn test_primitive<
        'en,
        T: FromStream<Context = ()> + IntoStream<'en> + fmt::Debug + PartialEq + Copy + 'en,
    >(
        value: T,
    ) {
        let encoded = en::encode(value).unwrap();
        let decoded: T = de::try_decode((), encoded).await.unwrap();
        assert_eq!(decoded, value);
    }

    #[tokio::test]
    async fn test_primitives() {
        test_primitive(true).await;
        test_primitive(false).await;

        for u in 0..66000u64 {
            test_primitive(u).await;
        }

        for i in -66000..66000i64 {
            test_primitive(i).await;
        }

        for _ in 0..100000 {
            let f: f32 = rand::thread_rng().gen();
            test_primitive(f).await;
        }
    }
}
