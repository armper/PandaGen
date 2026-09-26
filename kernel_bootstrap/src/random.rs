//! The machine's random numbers (SEC-030): the `entropy` crate's
//! generator, seeded at boot and stirred as the machine runs.
//!
//! Seeding takes what the machine offers. The CPU's own generator
//! (RDSEED, else RDRAND) where CPUID says there is one; always, a few
//! thousand readings of the time-stamp counter around work whose timing
//! varies -- caches, the interrupt that lands mid-loop, the emulator's
//! scheduling under QEMU. After that, keys, mouse movements and network
//! frames stir the pool with the moment they are handled, and the
//! generator folds the pool in as it fills.
//!
//! Salts, DNS query ids, TCP's initial sequence numbers and TLS keys come
//! from here. None of them used to: salts and ids were derived from the
//! clock, and sequence numbers counted up from 0x1000.

use core::sync::atomic::{AtomicBool, Ordering};
use entropy::{Pool, Rng};

static RNG: hal_x86_64::SpinLock<Option<Rng>> = hal_x86_64::SpinLock::new(None);
/// The CPU's generator was there to seed from.
static HARDWARE: AtomicBool = AtomicBool::new(false);

fn tsc() -> u64 {
    // SAFETY: RDTSC reads a counter; it has no side effects.
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// 64 bits from RDSEED or RDRAND, if the CPU has one and it answers.
fn cpu_random() -> Option<u64> {
    let (leaf1, leaf7) = (
        core::arch::x86_64::__cpuid(1),
        core::arch::x86_64::__cpuid_count(7, 0),
    );
    let has_rdseed = leaf7.ebx & (1 << 18) != 0;
    let has_rdrand = leaf1.ecx & (1 << 30) != 0;
    for _ in 0..16 {
        let mut value: u64;
        let ok: u8;
        // SAFETY: executed only when CPUID says the instruction exists;
        // it writes a register and the carry flag.
        unsafe {
            if has_rdseed {
                core::arch::asm!("rdseed {v}", "setc {ok}", v = out(reg) value, ok = out(reg_byte) ok);
            } else if has_rdrand {
                core::arch::asm!("rdrand {v}", "setc {ok}", v = out(reg) value, ok = out(reg_byte) ok);
            } else {
                return None;
            }
        }
        if ok == 1 {
            return Some(value);
        }
    }
    None
}

/// Timing jitter: the counter read around a little work whose length
/// depends on the last reading.
fn jitter(pool: &mut Pool, rounds: usize) {
    let mut scratch = [0u64; 64];
    let mut last = tsc();
    for i in 0..rounds {
        let spins = 16 + (last as usize & 31);
        for j in 0..spins {
            let at = (j * 7 + i) & 63;
            // SAFETY: `at` is in bounds; volatile so the work is done.
            unsafe {
                let p = scratch.as_mut_ptr().add(at);
                core::ptr::write_volatile(p, core::ptr::read_volatile(p) ^ last);
            }
        }
        let now = tsc();
        pool.add_u64(now.wrapping_sub(last) ^ now.rotate_left(29));
        last = now;
    }
}

/// Seed the generator. `personal` is what differs between machines and
/// boots (a MAC, the time) -- not secret, but it keeps two machines that
/// saw the same timings apart. Returns whether the CPU's generator helped.
pub fn seed(personal: &[u8]) -> bool {
    let mut pool = Pool::new();
    let mut hardware = false;
    for _ in 0..8 {
        if let Some(v) = cpu_random() {
            pool.add_u64(v);
            hardware = true;
        }
    }
    jitter(&mut pool, 4096);
    let nonce = tsc().to_le_bytes();
    *RNG.lock() = Some(Rng::new(pool, &nonce, personal));
    HARDWARE.store(hardware, Ordering::Relaxed);
    hardware
}

/// Fill `buf` with random bytes (seeding first, if boot has not yet).
pub fn fill(buf: &mut [u8]) {
    {
        let mut rng = RNG.lock();
        if let Some(rng) = rng.as_mut() {
            rng.fill(buf);
            return;
        }
    }
    seed(b"PandaGen late seed");
    if let Some(rng) = RNG.lock().as_mut() {
        rng.fill(buf);
    }
}

pub fn u16() -> u16 {
    let mut b = [0u8; 2];
    fill(&mut b);
    u16::from_le_bytes(b)
}

pub fn u32() -> u32 {
    let mut b = [0u8; 4];
    fill(&mut b);
    u32::from_le_bytes(b)
}

/// An event happened now (a key, a mouse byte, a frame): its moment goes
/// into the pool. Never waits: a busy generator just misses this one.
pub fn stir(what: u64) {
    if let Some(mut rng) = RNG.try_lock() {
        if let Some(rng) = rng.as_mut() {
            rng.add_event(tsc() ^ what.rotate_left(17));
        }
    }
}

/// What the Terminal's `random` says: bytes handed out, reseeds, and
/// whether the CPU's generator was there.
pub fn stats() -> Option<(u64, u64, bool)> {
    let rng = RNG.lock();
    rng.as_ref()
        .map(|r| (r.generated, r.reseeds, HARDWARE.load(Ordering::Relaxed)))
}
