extern crate orion;

const S: [u8; 32] = [
    0x85, 0x4E, 0x69, 0xFD, 0x6A, 0xEE, 0x45, 0x29, 0xB8, 0xA9, 0x5F, 0x6B, 0x3E, 0xBF, 0x1D, 0xD9,
    0x63, 0xAB, 0x91, 0x5D, 0x87, 0xAB, 0xED, 0x3B, 0x63, 0xA7, 0xFA, 0x8A, 0x51, 0x40, 0x8A, 0x9F,
];

fn secure_key(key_hint: &str) -> [u8; 32] {
    orion::default::hkdf(&S, key_hint.as_bytes(), b"").unwrap()
}

pub fn open_file(
    key_hint: &str,
    data: Vec<u8>,
) -> Result<Vec<u8>, orion::errors::UnknownCryptoError> {
    let secure_key = secure_key(&key_hint);
    orion::default::decrypt(&secure_key, data.as_slice())
}

pub fn seal_file(key_hint: &str, data: Vec<u8>) -> Vec<u8> {
    let secure_key = secure_key(&key_hint);
    orion::default::encrypt(&secure_key, data.as_slice()).unwrap()
}
