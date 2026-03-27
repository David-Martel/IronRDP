use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::time::Duration;
use std::borrow::Cow;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use anyhow::{Context as _, bail};
use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
use cpal::{SampleFormat, Stream, StreamConfig};
use ironrdp_rdpsnd::client::RdpsndClientHandler;
use ironrdp_rdpsnd::pdu::{AudioFormat, PitchPdu, VolumePdu, WaveFormat};
use tracing::{debug, error, warn};

/// Maximum time the cpal callback will block waiting for the next audio packet.
///
/// A short timeout prevents the audio thread from stalling the OS driver when
/// the network is temporarily quiet or the pipeline is restarting.  100 ms is
/// long enough to absorb typical RDP wave-packet jitter while still allowing
/// the driver to detect a genuine underrun and fill with silence promptly.
const AUDIO_RECV_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub struct RdpsndBackend {
    // Unfortunately, Stream is not `Send`, so we move it to a separate thread.
    stream_handle: Option<JoinHandle<()>>,
    stream_ended: Arc<AtomicBool>,
    tx: Option<Sender<Vec<u8>>>,
    format_no: Option<usize>,
}

impl Default for RdpsndBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl RdpsndBackend {
    pub fn new() -> Self {
        Self {
            tx: None,
            format_no: None,
            stream_handle: None,
            stream_ended: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Drop for RdpsndBackend {
    fn drop(&mut self) {
        self.close();
    }
}

impl RdpsndClientHandler for RdpsndBackend {
    fn get_formats(&self) -> &[AudioFormat] {
        &[
            #[cfg(feature = "opus")]
            AudioFormat {
                format: WaveFormat::OPUS,
                n_channels: 2,
                n_samples_per_sec: 48000,
                n_avg_bytes_per_sec: 192000,
                n_block_align: 4,
                bits_per_sample: 16,
                data: None,
            },
            AudioFormat {
                format: WaveFormat::PCM,
                n_channels: 2,
                n_samples_per_sec: 44100,
                n_avg_bytes_per_sec: 176400,
                n_block_align: 4,
                bits_per_sample: 16,
                data: None,
            },
        ]
    }

    fn wave(&mut self, format_no: usize, _ts: u32, data: Cow<'_, [u8]>) {
        if Some(format_no) != self.format_no {
            debug!("New audio format");
            self.close();
        }

        if self.stream_handle.is_none() {
            let (tx, rx) = mpsc::channel();
            self.tx = Some(tx);

            self.format_no = Some(format_no);
            let Some(format) = self.get_formats().get(format_no) else {
                warn!(?format_no, "Invalid format_no");
                return;
            };
            let format = format.clone();
            self.stream_ended.store(false, Ordering::Relaxed);
            let stream_ended = Arc::clone(&self.stream_ended);
            self.stream_handle = Some(thread::spawn(move || {
                let stream = match DecodeStream::new(&format, rx) {
                    Ok(stream) => stream,
                    Err(e) => {
                        error!(error = format!("{e:#}"));
                        return;
                    }
                };
                debug!("Stream thread parking loop");
                while !stream_ended.load(Ordering::Relaxed) {
                    thread::park();
                }
                debug!("Stream thread unparked");
                drop(stream);
            }));
        }

        if let Some(ref tx) = self.tx
            && let Err(error) = tx.send(data.to_vec())
        {
            // The stream thread's receiver was dropped (e.g. the stream thread
            // crashed or is shutting down).  Not a logic error on our side.
            warn!(%error, "Audio wave send failed; stream receiver dropped");
        };
    }

    fn set_volume(&mut self, volume: VolumePdu) {
        debug!(?volume);
    }

    fn set_pitch(&mut self, pitch: PitchPdu) {
        debug!(?pitch);
    }

    fn close(&mut self) {
        self.tx = None;
        if let Some(stream) = self.stream_handle.take() {
            self.stream_ended.store(true, Ordering::Relaxed);
            stream.thread().unpark();
            if let Err(err) = stream.join() {
                error!(?err, "Failed to join a stream thread");
            }
        }
    }
}

#[doc(hidden)]
pub struct DecodeStream {
    _dec_thread: Option<JoinHandle<()>>,
    stream: Stream,
    underrun_count: Arc<AtomicU64>,
    /// Cumulative count of Opus decode errors (get_nb_samples or decode
    /// failures).  Tracked separately from underruns so callers can
    /// distinguish decoder correctness problems from buffer starvation.
    decode_error_count: Arc<AtomicU64>,
}

impl DecodeStream {
    pub fn new(rx_format: &AudioFormat, mut rx: Receiver<Vec<u8>>) -> anyhow::Result<Self> {
        let mut dec_thread = None;
        let decode_error_count = Arc::new(AtomicU64::new(0));
        match rx_format.format {
            #[cfg(feature = "opus")]
            WaveFormat::OPUS => {
                let chan = match rx_format.n_channels {
                    1 => opus2::Channels::Mono,
                    2 => opus2::Channels::Stereo,
                    _ => bail!("unsupported #channels for Opus"),
                };
                let (dec_tx, dec_rx) = mpsc::channel();
                let mut dec = opus2::Decoder::new(rx_format.n_samples_per_sec, chan)?;
                let decode_error_count_clone = Arc::clone(&decode_error_count);
                dec_thread = Some(thread::spawn(move || {
                    // Loop exits cleanly when the sender is dropped (normal shutdown).
                    while let Ok(pkt) = rx.recv() {
                        let nb_samples = match dec.get_nb_samples(&pkt) {
                            Ok(nb_samples) => nb_samples,
                            Err(error) => {
                                let n = decode_error_count_clone.fetch_add(1, Ordering::Relaxed).saturating_add(1);
                                warn!(?error, decode_errors = n, "Failed to get Opus packet sample count; skipping packet");
                                continue;
                            }
                        };

                        #[expect(
                            clippy::as_conversions,
                            reason = "opus::Channels has no conversions to usize implemented"
                        )]
                        let mut pcm = vec![0u8; nb_samples * chan as usize * size_of::<i16>()];
                        if let Err(error) = dec.decode(&pkt, bytemuck::cast_slice_mut(pcm.as_mut_slice()), false) {
                            let n = decode_error_count_clone.fetch_add(1, Ordering::Relaxed).saturating_add(1);
                            warn!(?error, decode_errors = n, "Failed to decode Opus packet; skipping packet");
                            continue;
                        }

                        if dec_tx.send(pcm).is_err() {
                            // Playback receiver was dropped (cpal stream tearing down or
                            // format change).  This is expected during shutdown — exit silently.
                            break;
                        }
                    }
                }));
                rx = dec_rx;
            }
            WaveFormat::PCM => {}
            _ => bail!("audio format not supported"),
        }

        let sample_format = match rx_format.bits_per_sample {
            8 => SampleFormat::U8,
            16 => SampleFormat::I16,
            _ => {
                bail!("only PCM 8/16 bits formats supported");
            }
        };

        // Silence byte value: 0x00 for i16 PCM (two's complement zero),
        // 0x80 for u8 PCM (unsigned midpoint = silence).
        let silence_byte: u8 = match sample_format {
            SampleFormat::U8 => 0x80,
            _ => 0x00,
        };

        let host = cpal::default_host();
        let device = host.default_output_device().context("no default output device")?;
        let _supported_configs_range = device
            .supported_output_configs()
            .context("no supported output config")?;
        let default_config = device.default_output_config()?;
        debug!(?default_config);

        let underrun_count = Arc::new(AtomicU64::new(0));
        let mut rx = RxBuffer::new(rx, silence_byte, Arc::clone(&underrun_count));

        // Request ~40 ms of buffer at the negotiated sample rate.  This is a
        // hint to the OS driver; the actual callback size may differ, but
        // requesting a larger buffer absorbs network jitter and reduces the
        // frequency of underruns compared to BufferSize::Default (which on
        // Windows WASAPI can be as small as a few milliseconds).
        let buffer_size = cpal::BufferSize::Fixed(rx_format.n_samples_per_sec / 25);

        let config = StreamConfig {
            channels: rx_format.n_channels,
            sample_rate: rx_format.n_samples_per_sec,
            buffer_size,
        };
        debug!(?config);

        let stream = device
            .build_output_stream_raw(
                &config,
                sample_format,
                move |data, _info: &cpal::OutputCallbackInfo| {
                    let data = data.bytes_mut();
                    rx.fill(data)
                },
                |error| error!(%error),
                None,
            )
            .context("failed to setup output stream")?;

        stream.play().context("start audio output stream")?;
        debug!("Audio output stream started");

        Ok(Self {
            _dec_thread: dec_thread,
            stream,
            underrun_count,
            decode_error_count,
        })
    }

    pub fn stream(&self) -> &Stream {
        &self.stream
    }

    /// Returns the total number of cpal callback underruns since the stream
    /// was created.  Each underrun means the callback could not obtain audio
    /// data in time and wrote silence instead.
    pub fn underrun_count(&self) -> u64 {
        self.underrun_count.load(Ordering::Relaxed)
    }

    /// Returns the total number of Opus decode errors since the stream was
    /// created.  Non-zero values indicate decoder correctness problems (bad
    /// packets, corrupt data) distinct from buffer underruns.
    pub fn decode_error_count(&self) -> u64 {
        self.decode_error_count.load(Ordering::Relaxed)
    }
}

struct RxBuffer {
    receiver: Receiver<Vec<u8>>,
    last: Option<Vec<u8>>,
    idx: usize,
    /// Byte value that represents digital silence for the negotiated sample format.
    /// 0x00 for i16/i32 PCM; 0x80 for u8 PCM.
    silence_byte: u8,
    /// Cumulative count of underrun events (incremented once per `fill` call
    /// that cannot obtain audio data within [`AUDIO_RECV_TIMEOUT`]).
    underrun_count: Arc<AtomicU64>,
}

impl RxBuffer {
    fn new(receiver: Receiver<Vec<u8>>, silence_byte: u8, underrun_count: Arc<AtomicU64>) -> Self {
        Self {
            receiver,
            last: None,
            idx: 0,
            silence_byte,
            underrun_count,
        }
    }

    fn fill(&mut self, data: &mut [u8]) {
        let mut filled = 0;

        while filled < data.len() {
            if self.last.is_none() {
                match self.receiver.recv_timeout(AUDIO_RECV_TIMEOUT) {
                    Ok(rx) => {
                        debug!(rx.len = rx.len());
                        self.last = Some(rx);
                    }
                    Err(_) => {
                        // No packet arrived within the timeout window.  Fill
                        // the remainder of the callback buffer with silence so
                        // the driver receives valid audio data and the OS does
                        // not produce an audible click or report a hard error.
                        let underruns = self
                            .underrun_count
                            .fetch_add(1, Ordering::Relaxed)
                            .saturating_add(1);
                        debug!(underruns, "Playback buffer underrun, writing silence");
                        data[filled..].fill(self.silence_byte);
                        return;
                    }
                }
            }

            let Some(ref last) = self.last else {
                // `self.last` is `None` only if `recv_timeout` returned `Err`
                // above, which already handled the return.  This branch is
                // unreachable in practice but kept to satisfy the borrow
                // checker without an explicit `unwrap`.
                data[filled..].fill(self.silence_byte);
                return;
            };

            #[expect(clippy::arithmetic_side_effects)]
            while self.idx < last.len() && filled < data.len() {
                data[filled] = last[self.idx];
                assert!(filled < usize::MAX);
                assert!(self.idx < usize::MAX);
                filled += 1;
                self.idx += 1;
            }

            // If all elements from last have been consumed, clear `self.last`
            if self.idx >= last.len() {
                self.last = None;
                self.idx = 0;
            }
        }
    }
}
