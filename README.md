# Tinychain Binary Object Notation

Tinychain Binary Object Notation (TBON) is a compact and versatile stream-friendly binary serialization format.

Example:
```rust
let expected = ("one".to_string(), 2.0, vec![3, 4], Bytes::from(vec![5u8]));
let stream = tbon::en::encode(&expected).unwrap();
let actual = tbon::de::try_decode((), stream).await.unwrap();
assert_eq!(expected, actual);
```
