use parity_scale_codec_derive::codec::Encode;
use sidekick::EnumType;

fn main() {

    let a = EnumType::A;
    let b = EnumType::B(1, 2);
    let c = EnumType::C { a: 1, b: 2 };

    a.using_encoded(|ref slice| {
        assert_eq!(slice, &b"\x0f");
    });

    b.using_encoded(|ref slice| {
        assert_eq!(slice, &b"\x01\x01\0\0\0\x02\0\0\0\0\0\0\0");
    });

    c.using_encoded(|ref slice| {
        assert_eq!(slice, &b"\x02\x01\0\0\0\x02\0\0\0\0\0\0\0");
    });
}
