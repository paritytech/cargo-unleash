use parity_scale_codec_derive::{Encode, Decode};

#[derive(Debug, PartialEq, Encode, Decode)]
pub enum EnumType {
	#[codec(index = 15)]
	A,
	B(u32, u64),
	C {
		a: u32,
		b: u64,
	},
}
