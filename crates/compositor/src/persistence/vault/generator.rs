// Password generator.

use rand::Rng;

pub(super) const GEN_ALPHA: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub(super) const GEN_SYM: &[u8] = b"!@#$%^&*-_=+?";

pub fn generate_password(length: usize, symbols: bool) -> String {
    let alphabet: Vec<u8> = if symbols {
        GEN_ALPHA.iter().chain(GEN_SYM.iter()).copied().collect()
    } else {
        GEN_ALPHA.to_vec()
    };
    let len = length.clamp(8, 128);
    let mut rng = rand::rng();
    let mut next_byte = || {
        let mut b = [0u8; 1];
        rng.fill_bytes(&mut b);
        b[0]
    };
    (0..len)
        .map(|_| alphabet[uniform_index(alphabet.len(), &mut next_byte)] as char)
        .collect()
}

/// Uniform index in `0..n` (1 ≤ n ≤ 256) from random bytes. Rejection
/// sampling: bytes at or above the largest multiple of `n` are redrawn, so
/// every index is equally likely (plain `b % n` favours low indices).
pub(super) fn uniform_index(n: usize, next_byte: &mut impl FnMut() -> u8) -> usize {
    assert!((1..=256).contains(&n), "alphabet size {n} out of range");
    let zone = 256 - 256 % n;
    loop {
        let b = next_byte() as usize;
        if b < zone {
            return b % n;
        }
    }
}
