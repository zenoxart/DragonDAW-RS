//! Denormal protection. Decaying filters, reverbs and envelopes approach zero through "denormal" floats,
//! which x86 and ARM handle 10 – 100× slower than normal numbers: an effect's tail after the sound stops
//! can cost more than the whole block budget (random CPU overloads in quiet passages). Flushing them to
//! zero (x86: FTZ + DAZ, ARM: FZ) is inaudible – it only touches values below about -750 dBFS.
//!
//! The mode is per thread: [`flush_to_zero`] is called at the start of every render and in the mixer's
//! worker threads. It costs a few nanoseconds.

/// Sets flush-to-zero (and denormals-are-zero on x86) for the calling thread.
#[inline]
pub fn flush_to_zero() {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    // SAFETY: reads and writes the SSE control register of this thread; only the FTZ (bit 15) and DAZ
    // (bit 6) flags change, which affect results only for denormal values.
    unsafe {
        let mut csr: u32 = 0;
        std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr as *mut u32, options(nostack, preserves_flags));
        let want = csr | 0x8040;
        if want != csr {
            std::arch::asm!("ldmxcsr [{}]", in(reg) &want as *const u32, options(nostack, preserves_flags));
        }
    }
    #[cfg(target_arch = "aarch64")]
    // SAFETY: sets the FZ bit (24) of this thread's floating-point control register.
    unsafe {
        let mut fpcr: u64;
        std::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack, preserves_flags));
        if fpcr & (1 << 24) == 0 {
            fpcr |= 1 << 24;
            std::arch::asm!("msr fpcr, {}", in(reg) fpcr, options(nomem, nostack, preserves_flags));
        }
    }
}

/// Whether flush-to-zero is on for the calling thread.
pub fn is_on() -> bool {
    let tiny = std::hint::black_box(f32::MIN_POSITIVE);
    std::hint::black_box(tiny * std::hint::black_box(0.5)) == 0.0
}

#[cfg(test)]
mod tests {
    #[test]
    fn flushes_denormals_on_this_thread() {
        std::thread::spawn(|| {
            assert!(!super::is_on(), "off by default");
            super::flush_to_zero();
            assert!(super::is_on());
        })
        .join()
        .unwrap();
    }
}
