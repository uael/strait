//! Captures what Windows plays and hands it on as `Sound` datagrams.
//!
//! WASAPI loopback reads the default output's mix, so no driver is needed.
//! When the user picks another output, the capture follows it.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use strait::wire::{Datagram, SOUND_FRAMES};

use crate::log;

/// How often the capture checks which output is the default.
const FOLLOW_EVERY: Duration = Duration::from_secs(2);

/// Where captured sound goes.
pub type Sink = Arc<dyn Fn(Datagram) + Send + Sync>;

/// A running capture; dropping it stops the capture.
pub struct Capture {
    _stop: Sender<()>,
}

impl Capture {
    pub fn start(send: Sink) -> Capture {
        let (stop, stopped) = mpsc::channel();
        std::thread::Builder::new().name("sound".into()).spawn(move || run(send, stopped)).expect("spawn sound thread");
        Capture { _stop: stop }
    }
}

fn run(send: Sink, stopped: Receiver<()>) {
    let host = cpal::default_host();
    let mut last_error = None;
    loop {
        let device = host.default_output_device();
        let id = device.as_ref().and_then(|d| d.id().ok());
        // The stream captures while it lives.
        let _stream = match device.ok_or_else(|| "Windows has no sound output".to_owned()).and_then(|d| open(&d, send.clone())) {
            Ok((stream, rate, channels)) => {
                log!("sending Windows sound to the Mac: {channels} channels at {rate} Hz");
                last_error = None;
                Some(stream)
            }
            Err(e) => {
                if last_error.as_ref() != Some(&e) {
                    log!("cannot capture Windows sound: {e}");
                    last_error = Some(e);
                }
                None
            }
        };
        loop {
            match stopped.recv_timeout(FOLLOW_EVERY) {
                Err(RecvTimeoutError::Timeout) => {
                    let now = host.default_output_device().and_then(|d| d.id().ok());
                    if now != id || last_error.is_some() {
                        break;
                    }
                }
                Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                    log!("stopped sending Windows sound");
                    return;
                }
            }
        }
    }
}

/// Starts a loopback capture of `device`, returning it with its rate and channels.
fn open(device: &cpal::Device, send: Sink) -> Result<(cpal::Stream, u32, u16), String> {
    let config = device.default_output_config().map_err(|e| format!("read the output format: {e}"))?;
    if config.sample_format() != cpal::SampleFormat::F32 {
        return Err(format!("the output mixes {} samples; strait reads 32-bit float", config.sample_format()));
    }
    let (rate, channels) = (config.sample_rate(), config.channels());
    let mut pending: Vec<i16> = Vec::with_capacity(SOUND_FRAMES * 2);
    let stream = device
        .build_input_stream(
            config.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                for frame in data.chunks_exact(usize::from(channels)) {
                    let (l, r) = stereo(frame);
                    pending.extend([sample(l), sample(r)]);
                    if pending.len() == SOUND_FRAMES * 2 {
                        send(Datagram::Sound { rate, samples: std::mem::replace(&mut pending, Vec::with_capacity(SOUND_FRAMES * 2)) });
                    }
                }
            },
            |e| log!("Windows sound capture: {e}"),
            None,
        )
        .map_err(|e| format!("open a loopback capture: {e}"))?;
    stream.play().map_err(|e| format!("start the loopback capture: {e}"))?;
    Ok((stream, rate, channels))
}

fn sample(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16
}

/// Folds one frame down to stereo. Windows orders channels front left,
/// front right, center, LFE, then surround pairs; the center and the
/// surrounds join each side at -3 dB, and the LFE is left out.
fn stereo(frame: &[f32]) -> (f32, f32) {
    const HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;
    match frame {
        [] => (0.0, 0.0),
        [mono] => (*mono, *mono),
        [l, r] => (*l, *r),
        [l, r, center, rest @ ..] => {
            let surrounds = rest.get(1..).unwrap_or_default();
            let (sl, sr) = surrounds.chunks(2).fold((0.0, 0.0), |(a, b), pair| (a + pair[0], b + pair.get(1).copied().unwrap_or(0.0)));
            (l + HALF * (center + sl), r + HALF * (center + sr))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_channels_to_stereo() {
        assert_eq!(stereo(&[0.5]), (0.5, 0.5));
        assert_eq!(stereo(&[0.25, -0.25]), (0.25, -0.25));
        let h = std::f32::consts::FRAC_1_SQRT_2;
        // 5.1: L R C LFE SL SR; the LFE (9.0) is left out.
        let (l, r) = stereo(&[0.1, 0.2, 0.4, 9.0, 0.3, 0.5]);
        assert!((l - (0.1 + h * 0.7)).abs() < 1e-6 && (r - (0.2 + h * 0.9)).abs() < 1e-6);
        // 7.1 adds a second surround pair.
        let (l, r) = stereo(&[0.0, 0.0, 0.0, 9.0, 0.1, 0.2, 0.3, 0.4]);
        assert!((l - h * 0.4).abs() < 1e-6 && (r - h * 0.6).abs() < 1e-6);
        assert_eq!(sample(2.0), i16::MAX);
        assert_eq!(sample(-2.0), -i16::MAX);
    }
}
