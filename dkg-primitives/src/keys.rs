// Copyright 2022 Webb Technologies Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
use crate::types::{FE, GE};
use curv::{arithmetic::Converter, BigInt};
use hex::{self};
use secp256k1::curve::{Affine, Field};
use sha3::{Digest, Keccak256};
use std::convert::TryInto;

pub use multi_party_ecdsa::protocols::multi_party_ecdsa::gg_2020::{
	party_i::*,
	state_machine::{keygen::*, sign::*},
};

pub fn recover_pub_key(sig: &SignatureRecid, message: &BigInt) -> Result<GE, String> {
	recover_pub_key_raw(message, sig.recid, sig.r.clone(), sig.s.clone())
}

pub fn recover_pub_key_raw(message: &BigInt, v: u8, r: FE, s: FE) -> Result<GE, String> {
	// r^-1 * (s*R - z*G) = R * s * r^-1 - G * z * r^-1

	let p_minus_order: Field =
		Field::new(0, 0, 0, 1, 0x45512319, 0x50B75FC4, 0x402DA172, 0x2FC9BAEE);

	let order_as_fe: Field = Field::new(
		0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFE, 0xBAAEDCE6, 0xAF48A03B, 0xBFD25E8C,
		0xD0364141,
	);

	// r value as X coordinate in a Field format
	let mut fx = Field::default();
	let r_bytes: [u8; 32] = match r.to_bigint().to_bytes().try_into() {
		Ok(res) => res,
		Err(_err) => return Err("Invalid r value".to_string()),
	};
	let overflow = fx.set_b32(&r_bytes);
	debug_assert!(overflow);

	// Check validity of a point constructed from r value
	if v & 2 > 0 {
		if fx >= p_minus_order {
			return Err("Invalalid signature".to_string())
		}
		fx += order_as_fe;
	}

	// point, calculated from r value
	let mut r_calc_point = Affine::default();
	if !r_calc_point.set_xo_var(&fx, v & 1 > 0) {
		return Err("Invalid signature".to_string())
	}
	r_calc_point.x.normalize();
	r_calc_point.y.normalize();

	// point, calculated from r value in Secp256k1Point format
	let r_calc = {
		let r_x = BigInt::from_bytes(&r_calc_point.x.b32());
		let r_y = BigInt::from_bytes(&r_calc_point.y.b32());

		match GE::from_coords(&r_x, &r_y) {
			Ok(res) => res,
			Err(_err) => return Err("Could not construct Secp256k1Point from r value".to_string()),
		}
	};

	let g = GE::generator(); // G
	let z = FE::from(message); // z

	let rn = r.invert().unwrap(); // r^-1

	let rsrn = r_calc * s * rn.clone(); // R * s * r^-1
	let gzrn = g * z * rn; // G * z * r^-1

	let pub_key = rsrn - gzrn;

	Ok(pub_key)
}

pub fn convert_to_checksum_eth_address(addr: &str) -> Result<String, String> {
	let addr = addr.to_lowercase();

	let addr_hash = {
		let mut hasher = Keccak256::new();
		hasher.update(addr.as_bytes());
		hex::encode(hasher.finalize())
	};

	let addr_vec = addr.chars().collect::<Vec<char>>();
	let addr_hash_vec = addr_hash.chars().collect::<Vec<char>>();

	let mut checksum_addr = String::new();

	for i in 0..addr_vec.len() {
		let n = match u16::from_str_radix(&addr_hash_vec[i].to_string(), 16) {
			Ok(res) => res,
			Err(err) => return Err(err.to_string()),
		};

		if n > 7 {
			checksum_addr.push_str(&addr_vec[i].to_uppercase().to_string());
		} else {
			checksum_addr.push(addr_vec[i]);
		}
	}

	Ok(checksum_addr)
}

pub fn convert_to_eth_address(pub_key: &GE) -> Result<String, String> {
	let x = match pub_key.x_coord() {
		Some(res) => res,
		None => return Err("X coordinate is absent".to_string()),
	};
	let y = match pub_key.y_coord() {
		Some(res) => res,
		None => return Err("Y coordinate is absent".to_string()),
	};

	let mut serialized_pub_key = x.to_hex();
	serialized_pub_key.push_str(&y.to_hex());

	let mut hasher = Keccak256::new();
	match hex::decode(serialized_pub_key) {
		Ok(decoded) => hasher.update(decoded),
		Err(err) => return Err(err.to_string()),
	}
	let pub_key_hash = hasher.finalize();

	let serialized_pub_key_hash = hex::encode(&pub_key_hash);
	let eth_address = &serialized_pub_key_hash[24..serialized_pub_key_hash.len()];

	convert_to_checksum_eth_address(eth_address)
}

#[cfg(test)]
mod tests {
	use super::{convert_to_checksum_eth_address, convert_to_eth_address, recover_pub_key_raw, GE};
	use curv::{
		arithmetic::Converter,
		elliptic::curves::{Scalar, Secp256k1},
		BigInt,
	};

	#[test]
	fn test_recover_pub_key_raw() {
		let message =
			BigInt::from_hex("4ff5b6816dd118b8c362939cfb7332f667ff071a1828aa96c760871e1b5634fd")
				.unwrap();

		println!("Message: {:?}", message.to_hex());

		let v: u8 = 0;
		let r = Scalar::<Secp256k1>::from(
			&BigInt::from_hex("4f282dd8be26cc20c27ccb986452411cc90ba9b9e9802256b7ecd3ba98b6fac4")
				.unwrap(),
		);
		let s = Scalar::<Secp256k1>::from(
			&BigInt::from_hex("5e378bbb7f7c7db9c4c7baf898134d636c810d2cb2cec5c85e36ee2c341265be")
				.unwrap(),
		);

		println!("r: {:?}", &r);
		println!("s: {:?}", &s);

		let recovered = recover_pub_key_raw(&message, v, r, s).unwrap();

		let expected_x =
			BigInt::from_hex("91a27f998f3971e5b62bbde231264271faf91f837c506fde88c4bfb9c533f1c2")
				.unwrap();
		let expected_y =
			BigInt::from_hex("c7b40c9fdca6815d43b315c8b039ecda1ba7eabd97794496c3023730581d7d63")
				.unwrap();

		let actual_x = recovered.x_coord().unwrap();
		let actual_y = recovered.y_coord().unwrap();

		println!("Expected pubkey: {}{}", expected_x.to_hex(), expected_y.to_hex());
		println!("Recovered pubkey: {}{}", actual_x.to_hex(), actual_y.to_hex());

		assert_eq!(actual_x, expected_x);
		assert_eq!(actual_y, expected_y);
	}

	#[test]
	fn test_convert_to_checksum_eth_address() {
		let test = |addr: &str| {
			assert_eq!(addr, &convert_to_checksum_eth_address(&addr).unwrap());
		};

		test("5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed");
		test("fB6916095ca1df60bB79Ce92cE3Ea74c37c5d359");
		test("dbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB");
		test("D1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb");
	}

	#[test]
	fn test_convert_to_eth_address() {
		let x =
			BigInt::from_hex("91a27f998f3971e5b62bbde231264271faf91f837c506fde88c4bfb9c533f1c2")
				.unwrap();
		let y =
			BigInt::from_hex("c7b40c9fdca6815d43b315c8b039ecda1ba7eabd97794496c3023730581d7d63")
				.unwrap();

		let eth_addr = convert_to_eth_address(&GE::from_coords(&x, &y).unwrap()).unwrap();

		assert_eq!("E24FAFbc593B2Dbb8DaF296F9BBf5DA94E633A40", eth_addr);
	}
}
