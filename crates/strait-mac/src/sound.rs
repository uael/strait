//! Plays Windows' sound on the Mac's current output, AirPods included.
//!
//! The default output unit follows the device chosen in macOS and converts
//! the sample rate. Datagrams arrive with network jitter and on Windows'
//! clock, so a short buffer sits between them and the unit, held near its
//! target by dropping or repeating a single frame now and then.

use std::ffi::c_void;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::sys::*;

const CHANNELS: usize = 2;
/// Sound buffered before playback starts, and kept from then on.
const TARGET_MS: usize = 40;
/// How far the buffer may stray from its target before a frame is dropped
/// or repeated; the two clocks differ by parts per million.
const SLACK_MS: usize = 15;
/// Past this much, the network delivered a burst after a stall, and the
/// buffer drops straight back to its target.
const BURST_MS: usize = 150;
/// Room in the buffer; a datagram that does not fit is dropped.
const CAPACITY_MS: usize = 500;

/// The playback side of the buffer, owned by the output unit's thread.
struct Feed {
    consumer: Consumer<f32>,
    target: usize,
    slack: usize,
    burst: usize,
    /// False until the buffer first reaches its target, and again after it
    /// runs dry, so playback resumes with a full cushion.
    playing: bool,
    last: [f32; CHANNELS],
}

impl Feed {
    fn frames(&self) -> usize {
        self.consumer.slots() / CHANNELS
    }

    fn pop_frame(&mut self) -> Option<[f32; CHANNELS]> {
        if self.frames() == 0 {
            return None;
        }
        let frame = [self.consumer.pop().ok()?, self.consumer.pop().ok()?];
        self.last = frame;
        Some(frame)
    }

    /// Fills `out`, interleaved stereo, from the buffer.
    fn fill(&mut self, out: &mut [f32]) {
        let available = self.frames();
        if !self.playing && available < self.target {
            out.fill(0.0);
            return;
        }
        self.playing = true;
        if available > self.burst {
            for _ in 0..available - self.target {
                self.pop_frame();
            }
        } else if available > self.target + self.slack {
            self.pop_frame();
        }
        let mut frames = out.chunks_exact_mut(CHANNELS);
        if self.frames() < self.target.saturating_sub(self.slack)
            && let Some(first) = frames.next()
        {
            first.copy_from_slice(&self.last);
        }
        for frame in frames {
            match self.pop_frame() {
                Some(f) => frame.copy_from_slice(&f),
                None => {
                    frame.fill(0.0);
                    self.playing = false;
                }
            }
        }
    }
}

extern "C" fn render(
    feed: *mut c_void,
    _flags: *mut u32,
    _time: *const c_void,
    _bus: u32,
    frames: u32,
    data: *mut AudioBufferList,
) -> i32 {
    let feed = unsafe { &mut *feed.cast::<Feed>() };
    let buffer = unsafe { &mut (*data).mBuffers[0] };
    let out = unsafe { std::slice::from_raw_parts_mut(buffer.mData.cast::<f32>(), frames as usize * CHANNELS) };
    feed.fill(out);
    0
}

fn check(status: i32, what: &str) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("{what}: error {status}")) }
}

/// Windows' sound playing on the Mac's default output at `rate` Hz.
pub struct Player {
    unit: AudioUnit,
    producer: Producer<f32>,
    feed: *mut Feed,
    pub rate: u32,
}

impl Player {
    pub fn new(rate: u32) -> Result<Player, String> {
        let per_ms = rate as usize / 1000;
        let (producer, consumer) = RingBuffer::new(CAPACITY_MS * per_ms * CHANNELS);
        let feed = Box::into_raw(Box::new(Feed {
            consumer,
            target: TARGET_MS * per_ms,
            slack: SLACK_MS * per_ms,
            burst: BURST_MS * per_ms,
            playing: false,
            last: [0.0; CHANNELS],
        }));
        let description = AudioComponentDescription {
            componentType: kAudioUnitType_Output,
            componentSubType: kAudioUnitSubType_DefaultOutput,
            componentManufacturer: kAudioUnitManufacturer_Apple,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        let mut unit = std::ptr::null_mut();
        let component = unsafe { AudioComponentFindNext(std::ptr::null_mut(), &description) };
        // The player owns the feed from here, so a failure below frees it in Drop.
        let mut player = Player { unit, producer, feed, rate };
        if component.is_null() {
            return Err("no default output unit".into());
        }
        check(unsafe { AudioComponentInstanceNew(component, &mut unit) }, "open the default output")?;
        player.unit = unit;
        let format = AudioStreamBasicDescription {
            mSampleRate: f64::from(rate),
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
            mBytesPerPacket: (CHANNELS * 4) as u32,
            mFramesPerPacket: 1,
            mBytesPerFrame: (CHANNELS * 4) as u32,
            mChannelsPerFrame: CHANNELS as u32,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        let callback = AURenderCallbackStruct { inputProc: render, inputProcRefCon: feed.cast() };
        unsafe {
            check(
                AudioUnitSetProperty(unit, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input, 0, (&raw const format).cast(), size_of_val(&format) as u32),
                "set the sound format",
            )?;
            check(
                AudioUnitSetProperty(unit, kAudioUnitProperty_SetRenderCallback, kAudioUnitScope_Input, 0, (&raw const callback).cast(), size_of_val(&callback) as u32),
                "set the render callback",
            )?;
            check(AudioUnitInitialize(unit), "initialize the default output")?;
            check(AudioOutputUnitStart(unit), "start the default output")?;
        }
        Ok(player)
    }

    /// Queues interleaved stereo samples; drops them if the buffer is full.
    pub fn push(&mut self, samples: &[i16]) {
        if self.producer.slots() < samples.len() {
            return;
        }
        for &s in samples {
            let _ = self.producer.push(f32::from(s) / 32768.0);
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if !self.unit.is_null() {
            // The unit reads the feed until it is disposed, so it goes first.
            unsafe {
                AudioOutputUnitStop(self.unit);
                AudioUnitUninitialize(self.unit);
                AudioComponentInstanceDispose(self.unit);
            }
        }
        drop(unsafe { Box::from_raw(self.feed) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A feed with a 4-frame target, 1-frame slack and 8-frame burst limit.
    fn feed() -> (Producer<f32>, Feed) {
        let (producer, consumer) = RingBuffer::new(64);
        (producer, Feed { consumer, target: 4, slack: 1, burst: 8, playing: false, last: [0.0; 2] })
    }

    fn push_frames(p: &mut Producer<f32>, from: u16, count: u16) {
        for i in from..from + count {
            p.push(f32::from(i)).unwrap();
            p.push(-f32::from(i)).unwrap();
        }
    }

    fn left(out: &[f32]) -> Vec<f32> {
        out.chunks(2).map(|f| f[0]).collect()
    }

    #[test]
    fn waits_for_the_target_then_plays_in_order() {
        let (mut p, mut f) = feed();
        let mut out = [9.0; 4];
        push_frames(&mut p, 1, 3);
        f.fill(&mut out);
        assert_eq!(out, [0.0; 4], "silent below the target");
        push_frames(&mut p, 4, 2);
        f.fill(&mut out);
        assert_eq!(out, [1.0, -1.0, 2.0, -2.0]);
    }

    #[test]
    fn runs_dry_into_silence_and_waits_again() {
        let (mut p, mut f) = feed();
        push_frames(&mut p, 1, 4);
        let mut out = [9.0; 12];
        f.fill(&mut out);
        assert_eq!(left(&out)[..4], [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(left(&out)[4..], [0.0, 0.0]);
        assert!(!f.playing);
        push_frames(&mut p, 5, 2);
        let mut out = [9.0; 4];
        f.fill(&mut out);
        assert_eq!(out, [0.0; 4], "re-primes before playing again");
    }

    #[test]
    fn drops_a_frame_when_ahead_and_repeats_one_when_behind() {
        let (mut p, mut f) = feed();
        push_frames(&mut p, 1, 6);
        let mut out = [9.0; 2];
        f.fill(&mut out);
        assert_eq!(left(&out), [2.0], "6 frames is past target and slack: frame 1 dropped");
        let (mut p, mut f) = feed();
        push_frames(&mut p, 1, 4);
        let mut out = [9.0; 2];
        f.fill(&mut out);
        assert_eq!(left(&out), [1.0]);
        f.fill(&mut out);
        f.fill(&mut out);
        assert_eq!(left(&out), [2.0], "2 frames left is under target and slack: frame 2 repeated");
    }

    #[test]
    fn a_burst_drops_back_to_the_target() {
        let (mut p, mut f) = feed();
        push_frames(&mut p, 1, 10);
        let mut out = [9.0; 2];
        f.fill(&mut out);
        assert_eq!(left(&out), [7.0], "10 frames past the burst limit: down to the newest 4");
    }
}
