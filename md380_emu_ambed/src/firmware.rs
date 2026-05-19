//! Loads the MD-380 firmware blob and RAM core image (embedded at
//! build time via `include_bytes!`) into the process address space at
//! the fixed addresses the firmware code expects.
//!
//! This mirrors what Travis Goodspeed's md380-emu does (PoC||GTFO 13:5
//! "Decoding AMBE+2 in MD380 Firmware in Linux"):
//!
//! - **Firmware** (=D002.032.img=, ~972 KiB) → mapped at `0x0800C000`
//!   with `PROT_READ|PROT_EXEC`. The ARM Thumb-2 code lives here and
//!   the firmware's internal jump tables / function pointers all
//!   assume this exact address.
//!
//! - **RAM core** (=d02032-core.img=, 128 KiB) → mapped at
//!   `0x20000000` with `PROT_READ|PROT_WRITE`. Contains the codec's
//!   initialised data structures (audio buffers, the mysterious
//!   "context" structs at `ambe_mystery=0x20011224` and
//!   `ambe_en_mystery=0x2000c730`, etc).
//!
//! - **TCRAM** (= "Tightly-Coupled RAM" on the STM32) → 64 KiB
//!   zero-initialised at `0x10000000` with `PROT_READ|PROT_WRITE`.
//!   The firmware uses this for scratch; we just give it
//!   anonymous-mapped memory.
//!
//! All three mappings use `MAP_FIXED`, which is fragile in the
//! presence of ASLR — if anything else has already been mapped at
//! those addresses, loading fails. In practice this works on Pi OS.
//! If it doesn't, try running under `setarch armv7l -R`.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};

use thiserror::Error;

/// Process-wide flag that prevents two concurrent [`Firmware`]
/// instances. The firmware's RAM region at `0x20000000` holds shared
/// mutable state — letting a second `Firmware::load()` succeed would
/// `MAP_FIXED`-clobber the running instance's memory and corrupt
/// in-flight codec calls. Cleared again when the live `Firmware`
/// drops, so a subsequent reload works.
static FIRMWARE_LOADED: AtomicBool = AtomicBool::new(false);

/// Embedded at build time. The bytes are gitignored — see
/// `firmware/README.org` for where to get them.
const FIRMWARE_BYTES: &[u8] = include_bytes!("../firmware/D002.032.img");
const CORE_BYTES: &[u8] = include_bytes!("../firmware/d02032-core.img");

const FIRMWARE_ADDR: usize = 0x0800_C000;
const FIRMWARE_LEN: usize = 994_304; // ~972 KiB; matches md380-emu's hard-coded length

const RAM_ADDR: usize = 0x2000_0000;
const RAM_LEN: usize = 0x0002_0000; // 128 KiB

const TCRAM_ADDR: usize = 0x1000_0000;
const TCRAM_LEN: usize = 0x0001_0000; // 64 KiB

#[derive(Debug, Error)]
pub enum FirmwareError {
    #[error(
        "mmap at 0x{addr:x} (length {len} bytes) failed: {errno}. \
         Most likely something is already mapped at that address. \
         Try disabling ASLR for this process: \
         `setarch $(uname -m) -R ./md380-emu-ambed ...`"
    )]
    Mmap {
        addr: usize,
        len: usize,
        errno: std::io::Error,
    },
    #[error("mprotect at 0x{addr:x} failed: {errno}")]
    Mprotect {
        addr: usize,
        errno: std::io::Error,
    },
    #[error(
        "this binary must be a 32-bit ARM build to execute MD-380 firmware (Thumb-2 code) \
         natively. Detected target_arch = `{0}`. Cross-compile for armv7-unknown-linux-gnueabihf."
    )]
    // Only constructed on non-ARM builds; harmless on ARM.
    #[allow(dead_code)]
    WrongArch(&'static str),
    #[error(
        "another Firmware instance is already live in this process. Only one can exist at a time; \
         drop the existing Firmware (and its Md380Codec) before loading again."
    )]
    AlreadyLoaded,
}

/// Owned firmware + RAM mappings. While this struct lives, the
/// MD-380 firmware code is executable at `0x0800C000` and the RAM
/// region is read/write at `0x20000000`. Dropping it unmaps both.
pub struct Firmware {
    _firmware: MappedRegion,
    _ram: MappedRegion,
    _tcram: MappedRegion,
}

// SAFETY: the mmap regions are process-global at fixed addresses, so
// any thread can call into them. The codec wrapper is responsible for
// serialising concurrent calls (one worker thread per Firmware
// instance is the contract).
unsafe impl Send for Firmware {}

impl Firmware {
    /// Map the embedded firmware + RAM core into the process at the
    /// fixed addresses the firmware code expects.
    ///
    /// Only one `Firmware` may exist per process. The second call
    /// returns [`FirmwareError::AlreadyLoaded`]. Drop the existing
    /// `Firmware` (and any [`crate::codec::Md380Codec`] holding it)
    /// before calling again.
    pub fn load() -> Result<Self, FirmwareError> {
        check_arch()?;

        // Claim the singleton slot. If another Firmware is already
        // live, bail before touching mmap so we don't clobber it.
        if FIRMWARE_LOADED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(FirmwareError::AlreadyLoaded);
        }

        // From here on, any error path must release the singleton
        // slot so a retry is possible.
        Self::load_inner().inspect_err(|_| {
            FIRMWARE_LOADED.store(false, Ordering::SeqCst);
        })
    }

    fn load_inner() -> Result<Self, FirmwareError> {
        // Firmware: writable+exec while we populate it, then drop
        // write permission so accidental writes trap.
        let firmware = MappedRegion::new_fixed(
            FIRMWARE_ADDR,
            FIRMWARE_LEN,
            libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
        )?;
        firmware.write_padded(FIRMWARE_BYTES);
        firmware.set_protection(libc::PROT_READ | libc::PROT_EXEC)?;

        let ram = MappedRegion::new_fixed(
            RAM_ADDR,
            RAM_LEN,
            libc::PROT_READ | libc::PROT_WRITE,
        )?;
        ram.write_padded(CORE_BYTES);

        // TCRAM: zero-initialised scratch. Anonymous pages are
        // already zeroed by the kernel.
        let tcram = MappedRegion::new_fixed(
            TCRAM_ADDR,
            TCRAM_LEN,
            libc::PROT_READ | libc::PROT_WRITE,
        )?;

        tracing::info!(
            target: "md380_emu_ambed::firmware",
            firmware_addr = format!("0x{:08x}", FIRMWARE_ADDR),
            ram_addr = format!("0x{:08x}", RAM_ADDR),
            tcram_addr = format!("0x{:08x}", TCRAM_ADDR),
            firmware_bytes = FIRMWARE_BYTES.len(),
            core_bytes = CORE_BYTES.len(),
            "firmware + RAM mapped"
        );

        Ok(Self {
            _firmware: firmware,
            _ram: ram,
            _tcram: tcram,
        })
    }
}

impl Drop for Firmware {
    fn drop(&mut self) {
        // Release the singleton slot so a future load() can succeed.
        // The MappedRegion fields are dropped (and munmap'd) right
        // after this explicit drop returns.
        FIRMWARE_LOADED.store(false, Ordering::SeqCst);
    }
}

/// One mmap region. Unmaps on drop.
struct MappedRegion {
    ptr: NonNull<u8>,
    len: usize,
}

impl MappedRegion {
    fn new_fixed(addr: usize, len: usize, prot: i32) -> Result<Self, FirmwareError> {
        // SAFETY: requesting MAP_FIXED at a specific addr is unsafe in
        // the sense that it can clobber existing mappings. We accept
        // that explicitly — this binary owns the low ARM address
        // space at runtime.
        let raw = unsafe {
            libc::mmap(
                addr as *mut libc::c_void,
                len,
                prot,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED,
                -1,
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(FirmwareError::Mmap {
                addr,
                len,
                errno: std::io::Error::last_os_error(),
            });
        }
        if raw as usize != addr {
            return Err(FirmwareError::Mmap {
                addr,
                len,
                errno: std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!("kernel returned 0x{:x} instead of 0x{:x}", raw as usize, addr),
                ),
            });
        }
        Ok(Self {
            // SAFETY: raw is non-null on mmap success.
            ptr: unsafe { NonNull::new_unchecked(raw.cast()) },
            len,
        })
    }

    /// Copy `src` into the region. If `src` is shorter than the
    /// region, the tail is left zero (anonymous mapping default).
    /// If `src` is longer, the excess is dropped.
    fn write_padded(&self, src: &[u8]) {
        let n = src.len().min(self.len);
        // SAFETY: ptr is valid, len bytes are mmap'd writable, src is valid.
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), self.ptr.as_ptr(), n);
        }
    }

    fn set_protection(&self, prot: i32) -> Result<(), FirmwareError> {
        let rc = unsafe { libc::mprotect(self.ptr.as_ptr().cast(), self.len, prot) };
        if rc != 0 {
            return Err(FirmwareError::Mprotect {
                addr: self.ptr.as_ptr() as usize,
                errno: std::io::Error::last_os_error(),
            });
        }
        Ok(())
    }
}

impl Drop for MappedRegion {
    fn drop(&mut self) {
        // SAFETY: ptr + len are what we mmap'd; munmap of a valid mmap is safe.
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast(), self.len);
        }
    }
}

fn check_arch() -> Result<(), FirmwareError> {
    #[cfg(target_arch = "arm")]
    {
        Ok(())
    }
    #[cfg(not(target_arch = "arm"))]
    {
        Err(FirmwareError::WrongArch(std::env::consts::ARCH))
    }
}
