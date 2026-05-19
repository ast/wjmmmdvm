//! Calls into the MD-380 firmware's AMBE+2 codec functions.
//!
//! The firmware exposes two functions we care about (symbol map from
//! `applet/src/symbols_d02.032` in the md380tools repo):
//!
//! - `ambe_decode_wav` at **`0x08051249`** — decodes one 49-bit AMBE
//!   frame to 80 PCM samples (one of the two timeslots in a DMR
//!   superframe; called twice to produce 160 samples).
//! - `ambe_encode_thing` at **`0x08050d91`** — encodes 80 PCM samples
//!   to one 49-bit AMBE frame.
//!
//! Addresses have their LSB set to 1 because they're Thumb mode.
//! The actual instruction sits at `addr & ~1`; the LSB tells the
//! ARM CPU to switch to Thumb mode on BX/BLX.
//!
//! The codec uses several RAM-resident buffers and context structs
//! that the firmware allocates at fixed addresses:
//!
//! | Symbol               | Address       | Purpose                                |
//! |----------------------|---------------|----------------------------------------|
//! | `ambe_inbuffer`      | `0x20011c8e`  | 49 shorts, one bit per voice bit       |
//! | `ambe_outbuffer0`    | `0x20011aa8`  | 80 shorts, decoded PCM (timeslot 0)    |
//! | `ambe_outbuffer1`    | `0x20011b48`  | 80 shorts, decoded PCM (timeslot 1)    |
//! | `ambe_mystery`       | `0x20011224`  | Decode context structure               |
//! | `wav_inbuffer0`      | `0x2000de82`  | 80 shorts, PCM to encode (timeslot 0)  |
//! | `wav_inbuffer1`      | `0x2000df22`  | 80 shorts, PCM to encode (timeslot 1)  |
//! | `ambe_outbuffer`     | `0x2000dfc6`  | Encoder output bits                    |
//! | `ambe_en_mystery`    | `0x2000c730`  | Encode context structure               |

use crate::firmware::Firmware;

// Function pointers as raw addresses. Real call addresses are these
// minus 1 (the LSB is the Thumb mode flag); transmuting to a function
// pointer preserves the LSB and the BLX instruction handles the mode
// switch correctly.
//
// All these symbols are only referenced from the ARM-gated impl
// blocks below — cfg-gate them so host (x86) builds stay warning-free.
#[cfg(target_arch = "arm")]
const AMBE_DECODE_WAV: usize = 0x0805_1249;
#[cfg(target_arch = "arm")]
const AMBE_ENCODE_THING: usize = 0x0805_0d91;

#[cfg(target_arch = "arm")]
const AMBE_INBUFFER: usize = 0x2001_1c8e;
#[cfg(target_arch = "arm")]
const AMBE_OUTBUFFER0: usize = 0x2001_1aa8;
#[cfg(target_arch = "arm")]
const AMBE_OUTBUFFER1: usize = 0x2001_1b48;
#[cfg(target_arch = "arm")]
const AMBE_MYSTERY: usize = 0x2001_1224;

#[cfg(target_arch = "arm")]
const WAV_INBUFFER0: usize = 0x2000_de82;
#[cfg(target_arch = "arm")]
const WAV_INBUFFER1: usize = 0x2000_df22;
#[cfg(target_arch = "arm")]
const AMBE_OUTBUFFER: usize = 0x2000_dfc6;
#[cfg(target_arch = "arm")]
const AMBE_EN_MYSTERY: usize = 0x2000_c730;

/// 160 samples = one 20 ms PCM frame at 8 kHz.
pub const FRAME_PCM_SAMPLES: usize = 160;
/// 8 bytes per AMBE frame in md380-emu's .amb format (49 voice bits +
/// 1 status byte = 8 bytes).
pub const AMBE_FRAME_BYTES: usize = 8;

// The constants below are only referenced from the ARM-gated impl
// blocks; cfg-gate them to avoid dead_code warnings on host builds.
#[cfg(target_arch = "arm")]
const HALF_FRAME_PCM_SAMPLES: usize = 80;
#[cfg(target_arch = "arm")]
const AMBE_BITS: usize = 49;

#[cfg(target_arch = "arm")]
type AmbeDecodeWavFn = unsafe extern "C" fn(
    wavbuffer: *mut i16,
    eighty: i32,
    bitbuffer: *mut i16,
    a4: i32,
    a5: i32,
    a6: i16,
    a7: i32,
) -> i32;

#[cfg(target_arch = "arm")]
type AmbeEncodeThingFn = unsafe extern "C" fn(
    bitbuffer: *mut i16,
    a2: i32,
    wavbuffer: *mut i16,
    eighty: i32,
    a5: i32,
    timeslot: i16,
    a7: i32,
    context: u32,
) -> i32;

/// Owns the firmware mapping and provides encode/decode methods.
///
/// `Send` (can be transferred to another thread) but **not** `Sync`
/// (cannot be shared by reference for concurrent calls): the firmware
/// keeps shared mutable state at fixed memory addresses, so a codec
/// must be used by one thread at a time. The server transfers
/// ownership to a dedicated worker thread and queues requests
/// through an mpsc channel.
pub struct Md380Codec {
    _firmware: Firmware,
}

impl Md380Codec {
    pub fn new(firmware: Firmware) -> Self {
        Self {
            _firmware: firmware,
        }
    }

    /// Decode one 8-byte AMBE frame (49 voice bits, MSB-first the
    /// same way md380-emu's .amb format packs them) into 160 PCM
    /// samples.
    #[cfg(target_arch = "arm")]
    pub fn decode(&mut self, ambe_frame: &[u8; AMBE_FRAME_BYTES]) -> [i16; FRAME_PCM_SAMPLES] {
        // 1. Unpack the 49 bits into the firmware's bit buffer (one
        //    bit per `i16` slot — that's how md380-emu's source
        //    documents it).
        let bitbuf = AMBE_INBUFFER as *mut i16;
        unsafe {
            let mut idx = 0usize;
            // Bytes 1..7 contribute 48 bits, MSB-first.
            for i in 1..7 {
                let byte = ambe_frame[i];
                for j in 0..8 {
                    *bitbuf.add(idx) = ((byte >> (7 - j)) & 1) as i16;
                    idx += 1;
                }
            }
            // The 49th bit is the LSB of byte 7.
            *bitbuf.add(idx) = (ambe_frame[7] & 1) as i16;
            debug_assert_eq!(idx + 1, AMBE_BITS);
        }

        // 2. Call ambe_decode_wav twice — once for each 80-sample
        //    half (timeslot 0, then 1).
        let decode: AmbeDecodeWavFn = unsafe { std::mem::transmute(AMBE_DECODE_WAV) };
        unsafe {
            decode(
                AMBE_OUTBUFFER0 as *mut i16,
                HALF_FRAME_PCM_SAMPLES as i32,
                bitbuf,
                0,
                0,
                0,
                AMBE_MYSTERY as i32,
            );
            decode(
                AMBE_OUTBUFFER1 as *mut i16,
                HALF_FRAME_PCM_SAMPLES as i32,
                bitbuf,
                0,
                0,
                1,
                AMBE_MYSTERY as i32,
            );
        }

        // 3. Concatenate the two halves into one 160-sample buffer.
        let mut out = [0i16; FRAME_PCM_SAMPLES];
        unsafe {
            std::ptr::copy_nonoverlapping(
                AMBE_OUTBUFFER0 as *const i16,
                out.as_mut_ptr(),
                HALF_FRAME_PCM_SAMPLES,
            );
            std::ptr::copy_nonoverlapping(
                AMBE_OUTBUFFER1 as *const i16,
                out.as_mut_ptr().add(HALF_FRAME_PCM_SAMPLES),
                HALF_FRAME_PCM_SAMPLES,
            );
        }
        out
    }

    /// Encode 160 PCM samples to an 8-byte AMBE frame.
    #[cfg(target_arch = "arm")]
    pub fn encode(&mut self, pcm: &[i16; FRAME_PCM_SAMPLES]) -> [u8; AMBE_FRAME_BYTES] {
        // 1. Split PCM into the two 80-sample input buffers.
        unsafe {
            std::ptr::copy_nonoverlapping(
                pcm.as_ptr(),
                WAV_INBUFFER0 as *mut i16,
                HALF_FRAME_PCM_SAMPLES,
            );
            std::ptr::copy_nonoverlapping(
                pcm.as_ptr().add(HALF_FRAME_PCM_SAMPLES),
                WAV_INBUFFER1 as *mut i16,
                HALF_FRAME_PCM_SAMPLES,
            );
        }

        // 2. Call ambe_encode_thing once for each timeslot. The
        //    firmware writes the 49-bit output into ambe_outbuffer
        //    (50 shorts), bit per slot.
        let encode: AmbeEncodeThingFn = unsafe { std::mem::transmute(AMBE_ENCODE_THING) };
        unsafe {
            encode(
                AMBE_OUTBUFFER as *mut i16,
                0,
                WAV_INBUFFER0 as *mut i16,
                0x50,
                0x1840,
                0,
                0x2000,
                AMBE_EN_MYSTERY as u32,
            );
            encode(
                AMBE_OUTBUFFER as *mut i16,
                0,
                WAV_INBUFFER1 as *mut i16,
                0x50,
                0x1840,
                1,
                0x2000,
                AMBE_EN_MYSTERY as u32,
            );
        }

        // 3. Pack the 49 output bits into 8 bytes matching md380-emu's
        //    .amb frame format: byte[0] = status (0), bytes[1..7]
        //    hold 48 bits MSB-first, byte[7] LSB holds bit 49.
        let bits = AMBE_OUTBUFFER as *const i16;
        let mut out = [0u8; AMBE_FRAME_BYTES];
        unsafe {
            for i in 0..6 {
                let mut byte = 0u8;
                for j in 0..8 {
                    let bit = (*bits.add(i * 8 + j) & 1) as u8;
                    byte |= bit << (7 - j);
                }
                out[i + 1] = byte;
            }
            out[7] = (*bits.add(48) & 1) as u8;
        }
        out
    }

    /// Stub for non-ARM builds so callers compile cleanly.
    #[cfg(not(target_arch = "arm"))]
    pub fn decode(&mut self, _ambe_frame: &[u8; AMBE_FRAME_BYTES]) -> [i16; FRAME_PCM_SAMPLES] {
        unimplemented!(
            "Md380Codec::decode requires target_arch = arm. \
             Cross-compile to armv7-unknown-linux-gnueabihf."
        );
    }

    /// Stub for non-ARM builds so callers compile cleanly.
    #[cfg(not(target_arch = "arm"))]
    pub fn encode(&mut self, _pcm: &[i16; FRAME_PCM_SAMPLES]) -> [u8; AMBE_FRAME_BYTES] {
        unimplemented!(
            "Md380Codec::encode requires target_arch = arm. \
             Cross-compile to armv7-unknown-linux-gnueabihf."
        );
    }
}
