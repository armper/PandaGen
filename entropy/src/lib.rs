#![no_std]
//! Random bytes for a machine with no operating system under it (SEC-030).
//!
//! Keys for TLS, DNS query ids, TCP's initial sequence numbers and
//! passphrase salts all need numbers no one can guess. This crate is the
//! part of that which runs anywhere, and so under `cargo test`:
//!
//! - [`Pool`] gathers unpredictable samples -- the time-stamp counter
//!   read as interrupts arrive, the CPU's own random numbers where it has
//!   them -- by hashing each into a running SHA-256 state, so the pool is
//!   never worse than its best input.
//! - [`HmacDrbg`] is NIST SP 800-90A's HMAC_DRBG over SHA-256: a seed in,
//!   as many bytes out as are asked for, and a compromise of its state
//!   later cannot reveal output earlier (backtracking resistance).
//! - [`Rng`] joins them: a DRBG seeded from the pool, reseeded from it
//!   as fresh samples arrive.
//!
//! The kernel supplies the samples; nothing here reads hardware.

extern crate alloc;

use alloc::vec::Vec;
use remote_ipc::sha256;

/// Bytes one `generate` may return (SP 800-90A: 2^19 bits).
pub const MAX_REQUEST: usize = 1 << 16;
/// Generates before a reseed is required (SP 800-90A allows 2^48; this is
/// far stricter, so fresh samples are folded in often).
pub const RESEED_INTERVAL: u64 = 1 << 20;
/// Samples the pool gathers before `Rng` folds it into the DRBG.
pub const SAMPLES_PER_RESEED: u64 = 64;

/// A running hash of unpredictable samples.
#[derive(Clone)]
pub struct Pool {
    state: [u8; 32],
    pending: Vec<u8>,
    samples: u64,
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

impl Pool {
    pub fn new() -> Self {
        Self {
            state: [0; 32],
            pending: Vec::with_capacity(64),
            samples: 0,
        }
    }

    /// Mix in `bytes`: a sample, or a block from the CPU's generator.
    pub fn add(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        self.samples += 1;
        if self.pending.len() >= 64 {
            self.fold();
        }
    }

    /// Mix in a 64-bit sample (a counter read at an unpredictable moment).
    pub fn add_u64(&mut self, sample: u64) {
        self.add(&sample.to_le_bytes());
    }

    fn fold(&mut self) {
        let mut input = Vec::with_capacity(32 + self.pending.len());
        input.extend_from_slice(&self.state);
        input.extend_from_slice(&self.pending);
        self.state = sha256::digest(&input);
        self.pending.clear();
    }

    /// Samples added since the last `take`.
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// The pool's digest, for seeding; the pool moves on, so the same
    /// bytes are never handed out twice.
    pub fn take(&mut self) -> [u8; 32] {
        self.fold();
        let out = sha256::hmac(&self.state, b"PandaGen pool output");
        self.state = sha256::hmac(&self.state, b"PandaGen pool next");
        self.samples = 0;
        out
    }
}

/// HMAC_DRBG with SHA-256 (NIST SP 800-90A, section 10.1.2).
#[derive(Clone)]
pub struct HmacDrbg {
    key: [u8; 32],
    value: [u8; 32],
    reseed_counter: u64,
}

impl HmacDrbg {
    /// Instantiate from `entropy`, a `nonce` and a `personalization`
    /// string, as the standard lays out.
    pub fn new(entropy: &[u8], nonce: &[u8], personalization: &[u8]) -> Self {
        let mut drbg = Self {
            key: [0x00; 32],
            value: [0x01; 32],
            reseed_counter: 1,
        };
        let mut seed = Vec::with_capacity(entropy.len() + nonce.len() + personalization.len());
        seed.extend_from_slice(entropy);
        seed.extend_from_slice(nonce);
        seed.extend_from_slice(personalization);
        drbg.update(&seed);
        drbg
    }

    fn update(&mut self, data: &[u8]) {
        for round in [0x00u8, 0x01] {
            if round == 0x01 && data.is_empty() {
                break;
            }
            let mut input = Vec::with_capacity(33 + data.len());
            input.extend_from_slice(&self.value);
            input.push(round);
            input.extend_from_slice(data);
            self.key = sha256::hmac(&self.key, &input);
            self.value = sha256::hmac(&self.key, &self.value);
        }
    }

    /// Fold `entropy` (and optional `additional` input) into the state.
    pub fn reseed(&mut self, entropy: &[u8], additional: &[u8]) {
        let mut seed = Vec::with_capacity(entropy.len() + additional.len());
        seed.extend_from_slice(entropy);
        seed.extend_from_slice(additional);
        self.update(&seed);
        self.reseed_counter = 1;
    }

    /// Whether the standard requires a reseed before the next generate.
    pub fn needs_reseed(&self) -> bool {
        self.reseed_counter > RESEED_INTERVAL
    }

    /// Fill `out` (at most `MAX_REQUEST` bytes), with optional
    /// `additional` input. Returns false, filling nothing, when a reseed
    /// is due or the request is too large.
    pub fn generate(&mut self, out: &mut [u8], additional: &[u8]) -> bool {
        if self.needs_reseed() || out.len() > MAX_REQUEST {
            return false;
        }
        if !additional.is_empty() {
            self.update(additional);
        }
        for chunk in out.chunks_mut(32) {
            self.value = sha256::hmac(&self.key, &self.value);
            chunk.copy_from_slice(&self.value[..chunk.len()]);
        }
        self.update(additional);
        self.reseed_counter += 1;
        true
    }
}

/// The machine's random numbers: a DRBG kept fresh from a pool.
#[derive(Clone)]
pub struct Rng {
    drbg: HmacDrbg,
    pool: Pool,
    /// Bytes handed out, for the curious (`random` in the Terminal).
    pub generated: u64,
    pub reseeds: u64,
}

impl Rng {
    /// Seeded from what `pool` gathered, with `nonce` and `personalization`
    /// (things that differ between machines and boots, not secrets).
    pub fn new(mut pool: Pool, nonce: &[u8], personalization: &[u8]) -> Self {
        let seed = pool.take();
        Self {
            drbg: HmacDrbg::new(&seed, nonce, personalization),
            pool,
            generated: 0,
            reseeds: 0,
        }
    }

    /// Mix a sample into the pool (an interrupt's time-stamp).
    pub fn add_event(&mut self, sample: u64) {
        self.pool.add_u64(sample);
    }

    /// Mix bytes into the pool (the CPU's generator, a packet's timing).
    pub fn add_bytes(&mut self, bytes: &[u8]) {
        self.pool.add(bytes);
    }

    /// Fill `out` with random bytes, folding fresh samples in first when
    /// enough have arrived.
    pub fn fill(&mut self, out: &mut [u8]) {
        if self.pool.samples() >= SAMPLES_PER_RESEED || self.drbg.needs_reseed() {
            let fresh = self.pool.take();
            self.drbg.reseed(&fresh, b"");
            self.reseeds += 1;
        }
        for chunk in out.chunks_mut(MAX_REQUEST) {
            if !self.drbg.generate(chunk, b"") {
                let fresh = self.pool.take();
                self.drbg.reseed(&fresh, b"");
                self.reseeds += 1;
                let ok = self.drbg.generate(chunk, b"");
                debug_assert!(ok);
            }
        }
        self.generated += out.len() as u64;
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.fill(&mut b);
        u64::from_le_bytes(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// NIST CAVP HMAC_DRBG, SHA-256, no prediction resistance, 256-bit
    /// entropy, 128-bit nonce, no personalization or additional input,
    /// COUNT = 0: instantiate, generate 1024 bits, generate 1024 bits,
    /// and the second is the answer.
    #[test]
    fn hmac_drbg_matches_the_nist_vector() {
        let entropy = hex("ca851911349384bffe89de1cbdc46e6831e44d34a4fb935ee285dd14b71a7488");
        let nonce = hex("659ba96c601dc69fc902940805ec0ca8");
        let mut drbg = HmacDrbg::new(&entropy, &nonce, b"");
        let mut out = [0u8; 128];
        assert!(drbg.generate(&mut out, b""));
        assert!(drbg.generate(&mut out, b""));
        assert_eq!(
            out.to_vec(),
            hex(concat!(
                "e528e9abf2dece54d47c7e75e5fe302149f817ea9fb4bee6f4199697d04d5b89",
                "d54fbb978a15b5c443c9ec21036d2460b6f73ebad0dc2aba6e624abf07745bc1",
                "07694bb7547bb0995f70de25d6b29e2d3011bb19d27676c07162c8b5ccde0668",
                "961df86803482cb37ed6d5c0bb8d50cf1f50d476aa0458bdaba806f48be9dcb8",
            ))
        );
    }

    #[test]
    fn a_reseed_or_a_different_seed_changes_everything_after() {
        let mut a = HmacDrbg::new(b"same entropy here, 32 bytes long", b"nonce", b"");
        let mut b = a.clone();
        let (mut x, mut y) = ([0u8; 32], [0u8; 32]);
        a.generate(&mut x, b"");
        b.generate(&mut y, b"");
        assert_eq!(x, y, "deterministic");
        b.reseed(b"fresh", b"");
        a.generate(&mut x, b"");
        b.generate(&mut y, b"");
        assert_ne!(x, y);
        let mut too_big = alloc::vec![0u8; MAX_REQUEST + 1];
        assert!(!a.generate(&mut too_big, b""), "one request is bounded");
    }

    #[test]
    fn the_pool_depends_on_every_sample_and_never_repeats_itself() {
        let mut p = Pool::new();
        let mut q = Pool::new();
        for i in 0..100u64 {
            p.add_u64(i);
            q.add_u64(if i == 57 { 58 } else { i });
        }
        let (a, b) = (p.take(), q.take());
        assert_ne!(a, b, "one sample different");
        assert_ne!(p.take(), a, "taking twice gives two answers");
        assert_eq!(p.samples(), 0);
    }

    #[test]
    fn the_rng_reseeds_from_fresh_samples_and_fills_any_length() {
        let mut pool = Pool::new();
        pool.add_u64(12345);
        let mut rng = Rng::new(pool, b"mac", b"PandaGen");
        let mut first = [0u8; 16];
        rng.fill(&mut first);
        for i in 0..SAMPLES_PER_RESEED {
            rng.add_event(i * 7919);
        }
        let mut second = [0u8; 16];
        rng.fill(&mut second);
        assert_ne!(first, second);
        assert_eq!(rng.reseeds, 1);
        let mut big = alloc::vec![0u8; MAX_REQUEST * 2 + 5];
        rng.fill(&mut big);
        assert!(big.iter().any(|b| *b != 0));
        // No run of the same byte a quarter of the buffer long.
        assert!(big.windows(64).all(|w| w.iter().any(|b| *b != w[0])));
    }
}
