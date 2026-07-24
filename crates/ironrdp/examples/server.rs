//! Example of utilizing `ironrdp-server` crate.

#![allow(unused_crate_dependencies)] // False positives because there are both a library and a binary.
#![allow(clippy::print_stdout)]

use core::net::SocketAddr;
use core::num::{NonZeroU16, NonZeroUsize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context as _;
use ironrdp::cliprdr::backend::{CliprdrBackend, CliprdrBackendFactory};
use ironrdp::connector::DesktopSize;
use ironrdp::rdpsnd::pdu::{AudioFormat, ClientAudioFormatPdu, WaveFormat};
use ironrdp::rdpsnd::server::{RdpsndServerHandler, RdpsndServerMessage};
use ironrdp::server::tokio::sync::mpsc::UnboundedSender;
use ironrdp::server::tokio::time::{self, Duration, sleep};
use ironrdp::server::{
    BitmapUpdate, CliprdrServerFactory, Credentials, DisplayUpdate, KeyboardEvent, MouseEvent, PixelFormat, RdpServer,
    RdpServerDisplay, RdpServerDisplayUpdates, RdpServerInputHandler, ServerEvent, ServerEventSender,
    SoundServerFactory, TlsIdentityCtx, tokio,
};
use ironrdp_cliprdr_native::StubCliprdrBackend;
use tracing::{debug, info, warn};

const HELP: &str = "\
USAGE:
  cargo run --example=server -- [--bind-addr <SOCKET ADDRESS>] [--cert <CERTIFICATE>] [--key <CERTIFICATE KEY>] [--user USERNAME] [--pass PASSWORD] [--sec tls|hybrid]
";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), anyhow::Error> {
    let action = match parse_args() {
        Ok(action) => action,
        Err(e) => {
            println!("{HELP}");
            return Err(e.context("invalid argument(s)"));
        }
    };

    setup_logging()?;

    match action {
        Action::ShowHelp => {
            println!("{HELP}");
            Ok(())
        }
        Action::Run {
            bind_addr,
            hybrid,
            user,
            pass,
            cert,
            key,
        } => run(bind_addr, hybrid, user, pass, cert, key).await,
    }
}

#[derive(Debug)]
enum Action {
    ShowHelp,
    Run {
        bind_addr: SocketAddr,
        hybrid: bool,
        user: String,
        pass: String,
        cert: Option<PathBuf>,
        key: Option<PathBuf>,
    },
}

fn parse_args() -> anyhow::Result<Action> {
    let mut args = pico_args::Arguments::from_env();

    let action = if args.contains(["-h", "--help"]) {
        Action::ShowHelp
    } else {
        let bind_addr = args
            .opt_value_from_str("--bind-addr")?
            .unwrap_or_else(|| "127.0.0.1:3389".parse().expect("valid hardcoded SocketAddr string"));

        let sec = args.opt_value_from_str("--sec")?.unwrap_or_else(|| "hybrid".to_owned());
        let hybrid = match sec.as_ref() {
            "tls" => false,
            "hybrid" => true,
            _ => anyhow::bail!("Unhandled security: '{sec}'"),
        };

        let cert = args.opt_value_from_str("--cert")?;
        let key = args.opt_value_from_str("--key")?;

        let user = args.opt_value_from_str("--user")?.unwrap_or_else(|| "user".to_owned());
        let pass = args.opt_value_from_str("--pass")?.unwrap_or_else(|| "pass".to_owned());

        Action::Run {
            bind_addr,
            hybrid,
            user,
            pass,
            cert,
            key,
        }
    };

    Ok(action)
}

fn setup_logging() -> anyhow::Result<()> {
    use tracing::metadata::LevelFilter;
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::prelude::*;

    let fmt_layer = tracing_subscriber::fmt::layer().compact();

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .with_env_var("IRONRDP_LOG")
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(env_filter)
        .try_init()
        .context("failed to set tracing global subscriber")?;

    Ok(())
}

#[derive(Clone, Debug)]
struct Handler;

impl Handler {
    fn new() -> Self {
        Self
    }
}

impl RdpServerInputHandler for Handler {
    fn keyboard(&mut self, event: KeyboardEvent) {
        info!(?event, "keyboard");
    }

    fn mouse(&mut self, event: MouseEvent) {
        info!(?event, "mouse");
    }
}

const WIDTH: u16 = 1920;
const HEIGHT: u16 = 1080;

struct DisplayUpdates {
    frame_counter: u64,
    stripe_y: u16,
}

impl DisplayUpdates {
    fn new() -> Self {
        Self {
            frame_counter: 0,
            stripe_y: 0,
        }
    }
}

const STRIPE_HEIGHT: u16 = 16;

#[async_trait::async_trait]
impl RdpServerDisplayUpdates for DisplayUpdates {
    async fn next_update(&mut self) -> anyhow::Result<Option<DisplayUpdate>> {
        if self.stripe_y >= HEIGHT {
            self.stripe_y = 0;
            self.frame_counter += 1;
            sleep(Duration::from_millis(66)).await; // ~15 FPS VSync pacing
        }

        let y = self.stripe_y;
        let h = STRIPE_HEIGHT.min(HEIGHT - y);
        self.stripe_y += h;

        let full_frame = render_full_desktop_frame(self.frame_counter);

        // Extract stripe data for [y .. y+h]
        let start_idx = (y as usize) * (WIDTH as usize) * 4;
        let end_idx = ((y + h) as usize) * (WIDTH as usize) * 4;
        let stripe_data = full_frame[start_idx..end_idx].to_vec();

        let bitmap = BitmapUpdate {
            x: 0,
            y,
            width: NonZeroU16::new(WIDTH).unwrap(),
            height: NonZeroU16::new(h).unwrap(),
            format: PixelFormat::BgrA32,
            data: stripe_data.into(),
            stride: NonZeroUsize::new((WIDTH as usize) * 4).unwrap(),
        };

        Ok(Some(DisplayUpdate::Bitmap(bitmap)))
    }
}

fn render_full_desktop_frame(frame_num: u64) -> Vec<u8> {
    let mut data = vec![0u8; (WIDTH as usize) * (HEIGHT as usize) * 4];

    // 1. Desktop Background Gradient (Dark Slate / Navy)
    for y in 0..HEIGHT {
        let r = (26 + (y as u32 * 20 / HEIGHT as u32)) as u8;
        let g = (29 + (y as u32 * 25 / HEIGHT as u32)) as u8;
        let b = (36 + (y as u32 * 30 / HEIGHT as u32)) as u8;
        for x in 0..WIDTH {
            let idx = ((y as usize) * (WIDTH as usize) + (x as usize)) * 4;
            data[idx] = b;
            data[idx + 1] = g;
            data[idx + 2] = r;
            data[idx + 3] = 255;
        }
    }

    // 2. Top Taskbar (y: 0..40, Dark Charcoal)
    for y in 0..40 {
        for x in 0..WIDTH {
            let idx = ((y as usize) * (WIDTH as usize) + (x as usize)) * 4;
            data[idx] = 23;
            data[idx + 1] = 17;
            data[idx + 2] = 13;
            data[idx + 3] = 255;
        }
    }

    // Taskbar Accent Line (y: 39, Electric Blue)
    for x in 0..WIDTH {
        let idx = ((39usize) * (WIDTH as usize) + (x as usize)) * 4;
        data[idx] = 235;
        data[idx + 1] = 111;
        data[idx + 2] = 31;
        data[idx + 3] = 255;
    }

    // 3. Central Application Window Frame (x: 400..1520, y: 150..850)
    let win_x0 = 400;
    let win_x1 = 1520;
    let win_y0 = 150;
    let win_y1 = 850;

    for y in win_y0..win_y1 {
        for x in win_x0..win_x1 {
            let idx = ((y as usize) * (WIDTH as usize) + (x as usize)) * 4;
            if y < 190 {
                // Window Title Header (#161B22)
                data[idx] = 34;
                data[idx + 1] = 27;
                data[idx + 2] = 22;
            } else {
                // Window Body (#21262D)
                data[idx] = 45;
                data[idx + 1] = 38;
                data[idx + 2] = 33;
            }
            data[idx + 3] = 255;
        }
    }

    // Window Window Control Buttons (Red, Yellow, Green)
    draw_circle(&mut data, WIDTH, 430, 170, 6, [86, 95, 255, 255]); // Red
    draw_circle(&mut data, WIDTH, 450, 170, 6, [46, 189, 255, 255]); // Yellow
    draw_circle(&mut data, WIDTH, 470, 170, 6, [63, 201, 39, 255]); // Green

    // 4. Color Calibration Test Bars (x: 1200..1880, y: 900..1040)
    let bar_colors = [
        [255, 0, 0, 255],     // Blue
        [0, 255, 0, 255],     // Green
        [0, 0, 255, 255],     // Red
        [255, 255, 0, 255],   // Cyan
        [255, 0, 255, 255],   // Magenta
        [0, 255, 255, 255],   // Yellow
        [255, 255, 255, 255], // White
    ];
    let bar_w = 680 / bar_colors.len() as u16;
    for (i, col) in bar_colors.iter().enumerate() {
        let bx0 = 1200 + (i as u16 * bar_w);
        let bx1 = (bx0 + bar_w).min(1880);
        for y in 900..1040 {
            for x in bx0..bx1 {
                let idx = ((y as usize) * (WIDTH as usize) + (x as usize)) * 4;
                data[idx] = col[0];
                data[idx + 1] = col[1];
                data[idx + 2] = col[2];
                data[idx + 3] = col[3];
            }
        }
    }

    // 5. Animated Bouncing Motion Target inside Window Body
    let ball_cx = (600.0 + ((frame_num as f32 * 0.1).sin() * 300.0)) as u16;
    let ball_cy = (500.0 + ((frame_num as f32 * 0.15).cos() * 200.0)) as u16;
    draw_circle(&mut data, WIDTH, ball_cx, ball_cy, 25, [31, 111, 235, 255]); // Electric Blue Motion Target

    data
}

fn draw_circle(data: &mut [u8], stride_w: u16, cx: u16, cy: u16, radius: u16, color: [u8; 4]) {
    let r2 = (radius as i32) * (radius as i32);
    let y0 = cy.saturating_sub(radius);
    let y1 = (cy + radius).min(HEIGHT);
    let x0 = cx.saturating_sub(radius);
    let x1 = (cx + radius).min(stride_w);

    for y in y0..y1 {
        let dy = y as i32 - cy as i32;
        for x in x0..x1 {
            let dx = x as i32 - cx as i32;
            if dx * dx + dy * dy <= r2 {
                let idx = ((y as usize) * (stride_w as usize) + (x as usize)) * 4;
                if idx + 3 < data.len() {
                    data[idx] = color[0];
                    data[idx + 1] = color[1];
                    data[idx + 2] = color[2];
                    data[idx + 3] = color[3];
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl RdpServerDisplay for Handler {
    async fn size(&mut self) -> DesktopSize {
        DesktopSize {
            width: WIDTH,
            height: HEIGHT,
        }
    }

    async fn updates(&mut self) -> anyhow::Result<Box<dyn RdpServerDisplayUpdates>> {
        Ok(Box::new(DisplayUpdates::new()))
    }
}

struct StubCliprdrServerFactory;

impl CliprdrBackendFactory for StubCliprdrServerFactory {
    fn build_cliprdr_backend(&self) -> Box<dyn CliprdrBackend> {
        Box::new(StubCliprdrBackend::new())
    }
}

impl ServerEventSender for StubCliprdrServerFactory {
    fn set_sender(&mut self, _sender: UnboundedSender<ServerEvent>) {}
}

impl CliprdrServerFactory for StubCliprdrServerFactory {}

#[derive(Debug)]
pub struct Inner {
    ev_sender: Option<UnboundedSender<ServerEvent>>,
}

struct StubSoundServerFactory {
    inner: Arc<Mutex<Inner>>,
}

impl ServerEventSender for StubSoundServerFactory {
    fn set_sender(&mut self, sender: UnboundedSender<ServerEvent>) {
        let mut inner = self.inner.lock().expect("poisoned");
        inner.ev_sender = Some(sender);
    }
}

impl SoundServerFactory for StubSoundServerFactory {
    fn build_backend(&self) -> Box<dyn RdpsndServerHandler> {
        Box::new(SndHandler {
            inner: Arc::clone(&self.inner),
            task: None,
        })
    }
}

#[derive(Debug)]
struct SndHandler {
    inner: Arc<Mutex<Inner>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl SndHandler {
    fn choose_format(&self, client_formats: &[AudioFormat]) -> Option<u16> {
        for (n, fmt) in client_formats.iter().enumerate() {
            if self.get_formats().contains(fmt) {
                return u16::try_from(n).ok();
            }
        }
        None
    }
}

impl RdpsndServerHandler for SndHandler {
    fn get_formats(&self) -> &[AudioFormat] {
        &[
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

    fn start(&mut self, client_format: &ClientAudioFormatPdu) -> Option<u16> {
        debug!(?client_format);

        let Some(nfmt) = self.choose_format(&client_format.formats) else {
            return Some(0);
        };

        let fmt = client_format.formats[usize::from(nfmt)].clone();

        let mut opus_enc = if fmt.format == WaveFormat::OPUS {
            let n_channels: opus2::Channels = match fmt.n_channels {
                1 => opus2::Channels::Mono,
                2 => opus2::Channels::Stereo,
                n => {
                    warn!("Invalid OPUS channels: {}", n);
                    return Some(0);
                }
            };

            match opus2::Encoder::new(fmt.n_samples_per_sec, n_channels, opus2::Application::Audio) {
                Ok(enc) => Some(enc),
                Err(err) => {
                    warn!("Failed to create OPUS encoder: {}", err);
                    return Some(0);
                }
            }
        } else {
            None
        };

        let inner = Arc::clone(&self.inner);
        self.task = Some(tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(20));
            let mut ts = 0;
            let mut phase = 0.0f32;
            loop {
                interval.tick().await;
                let wave = generate_sine_wave(fmt.n_samples_per_sec, 440.0, 20, &mut phase);

                let data = if let Some(ref mut enc) = opus_enc {
                    match enc.encode_vec(&wave, wave.len()) {
                        Ok(data) => data,
                        Err(err) => {
                            warn!("Failed to encode with OPUS: {}", err);
                            return;
                        }
                    }
                } else {
                    wave.into_iter().flat_map(|value| value.to_le_bytes()).collect()
                };

                let inner = inner.lock().expect("poisoned");
                if let Some(sender) = inner.ev_sender.as_ref() {
                    let _ = sender.send(ServerEvent::Rdpsnd(RdpsndServerMessage::Wave(data, ts)));
                }
                ts = ts.wrapping_add(100);
            }
        }));

        Some(nfmt)
    }

    fn stop(&mut self) {
        let Some(task) = self.task.take() else {
            return;
        };
        task.abort();
    }
}

fn generate_sine_wave(sample_rate: u32, frequency: f32, duration_ms: u64, phase: &mut f32) -> Vec<i16> {
    use core::f32::consts::PI;

    let total_samples = (u64::from(sample_rate) * duration_ms) / 1000;

    #[expect(clippy::as_conversions)]
    let delta_phase = 2.0 * PI * frequency / sample_rate as f32;

    let amplitude = 32767.0; // Max amplitude for 16-bit audio

    let capacity = usize::try_from(total_samples).expect("u64-to-usize") * 2; // 2 channels
    let mut samples = Vec::with_capacity(capacity);

    for _ in 0..total_samples {
        let sample = (*phase).sin();
        *phase += delta_phase;

        // Wrap phase to maintain precision and avoid overflow.
        *phase %= 2.0 * PI;

        #[expect(clippy::as_conversions, clippy::cast_possible_truncation)]
        let sample_i16 = (sample * amplitude) as i16;

        // Write same sample to both channels (stereo)
        samples.push(sample_i16);
        samples.push(sample_i16);
    }

    samples
}

async fn run(
    bind_addr: SocketAddr,
    hybrid: bool,
    username: String,
    password: String,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
) -> anyhow::Result<()> {
    info!(%bind_addr, ?cert, ?key, "run");

    let handler = Handler::new();

    let server_builder = RdpServer::builder().with_addr(bind_addr);

    let server_builder = if let Some((cert_path, key_path)) = cert.as_deref().zip(key.as_deref()) {
        let identity = TlsIdentityCtx::init_from_paths(cert_path, key_path).context("failed to init TLS identity")?;
        let acceptor = identity.make_acceptor().context("failed to build TLS acceptor")?;

        if hybrid {
            server_builder.with_hybrid(acceptor, identity.pub_key)
        } else {
            server_builder.with_tls(acceptor)
        }
    } else {
        server_builder.with_no_security()
    };

    let cliprdr = Box::new(StubCliprdrServerFactory);

    let sound = Box::new(StubSoundServerFactory {
        inner: Arc::new(Mutex::new(Inner { ev_sender: None })),
    });

    let mut server = server_builder
        .with_input_handler(handler.clone())
        .with_display_handler(handler.clone())
        .with_cliprdr_factory(Some(cliprdr))
        .with_sound_factory(Some(sound))
        .build();

    server.set_credentials(Some(Credentials {
        username,
        password,
        domain: None,
    }));

    server.run().await
}
