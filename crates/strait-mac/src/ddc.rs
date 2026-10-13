//! DDC/CI to the monitor: the Mac selects which input the monitor shows.
//!
//! On Apple silicon each external display link has a `DCPAVServiceProxy`;
//! DDC travels on its I2C bus. Only links that pass DDC answer a read, so
//! the monitor is the external link whose input source can be read.

use std::thread::sleep;
use std::time::Duration;

use core_foundation_sys::base::{CFEqual, CFRelease, kCFAllocatorDefault};

use crate::sys::*;

const CHIP: u32 = 0x37;
const OFFSET: u32 = 0x51;
const HOST: u8 = 0x51;
const DISPLAY: u8 = 0x6E;
/// VCP feature: input source.
pub const INPUT_SOURCE: u8 = 0x60;
/// Time a monitor needs to answer a request, per DDC/CI.
const REPLY_DELAY: Duration = Duration::from_millis(50);

pub struct Monitor(IOAVServiceRef);

// The service is an immutable CF object; IOAVService calls are made from
// one thread at a time through `&mut self`.
unsafe impl Send for Monitor {}

impl Drop for Monitor {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0.cast()) };
    }
}

fn checksum(first: u8, bytes: &[u8]) -> u8 {
    bytes.iter().fold(first ^ HOST, |acc, b| acc ^ b)
}

impl Monitor {
    /// The external display that answers DDC, if any.
    pub fn find() -> Option<Monitor> {
        let mut iter = 0;
        let matching = unsafe { IOServiceMatching(c"DCPAVServiceProxy".as_ptr()) };
        if unsafe { IOServiceGetMatchingServices(0, matching, &mut iter) } != 0 {
            return None;
        }
        let location = CfString::new("Location");
        let external = CfString::new("External");
        let mut found = None;
        loop {
            let service = unsafe { IOIteratorNext(iter) };
            if service == 0 {
                break;
            }
            if found.is_none() {
                let value = unsafe { IORegistryEntryCreateCFProperty(service, location.0, kCFAllocatorDefault, 0) };
                let is_external = !value.is_null() && unsafe { CFEqual(value, external.0.cast()) } != 0;
                if !value.is_null() {
                    unsafe { CFRelease(value) };
                }
                if is_external {
                    let av = unsafe { IOAVServiceCreateWithService(kCFAllocatorDefault, service) };
                    if !av.is_null() {
                        let mut monitor = Monitor(av);
                        if monitor.read(INPUT_SOURCE).is_some() {
                            found = Some(monitor);
                        }
                    }
                }
            }
            unsafe { IOObjectRelease(service) };
        }
        unsafe { IOObjectRelease(iter) };
        found
    }

    /// Current value of a VCP feature.
    pub fn read(&mut self, feature: u8) -> Option<u16> {
        let request = [0x82, 0x01, feature];
        let mut packet = request.to_vec();
        packet.push(checksum(DISPLAY, &request));
        for _ in 0..3 {
            if unsafe { IOAVServiceWriteI2C(self.0, CHIP, OFFSET, packet.as_ptr(), packet.len() as u32) } != 0 {
                sleep(REPLY_DELAY);
                continue;
            }
            sleep(REPLY_DELAY);
            let mut reply = [0u8; 12];
            if unsafe { IOAVServiceReadI2C(self.0, CHIP, OFFSET, reply.as_mut_ptr(), reply.len() as u32) } != 0 {
                continue;
            }
            // Source address, length, reply opcode, result, feature, type,
            // maximum (2 bytes), current (2 bytes), checksum.
            if reply[2] == 0x02 && reply[3] == 0 && reply[4] == feature {
                return Some(u16::from_be_bytes([reply[8], reply[9]]));
            }
        }
        None
    }

    /// Sets a VCP feature and confirms the monitor took it.
    pub fn write(&mut self, feature: u8, value: u16) -> bool {
        let [hi, lo] = value.to_be_bytes();
        let request = [0x84, 0x03, feature, hi, lo];
        let mut packet = request.to_vec();
        packet.push(checksum(DISPLAY, &request));
        for _ in 0..2 {
            if unsafe { IOAVServiceWriteI2C(self.0, CHIP, OFFSET, packet.as_ptr(), packet.len() as u32) } != 0 {
                sleep(REPLY_DELAY);
                continue;
            }
            // Switching inputs keeps the monitor busy for a moment.
            for _ in 0..10 {
                sleep(REPLY_DELAY * 4);
                if self.read(feature) == Some(value) {
                    return true;
                }
            }
        }
        false
    }
}
