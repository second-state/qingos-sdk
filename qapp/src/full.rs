//! 整屏页（ADR 0030）：每帧读按键、画一帧交给系统、发音调。
//!
//! 清单写 `screen = "full"` 和 `rate_hz = 60`。系统每帧调一次 [`crate::App::every`] 给的函数，调完取画面。
//! 状态都放在 app 自己的内存里（这里的 [`Keys`]、[`Frame`] 也是），系统存下整块内存就是一个存档。
//!
//! ```ignore
//! use qapp::full::{key, Frame, Keys};
//! static S: Mutex<(Frame, Keys, i32)> = ...;
//! app.every(|| {
//!     let mut s = S.lock().unwrap();
//!     s.1.read();
//!     if s.1.held(key::RIGHT) { s.2 += 1; }
//!     if s.1.pressed(key::A) { qapp::full::tone(qapp::full::Channel::Square1, 880, 100, 60); }
//!     s.0.clear(0);
//!     s.0.rect(s.2, 72, 16, 16, 1);
//!     s.0.show();
//! });
//! ```

mod draw;
mod font;
mod music;
pub use draw::{Camera, Sprite, TileMap};
pub use font::{Font, Glyph, LINE};
pub use music::{hz, Music, Note, Song, Track};

/// 画面的宽和高：放大 3 倍是 720×480。
pub const W: usize = 240;
pub const H: usize = 160;

/// 按键的位，和系统的一样（`buttons()` 回的位图）。
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
}

/// 合成器的四个声道。第五个放短录音，用 [`tone_wav`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Square1 = 0,
    Square2 = 1,
    Triangle = 2,
    Noise = 3,
}

#[cfg(target_arch = "wasm32")]
mod ffi {
    #[link(wasm_import_module = "qing")]
    extern "C" {
        pub fn frame(pixels: u32, palette: u32) -> i32;
        pub fn buttons() -> u32;
        pub fn tone(channel: u32, hz: u32, ms: u32, volume: u32) -> i32;
        pub fn tone_wav(ptr: u32, len: u32) -> i32;
    }
}

/// 这一帧和上一帧的按键状态。每帧开头 [`Keys::read`] 一次。
#[derive(Debug, Default, Clone, Copy)]
pub struct Keys {
    now: u32,
    before: u32,
}

impl Keys {
    /// 读这一帧的按键。一帧只读一次，「刚按下」才对。
    pub fn read(&mut self) {
        self.set(buttons());
    }

    /// 用给定的位图当这一帧（测试、回放用）。
    pub fn set(&mut self, bits: u32) {
        self.before = self.now;
        self.now = bits;
    }

    /// 按着。
    pub fn held(&self, k: u32) -> bool {
        self.now & k != 0
    }

    /// 这一帧刚按下（上一帧没按）。
    pub fn pressed(&self, k: u32) -> bool {
        self.now & !self.before & k != 0
    }

    /// 这一帧刚松开。
    pub fn released(&self, k: u32) -> bool {
        !self.now & self.before & k != 0
    }

    /// 这一帧的位图。
    pub fn bits(&self) -> u32 {
        self.now
    }
}

/// 一帧：240×160 个像素，一个像素是调色板的一格；调色板 256 格，app 自己定。
pub struct Frame {
    pub pixels: Vec<u8>,
    pub palette: [[u8; 3]; 256],
}

impl Default for Frame {
    fn default() -> Self {
        Frame { pixels: vec![0; W * H], palette: [[0; 3]; 256] }
    }
}

impl Frame {
    /// 全涂成一格颜色。
    pub fn clear(&mut self, c: u8) {
        self.pixels.fill(c);
    }

    /// 画一个点。出了画面不画。
    pub fn set(&mut self, x: i32, y: i32, c: u8) {
        if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
            self.pixels[y as usize * W + x as usize] = c;
        }
    }

    /// 一个点的颜色格。出了画面是 None。
    pub fn get(&self, x: i32, y: i32) -> Option<u8> {
        ((0..W as i32).contains(&x) && (0..H as i32).contains(&y)).then(|| self.pixels[y as usize * W + x as usize])
    }

    /// 画一个实心的方块。出了画面的部分不画。
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u8) {
        let (x0, x1) = (x.clamp(0, W as i32), (x + w).min(W as i32));
        let (y0, y1) = (y.max(0), (y + h).min(H as i32));
        for yy in y0..y1 {
            let row = yy as usize * W;
            self.pixels[row + x0 as usize..row + x1.max(x0) as usize].fill(c);
        }
    }

    /// 把这一帧交给系统。一帧交一次，在 `every` 的函数里交。
    pub fn show(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            ffi::frame(self.pixels.as_ptr() as u32, self.palette.as_ptr() as u32) == 0
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            native::FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            true
        }
    }
}

/// 这一帧哪些键按着。用 [`Keys`] 才知道「刚按下」。
pub fn buttons() -> u32 {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        ffi::buttons()
    }
    #[cfg(not(target_arch = "wasm32"))]
    native::KEYS.load(std::sync::atomic::Ordering::Relaxed)
}

/// 发一个音调：声道、频率、多少毫秒、音量 0–100。`ms` 是 0 就停掉这个声道。
/// 同一个声道上的新音调顶掉在响的。
pub fn tone(ch: Channel, hz: u32, ms: u32, volume: u32) -> bool {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        ffi::tone(ch as u32, hz, ms, volume) == 0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::TONES.lock().unwrap().push((ch as u32, hz, ms, volume));
        true
    }
}

/// 在第五个声道放一小段 WAV（16 位 PCM，最长 2 秒），和音调一起混着响。长的用 [`crate::host::sound`]。
pub fn tone_wav(wav: &[u8]) -> bool {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        ffi::tone_wav(wav.as_ptr() as u32, wav.len() as u32) == 0
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        wav.len() >= 44 && &wav[..4] == b"RIFF"
    }
}

/// 开发机上（不是 wasm）测 app 用：设按键、看交了几帧、发了哪些音调。
#[cfg(not(target_arch = "wasm32"))]
pub mod native {
    use std::sync::atomic::{AtomicU32, AtomicU64};
    use std::sync::Mutex;
    pub static KEYS: AtomicU32 = AtomicU32::new(0);
    pub static FRAMES: AtomicU64 = AtomicU64::new(0);
    /// 声道、频率、毫秒、音量。
    pub static TONES: Mutex<Vec<(u32, u32, u32, u32)>> = Mutex::new(Vec::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressed_is_only_the_first_frame() {
        let mut k = Keys::default();
        k.set(key::A | key::RIGHT);
        assert!(k.pressed(key::A) && k.held(key::RIGHT));
        k.set(key::A);
        assert!(!k.pressed(key::A) && k.held(key::A));
        assert!(k.released(key::RIGHT));
        k.set(0);
        k.set(key::A);
        assert!(k.pressed(key::A));
    }

    #[test]
    fn rect_is_clipped_to_the_frame() {
        let mut f = Frame::default();
        f.rect(-4, -4, 8, 8, 3);
        assert_eq!(f.get(0, 0), Some(3));
        assert_eq!(f.get(3, 3), Some(3));
        assert_eq!(f.get(4, 4), Some(0));
        f.rect(236, 156, 100, 100, 5);
        assert_eq!(f.get(239, 159), Some(5));
        f.rect(300, 0, 10, 10, 9);
        // 最底下一行右边出界：以前切片越过整块像素会崩。
        f.rect(250, 159, 10, 1, 9);
        assert_eq!(f.get(-1, 0), None);
        assert!(!f.pixels.contains(&9));
    }
}
