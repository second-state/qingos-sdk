//! 整屏页的四样能力：逐帧画面、按键状态、音调，加上把画面放大到屏上。
//!
//! 设备和开发机上的 `vibegame` 命令行用这同一份代码。app 这头的导入函数（`qing` 模块里）：
//! - `frame(pixels, palette) -> i32`：交一帧。`pixels` 是 240×160 个字节，一个字节指向调色板的一格；
//!   `palette` 是 256 格，每格 R G B 三个字节。系统在 `qing_tick` 之后取最新交的那一帧。0 是收了，-1 是地址不对。
//! - `frame565(pixels) -> i32`：清单写了 `frame = "640x480"` 的 app 交一帧真彩画面（ADR 0036）。
//!   `pixels` 是 640×480 个像素，一个像素两个字节，RGB565，高字节在前。系统不放大，放在屏中间。
//!   0 是收了，-1 是清单没写或地址不对。写了的 app 调 `frame` 也回 -1：一个 app 只交一种画面。
//! - `buttons() -> u32`：这一帧哪些键按着，位图见 [`key`]。
//! - `tone(channel, hz, ms, volume) -> i32`：在声道 0–3 上发一个音调。`ms` 是 0 就停掉这个声道。
//!   `volume` 是 0–100。0 是收了，-1 是声道不对。
//! - `tone_wav(ptr, len) -> i32`：在第五个声道放一小段 WAV（16 位 PCM，最长 2 秒）。0 是收了，-1 是不是 WAV 或太长。
//!
//! 系统这边的状态很少：最新的一帧、按着的键、合成器里在响的。这些都不用存：
//! 随时存档只存 wasm 内存，读档时停掉声音。

use std::sync::{Arc, Mutex};
use wasmtime::{Caller, Linker};

/// 调色板画面（[`Format::Indexed`]）的宽和高。放大 3 倍是 720×480。
pub const W: usize = 240;
pub const H: usize = 160;
/// 一帧的像素有几个字节。
pub const PIXELS: usize = W * H;
/// 调色板有几个字节：256 格，每格 R G B。
pub const PALETTE: usize = 256 * 3;

/// 按键状态的位。菜单键、电源、音量归系统，不在这里。
pub mod key {
    pub const UP: u32 = 1 << 0;
    pub const DOWN: u32 = 1 << 1;
    pub const LEFT: u32 = 1 << 2;
    pub const RIGHT: u32 = 1 << 3;
    pub const A: u32 = 1 << 4;
    pub const B: u32 = 1 << 5;
    pub const X: u32 = 1 << 6;
    pub const Y: u32 = 1 << 7;
    pub const L1: u32 = 1 << 8;
    pub const R1: u32 = 1 << 9;
    pub const START: u32 = 1 << 10;
    pub const SELECT: u32 = 1 << 11;
    /// 键的名字，脚本和日志里用。
    pub const NAMES: [(&str, u32); 12] = [
        ("up", UP),
        ("down", DOWN),
        ("left", LEFT),
        ("right", RIGHT),
        ("a", A),
        ("b", B),
        ("x", X),
        ("y", Y),
        ("l1", L1),
        ("r1", R1),
        ("start", START),
        ("select", SELECT),
    ];
    /// 名字查位，大小写都认。
    pub fn of(name: &str) -> Option<u32> {
        NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, b)| *b)
    }
}

/// 整屏 app 交哪种画面：清单的 `frame`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    /// 240×160，一个像素是调色板的一格。系统整数倍放大（ADR 0030）。清单不写 `frame` 就是它。
    #[default]
    Indexed,
    /// 640×480，一个像素两个字节，RGB565，高字节在前。系统不放大（ADR 0036）。清单写 `frame = "640x480"`。
    Rgb565,
}

impl Format {
    /// 清单的 `frame`：空的是 [`Format::Indexed`]，`"640x480"` 是 [`Format::Rgb565`]，别的不认。
    pub fn parse(s: &str) -> Option<Format> {
        match s {
            "" => Some(Format::Indexed),
            "640x480" => Some(Format::Rgb565),
            _ => None,
        }
    }

    /// 画面的宽和高。
    pub fn size(self) -> (usize, usize) {
        match self {
            Format::Indexed => (W, H),
            Format::Rgb565 => (640, 480),
        }
    }

    /// 一帧的像素有几个字节。
    pub fn bytes(self) -> usize {
        let (w, h) = self.size();
        match self {
            Format::Indexed => w * h,
            Format::Rgb565 => w * h * 2,
        }
    }

    /// 一块 `dw`×`dh` 的屏放得下几倍、从哪里开始画。放不下一倍是 0。
    /// 调色板的取最大的整数倍；RGB565 的不放大，放得下就是 1。
    pub fn fit(self, dw: usize, dh: usize) -> (usize, usize, usize) {
        let (w, h) = self.size();
        let mut k = (dw / w).min(dh / h);
        if self == Format::Rgb565 {
            k = k.min(1);
        }
        (k, (dw - w * k) / 2, (dh - h * k) / 2)
    }
}

/// 一个 RGB565 的点（高字节在前）换成 0x00RRGGBB。低位用高位补，白还是白。
pub fn rgb565(hi: u8, lo: u8) -> u32 {
    let v = (hi as u32) << 8 | lo as u32;
    let (r, g, b) = (v >> 11, (v >> 5) & 0x3f, v & 0x1f);
    (r << 3 | r >> 2) << 16 | (g << 2 | g >> 4) << 8 | (b << 3 | b >> 2)
}

/// 一帧：像素，调色板的画面还有调色板。
#[derive(Clone)]
pub struct Frame {
    pub format: Format,
    pub pixels: Vec<u8>,
    /// 调色板的画面才有：256 格，每格 R G B。RGB565 的是空的。
    pub palette: Vec<u8>,
}

impl Default for Frame {
    /// 还没交过画面：240×160，全黑。
    fn default() -> Self {
        Frame::blank(Format::Indexed)
    }
}

impl Frame {
    /// 还没交过画面：这种格式的一帧，全黑。
    pub fn blank(format: Format) -> Frame {
        let palette = if format == Format::Indexed { vec![0; PALETTE] } else { Vec::new() };
        Frame { format, pixels: vec![0; format.bytes()], palette }
    }

    /// 宽和高。
    pub fn size(&self) -> (usize, usize) {
        self.format.size()
    }

    /// 一个点的颜色，0x00RRGGBB。
    fn color(&self, x: usize, y: usize) -> u32 {
        let w = self.format.size().0;
        match self.format {
            Format::Indexed => {
                let i = self.pixels[y * w + x] as usize * 3;
                (self.palette[i] as u32) << 16 | (self.palette[i + 1] as u32) << 8 | self.palette[i + 2] as u32
            }
            Format::Rgb565 => {
                let i = (y * w + x) * 2;
                rgb565(self.pixels[i], self.pixels[i + 1])
            }
        }
    }

    /// 一个点的颜色。
    pub fn rgb(&self, x: usize, y: usize) -> [u8; 3] {
        let c = self.color(x, y);
        [(c >> 16) as u8, (c >> 8) as u8, c as u8]
    }

    /// 放大 `k` 倍（最近邻）的 RGB 字节，一行接一行。截图用。
    pub fn rgb_scaled(&self, k: usize) -> Vec<u8> {
        let k = k.max(1);
        let (w, h) = self.size();
        let mut out = Vec::with_capacity(w * h * k * k * 3);
        for y in 0..h {
            let row_start = out.len();
            for x in 0..w {
                let c = self.rgb(x, y);
                for _ in 0..k {
                    out.extend_from_slice(&c);
                }
            }
            let row = out[row_start..].to_vec();
            for _ in 1..k {
                out.extend_from_slice(&row);
            }
        }
        out
    }

    /// 画到一块 `dw`×`dh` 的屏上（每个点 0x00RRGGBB），放在中间，四周黑边。
    /// 调色板的取最大的整数倍；RGB565 的不放大（[`Format::fit`]）。屏放不下就全黑。
    pub fn blit(&self, dst: &mut [u32], dw: usize, dh: usize) {
        let (k, x0, y0) = self.format.fit(dw, dh);
        dst.iter_mut().for_each(|p| *p = 0);
        if k == 0 {
            return;
        }
        let (w, h) = self.size();
        let pal: Vec<u32> = self.palette.chunks(3).map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect();
        for y in 0..h {
            let top = (y0 + y * k) * dw + x0;
            let line = &mut dst[top..top + w * k];
            match self.format {
                Format::Indexed => {
                    for (x, &p) in self.pixels[y * w..(y + 1) * w].iter().enumerate() {
                        line[x * k..(x + 1) * k].fill(pal[p as usize]);
                    }
                }
                Format::Rgb565 => {
                    for (x, c) in self.pixels[y * w * 2..(y + 1) * w * 2].as_chunks::<2>().0.iter().enumerate() {
                        line[x * k..(x + 1) * k].fill(rgb565(c[0], c[1]));
                    }
                }
            }
            for r in 1..k {
                dst.copy_within(top..top + w * k, top + r * dw);
            }
        }
    }
}

/// 240×160 的画面在一块 `dw`×`dh` 的屏上放得下几倍、从哪里开始画。放不下一倍是 0。
pub fn fit(dw: usize, dh: usize) -> (usize, usize, usize) {
    Format::Indexed.fit(dw, dh)
}

/// app 发的一个音调。日志和测试看它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tone {
    /// 0、1 方波，2 三角波，3 噪声，4 短录音。
    pub channel: u32,
    pub hz: u32,
    pub ms: u32,
    pub volume: u32,
}

/// 短录音最长几秒。长的走 `sound`。
pub const WAV_MAX_SECS: f32 = 2.0;

/// 合成器：五个声道，混成一路单声道。系统按自己的采样率要样本。
pub struct Synth {
    rate: u32,
    voices: [Voice; 4],
    wav: Option<Wav>,
}

#[derive(Default, Clone, Copy)]
struct Voice {
    hz: u32,
    /// 还剩几个样本。0 是不响。
    left: u64,
    amp: f32,
    phase: f32,
    /// 噪声用的移位寄存器（15 位，和 FC 的一样）。
    lfsr: u16,
}

struct Wav {
    samples: Vec<i16>,
    rate: u32,
    /// 放到哪了，单位是原录音的样本，带小数。
    at: f64,
}

/// 一个声道最响时的幅度。五个一起最响会超，混完再截。
const PEAK: f32 = 0.25;

impl Synth {
    pub fn new(rate: u32) -> Self {
        Synth { rate: rate.max(1), voices: [Voice { lfsr: 1, ..Default::default() }; 4], wav: None }
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// 换采样率（声卡定的）。在响的照样响完。
    pub fn set_rate(&mut self, rate: u32) {
        self.rate = rate.max(1);
    }

    /// 在声道 0–3 上发一个音调。`ms` 或 `hz` 是 0 就停掉。声道不对回 false。
    pub fn tone(&mut self, channel: u32, hz: u32, ms: u32, volume: u32) -> bool {
        let Some(v) = self.voices.get_mut(channel as usize) else { return false };
        v.hz = hz;
        v.left = if hz == 0 { 0 } else { ms.min(60_000) as u64 * self.rate as u64 / 1000 };
        v.amp = volume.min(100) as f32 / 100.0 * PEAK;
        true
    }

    /// 在第五个声道放一段 WAV，顶掉在放的那段。不是 16 位 PCM 或超过 2 秒回 false。
    pub fn wav(&mut self, bytes: &[u8]) -> bool {
        match parse_wav(bytes) {
            Some((samples, rate)) if samples.len() as f32 <= rate as f32 * WAV_MAX_SECS => {
                self.wav = Some(Wav { samples, rate, at: 0.0 });
                true
            }
            _ => false,
        }
    }

    /// 全停（读档、退出时）。
    pub fn stop(&mut self) {
        for v in &mut self.voices {
            v.left = 0;
        }
        self.wav = None;
    }

    /// 有没有声道在响。
    pub fn busy(&self) -> bool {
        self.voices.iter().any(|v| v.left > 0) || self.wav.is_some()
    }

    /// 混出下面这段样本（单声道，-1 到 1），时间往前走这么多。
    pub fn render(&mut self, out: &mut [f32]) {
        let rate = self.rate as f32;
        for s in out.iter_mut() {
            let mut mix = 0.0;
            for (ch, v) in self.voices.iter_mut().enumerate() {
                if v.left == 0 {
                    continue;
                }
                v.left -= 1;
                let step = v.hz as f32 / rate;
                let wave = match ch {
                    0 | 1 => {
                        if v.phase < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    2 => 1.0 - 4.0 * (v.phase - 0.5).abs(),
                    _ => {
                        if v.lfsr & 1 == 0 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                };
                v.phase += step;
                while v.phase >= 1.0 {
                    v.phase -= 1.0;
                    if ch == 3 {
                        let bit = (v.lfsr ^ (v.lfsr >> 1)) & 1;
                        v.lfsr = (v.lfsr >> 1) | (bit << 14);
                    }
                }
                mix += wave * v.amp;
            }
            if let Some(w) = &mut self.wav {
                if let Some(&x) = w.samples.get(w.at as usize) {
                    mix += x as f32 / 32768.0 * 0.5;
                    w.at += w.rate as f64 / rate as f64;
                }
                if w.at as usize >= w.samples.len() {
                    self.wav = None;
                }
            }
            *s = mix.clamp(-1.0, 1.0);
        }
    }
}

/// 读 WAV：16 位 PCM，单声道或多声道（多声道取第一个）。返回样本和采样率。
fn parse_wav(b: &[u8]) -> Option<(Vec<i16>, u32)> {
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return None;
    }
    let (mut fmt, mut data) = (None, None);
    let mut i = 12;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let n = u32::from_le_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        let body = b.get(i + 8..(i + 8 + n).min(b.len()))?;
        match id {
            b"fmt " if body.len() >= 16 => fmt = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        i += 8 + n + (n & 1);
    }
    let (fmt, data) = (fmt?, data?);
    let format = u16::from_le_bytes([fmt[0], fmt[1]]);
    let channels = u16::from_le_bytes([fmt[2], fmt[3]]).max(1) as usize;
    let rate = u32::from_le_bytes(fmt[4..8].try_into().ok()?);
    let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
    if format != 1 || bits != 16 || rate == 0 {
        return None;
    }
    let samples = data.chunks_exact(2 * channels).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
    Some((samples, rate))
}

/// 一个 app 的整屏状态：app 的导入函数往里写，系统从里读。
pub struct Full {
    /// 清单的 `frame`：这个 app 交哪种画面。
    format: Format,
    state: Mutex<State>,
    /// 停着：不在前台，或者系统层叠在上面。停着时定频的线程不调它。
    paused: std::sync::atomic::AtomicBool,
}

struct State {
    buttons: u32,
    /// 最新交的一帧。没交过是 None。
    frame: Option<Frame>,
    /// 上次取过以后又交过。
    fresh: bool,
    synth: Synth,
    /// 上次取过以后 app 发的音调。
    tones: Vec<Tone>,
}

/// 合成器默认的采样率。系统按声卡改。
pub const RATE: u32 = 48_000;

impl Default for Full {
    /// 交 240×160 调色板画面的 app。
    fn default() -> Self {
        Full::new(Format::Indexed)
    }
}

impl Full {
    /// 交 `format` 这种画面的 app（清单的 `frame`，[`Format::parse`]）。
    pub fn new(format: Format) -> Self {
        Full {
            format,
            state: Mutex::new(State { buttons: 0, frame: None, fresh: false, synth: Synth::new(RATE), tones: Vec::new() }),
            paused: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// 这个 app 交哪种画面。
    pub fn format(&self) -> Format {
        self.format
    }

    /// 还没交过画面时显示的：这种格式的一帧，全黑。
    pub fn blank(&self) -> Frame {
        Frame::blank(self.format)
    }

    /// 停下或接着跑。停下时键都算没按，在响的声音停掉：系统层上来时游戏不该还在响。
    pub fn pause(&self, on: bool) {
        self.paused.store(on, std::sync::atomic::Ordering::SeqCst);
        if on {
            let mut s = self.state.lock().unwrap();
            s.buttons = 0;
            s.synth.stop();
        }
    }

    /// 停着没有。
    pub fn paused(&self) -> bool {
        self.paused.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 系统交这一帧的按键状态。系统层叠上来时交 0。
    pub fn set_buttons(&self, bits: u32) {
        self.state.lock().unwrap().buttons = bits & 0xfff;
    }

    /// `qing_tick` 之后取画面：这一帧交了新的才有，没交（晚了）是 None。
    pub fn take_frame(&self) -> Option<Frame> {
        let mut s = self.state.lock().unwrap();
        if !std::mem::take(&mut s.fresh) {
            return None;
        }
        s.frame.clone()
    }

    /// 最新的一帧，取过也给。截图用。
    pub fn last_frame(&self) -> Option<Frame> {
        self.state.lock().unwrap().frame.clone()
    }

    /// 上次取过以后 app 发的音调。
    pub fn take_tones(&self) -> Vec<Tone> {
        std::mem::take(&mut self.state.lock().unwrap().tones)
    }

    /// 用合成器做点事：声卡要样本、换采样率、全停。
    pub fn synth<R>(&self, f: impl FnOnce(&mut Synth) -> R) -> R {
        f(&mut self.state.lock().unwrap().synth)
    }

    /// 收一帧。`palette` 只有调色板的画面才有。
    fn put_frame(&self, pixels: &[u8], palette: &[u8]) {
        let mut s = self.state.lock().unwrap();
        let format = self.format;
        let f = s.frame.get_or_insert_with(|| Frame::blank(format));
        f.pixels.copy_from_slice(pixels);
        f.palette.copy_from_slice(palette);
        s.fresh = true;
    }

    fn tone(&self, t: Tone) -> bool {
        let mut s = self.state.lock().unwrap();
        let ok = s.synth.tone(t.channel, t.hz, t.ms, t.volume);
        if ok {
            s.tones.push(t);
        }
        ok
    }

    fn tone_wav(&self, wav: &[u8]) -> bool {
        let mut s = self.state.lock().unwrap();
        let ok = s.synth.wav(wav);
        if ok {
            let secs_ms = parse_wav(wav).map(|(x, r)| (x.len() as u64 * 1000 / r as u64) as u32).unwrap_or(0);
            s.tones.push(Tone { channel: 4, hz: 0, ms: secs_ms, volume: 100 });
        }
        ok
    }
}

/// 设备上所有整屏 app 看到的语言：0 英文，1 中文。系统设置换语言时设一次。
static LANG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 换整屏 app 看到的语言：`zh` 开头是中文，别的都是英文。
pub fn set_lang(code: &str) {
    LANG.store(code.starts_with("zh") as u32, std::sync::atomic::Ordering::Relaxed);
}

/// 把整屏页的导入函数接到 linker 上（`frame`、`frame565`、`buttons`、`tone`、`tone_wav`、`lang`）。`get` 从宿主的数据里拿这个 app 的 [`Full`]。
pub fn link<T: 'static>(linker: &mut Linker<T>, get: fn(&T) -> &Arc<Full>) -> anyhow::Result<()> {
    fn mem<T>(c: &mut Caller<'_, T>) -> Option<wasmtime::Memory> {
        c.get_export("memory").and_then(|e| e.into_memory())
    }
    linker.func_wrap("qing", "frame", move |mut c: Caller<'_, T>, px: u32, pal: u32| -> i32 {
        let Some(m) = mem(&mut c) else { return -1 };
        let (data, host) = m.data_and_store_mut(&mut c);
        let (px, pal) = (px as usize, pal as usize);
        let (Some(p), Some(q)) = (data.get(px..px + PIXELS), data.get(pal..pal + PALETTE)) else { return -1 };
        let full = get(host);
        if full.format != Format::Indexed {
            return -1;
        }
        full.put_frame(p, q);
        0
    })?;
    linker.func_wrap("qing", "frame565", move |mut c: Caller<'_, T>, px: u32| -> i32 {
        let Some(m) = mem(&mut c) else { return -1 };
        let (data, host) = m.data_and_store_mut(&mut c);
        let full = get(host);
        if full.format != Format::Rgb565 {
            return -1;
        }
        let Some(p) = data.get(px as usize..px as usize + Format::Rgb565.bytes()) else { return -1 };
        full.put_frame(p, &[]);
        0
    })?;
    linker.func_wrap("qing", "buttons", move |c: Caller<'_, T>| -> u32 { get(c.data()).state.lock().unwrap().buttons })?;
    linker.func_wrap("qing", "tone", move |c: Caller<'_, T>, channel: u32, hz: u32, ms: u32, volume: u32| -> i32 {
        if channel > 3 {
            return -1;
        }
        if get(c.data()).tone(Tone { channel, hz, ms, volume }) {
            0
        } else {
            -1
        }
    })?;
    linker.func_wrap("qing", "tone_wav", move |mut c: Caller<'_, T>, p: u32, n: u32| -> i32 {
        let Some(m) = mem(&mut c) else { return -1 };
        let (data, host) = m.data_and_store_mut(&mut c);
        let Some(wav) = data.get(p as usize..p as usize + n as usize) else { return -1 };
        if get(host).tone_wav(wav) {
            0
        } else {
            -1
        }
    })?;
    linker.func_wrap("qing", "lang", |_: Caller<'_, T>| -> u32 { LANG.load(std::sync::atomic::Ordering::Relaxed) })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 停下：键都算没按，在响的声音停掉（系统层叠上来时游戏不该还在响）。
    #[test]
    fn pause_clears_keys_and_sound() {
        let f = Full::default();
        f.set_buttons(key::A | key::UP);
        f.synth(|s| s.tone(0, 440, 1000, 50));
        f.pause(true);
        assert!(f.paused());
        assert!(!f.synth(|s| s.busy()));
        assert_eq!(f.state.lock().unwrap().buttons, 0);
        f.pause(false);
        assert!(!f.paused());
    }

    fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
        let mut b = Vec::new();
        let n = samples.len() as u32 * 2;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 1, 0]);
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&[2, 0, 16, 0]);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&n.to_le_bytes());
        for s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    #[test]
    fn blit_scales_by_whole_numbers_with_black_borders() {
        let mut f = Frame::default();
        f.palette[3..6].copy_from_slice(&[0xff, 0x80, 0x01]);
        f.pixels[0] = 1;
        // 720×480 正好 3 倍，没有黑边。
        let mut dst = vec![0u32; 720 * 480];
        f.blit(&mut dst, 720, 480);
        assert_eq!(dst[0], 0xff8001);
        assert_eq!(dst[2 * 720 + 2], 0xff8001);
        assert_eq!(dst[3], 0);
        // 800×600：还是 3 倍，左右各 40、上下各 60 的黑边。
        let (k, x0, y0) = fit(800, 600);
        assert_eq!((k, x0, y0), (3, 40, 60));
        let mut dst = vec![7u32; 800 * 600];
        f.blit(&mut dst, 800, 600);
        assert_eq!(dst[60 * 800 + 40], 0xff8001);
        assert_eq!(dst[60 * 800 + 39], 0);
        // 截图的放大和 blit 一样。
        let rgb = f.rgb_scaled(3);
        assert_eq!(rgb.len(), 720 * 480 * 3);
        assert_eq!(&rgb[(2 * 720 + 2) * 3..(2 * 720 + 2) * 3 + 3], &[0xff, 0x80, 0x01]);
        assert_eq!(&rgb[3 * 3..3 * 3 + 3], &[0, 0, 0]);
    }

    /// RGB565 的画面：不放大，放在中间，四周黑边；颜色按高字节在前读。屏放不下就全黑。
    #[test]
    fn rgb565_is_centred_unscaled_with_black_borders() {
        assert_eq!(rgb565(0xff, 0xff), 0xffffff);
        assert_eq!(rgb565(0xf8, 0x00), 0xff0000);
        assert_eq!(rgb565(0x07, 0xe0), 0x00ff00);
        assert_eq!(rgb565(0x00, 0x1f), 0x0000ff);
        assert_eq!(Format::parse("640x480"), Some(Format::Rgb565));
        assert_eq!(Format::parse(""), Some(Format::Indexed));
        assert_eq!(Format::parse("320x240"), None);
        let mut f = Frame::blank(Format::Rgb565);
        assert_eq!(f.pixels.len(), 640 * 480 * 2);
        assert!(f.palette.is_empty());
        f.pixels[..2].copy_from_slice(&[0xf8, 0x00]); // 左上角红
        let last = (640 * 480 - 1) * 2;
        f.pixels[last..].copy_from_slice(&[0x00, 0x1f]); // 右下角蓝
        // 720×480：左右各 40 的黑边，上下没有。
        assert_eq!(Format::Rgb565.fit(720, 480), (1, 40, 0));
        let mut dst = vec![7u32; 720 * 480];
        f.blit(&mut dst, 720, 480);
        assert_eq!(dst[40], 0xff0000);
        assert_eq!(dst[39], 0, "左边黑边");
        assert_eq!(dst[41], 0, "不放大：旁边一个点是下一个像素");
        assert_eq!(dst[479 * 720 + 40 + 639], 0x0000ff);
        assert_eq!(dst[479 * 720 + 40 + 640], 0, "右边黑边");
        // 1280×960 也不放大：放在中间。
        assert_eq!(Format::Rgb565.fit(1280, 960), (1, 320, 240));
        // 比 640×480 小的屏：放不下，全黑。
        assert_eq!(Format::Rgb565.fit(480, 320).0, 0);
        let mut small = vec![7u32; 480 * 320];
        f.blit(&mut small, 480, 320);
        assert!(small.iter().all(|p| *p == 0));
        // 截图：640×480，不放大。
        let rgb = f.rgb_scaled(1);
        assert_eq!(rgb.len(), 640 * 480 * 3);
        assert_eq!(&rgb[..3], &[0xff, 0, 0]);
    }

    /// 一个 app 只交一种画面：`frame565` 只收写了 `frame = "640x480"` 的，`frame` 只收没写的。
    #[test]
    fn each_app_takes_one_kind_of_frame() {
        let f = Full::new(Format::Rgb565);
        assert_eq!(f.format(), Format::Rgb565);
        assert_eq!(f.blank().pixels.len(), 640 * 480 * 2);
        f.put_frame(&vec![0xab; 640 * 480 * 2], &[]);
        let got = f.take_frame().unwrap();
        assert_eq!(got.format, Format::Rgb565);
        assert_eq!(got.pixels[0], 0xab);
        assert!(f.take_frame().is_none(), "不交就不重画");
        assert!(f.last_frame().is_some(), "系统一直有最后交的那一帧");
    }

    #[test]
    fn tones_sound_for_their_length_and_stop() {
        let mut s = Synth::new(1000);
        assert!(s.tone(0, 100, 50, 100));
        assert!(!s.tone(4, 100, 50, 100), "第五个声道只放录音");
        let mut buf = [0.0f32; 60];
        s.render(&mut buf);
        assert!(buf[..50].iter().all(|x| x.abs() > 0.2), "50 毫秒里一直响");
        assert!(buf[50..].iter().all(|x| *x == 0.0), "到点停");
        assert!(!s.busy());
        // 方波 100 Hz、采样率 1000：五个正、五个负。
        s.tone(1, 100, 10, 100);
        s.render(&mut buf[..10]);
        assert!(buf[..5].iter().all(|x| *x > 0.0) && buf[5..10].iter().all(|x| *x < 0.0));
    }

    #[test]
    fn noise_and_triangle_and_wav_mix() {
        let mut s = Synth::new(8000);
        s.tone(2, 440, 100, 100);
        s.tone(3, 4000, 100, 100);
        assert!(s.wav(&wav(&[16000; 800], 8000)));
        let mut buf = vec![0.0f32; 800];
        s.render(&mut buf);
        assert!(buf.iter().any(|x| *x > 0.5), "三个一起响，比一个响");
        assert!(!s.busy(), "都放完了");
        // 超过 2 秒的录音不收，不是 WAV 的不收。
        assert!(!s.wav(&wav(&vec![0; 8000 * 3], 8000)));
        assert!(!s.wav(b"not a wav"));
    }

    #[test]
    fn frame_is_taken_once_after_each_tick() {
        let f = Full::default();
        assert!(f.take_frame().is_none());
        f.put_frame(&[1; PIXELS], &[2; PALETTE]);
        assert_eq!(f.take_frame().unwrap().pixels[0], 1);
        assert!(f.take_frame().is_none(), "这一帧没交新的：晚了不补");
        assert!(f.last_frame().is_some(), "截图还拿得到");
        assert_eq!(key::of("Start"), Some(key::START));
    }
}
