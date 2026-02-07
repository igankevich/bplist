use crate::Deserializer;
use crate::from_slice;
use crate::to_vec;

use alloc::string::String;
use alloc::vec::Vec;

use arbitrary::Arbitrary;
use arbtest::arbtest;
use serde::Deserialize;
use serde::Serialize;

#[test]
fn real_data_works() {
    for data in [
        [
            98, 112, 108, 105, 115, 116, 48, 48, 212, 1, 2, 3, 4, 5, 6, 6, 7, 81, 51, 81, 49, 81,
            50, 81, 48, 16, 0, 34, 0, 0, 0, 0, 16, 1, 8, 17, 19, 21, 23, 25, 27, 32, 0, 0, 0, 0, 0,
            0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 34,
        ]
        .as_slice(),
        [
            98, 112, 108, 105, 115, 116, 48, 48, 210, 1, 2, 3, 4, 81, 49, 81, 50, 16, 3, 162, 5,
            10, 210, 6, 7, 8, 9, 83, 50, 46, 49, 83, 50, 46, 50, 35, 64, 84, 91, 157, 0, 0, 0, 0,
            35, 64, 176, 115, 0, 0, 0, 0, 0, 210, 6, 7, 11, 12, 35, 63, 227, 53, 160, 0, 0, 0, 0,
            35, 64, 86, 192, 0, 0, 0, 0, 0, 8, 13, 15, 17, 19, 22, 27, 31, 35, 44, 53, 58, 67, 0,
            0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 13, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 76,
        ]
        .as_slice(),
        [
            98, 112, 108, 105, 115, 116, 48, 48, 16, 0, 8, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0,
            0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10,
        ]
        .as_slice(),
    ] {
        let de = Deserializer::from_slice(&data, u32::MAX).unwrap();
        let value = serde_json::Value::deserialize(de).unwrap();
        std::eprintln!("{}", serde_json::to_string_pretty(&value).unwrap());
    }
}

#[test]
fn serializer_works() {
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Arbitrary)]
    struct Empty;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Arbitrary)]
    struct Object {
        value_u8: u8,
        value_u16: u16,
        value_u32: u32,
        value_u64: u64,
        value_u128: u128,
        string: String,
        option: Option<u8>,
        unit: (),
        empty: Empty,
        tuple: (i8, i16, i32, i64, i128),
        children: Vec<Object>,
    }
    arbtest(|u| {
        let expected: Object = u.arbitrary()?;
        let output = to_vec(&expected);
        //std::eprintln!("Output {output:?}");
        let actual: Object =
            from_slice(&output, u32::MAX).unwrap_or_else(|_| panic!("{expected:#?}"));
        assert_eq!(expected, actual);
        Ok(())
    });
}
