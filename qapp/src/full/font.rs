//! 点阵字：一个字是 12 行，一行是一个 `u16` 的位，最高位在最左边。
//!
//! 字形不在这里：app 自己带一个 `Font`（一张排好序的 `Glyph` 表）。

use super::{Frame, Sprite};

/// 一个字：哪个字、往右走几个像素、12 行点阵。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub ch: char,
    pub w: u8,
    pub rows: [u16; 12],
}

/// 一套烘好的字：按字排好序，查的时候二分。
#[derive(Debug, Clone, Copy)]
pub struct Font {
    pub glyphs: &'static [Glyph],
}

/// 一行字多高。
pub const LINE: i32 = 12;

impl Font {
    /// 找一个字。没烘进来的回 None。
    pub fn glyph(&self, ch: char) -> Option<&Glyph> {
        self.glyphs.binary_search_by(|g| g.ch.cmp(&ch)).ok().map(|i| &self.glyphs[i])
    }

    /// 一行字画出来多宽。没烘进来的字按 6 算。
    pub fn width(&self, s: &str) -> i32 {
        s.chars().map(|c| self.glyph(c).map_or(6, |g| g.w as i32)).sum()
    }

    /// 把一行字变成一张精灵（颜色格 `c`，别处透明）。要大字就再 `scaled`。
    pub fn sprite(&self, s: &str, c: u8) -> Sprite {
        let w = self.width(s);
        let mut f = Frame::default();
        f.text(self, 0, 0, s, c);
        let px = (0..LINE).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| f.get(x, y).unwrap_or(0)).collect();
        Sprite { w, h: LINE, px }
    }
}

impl Frame {
    /// 在 (x, y)（左上角）写一行字，回写完往右走了多少。没烘进来的字画一个空心的框，一眼看得出漏了。
    pub fn text(&mut self, font: &Font, x: i32, y: i32, s: &str, c: u8) -> i32 {
        let mut at = x;
        for ch in s.chars() {
            match font.glyph(ch) {
                Some(g) => {
                    for (row, bits) in g.rows.iter().enumerate() {
                        for col in 0..16 {
                            if bits & (0x8000 >> col) != 0 {
                                self.set(at + col, y + row as i32, c);
                            }
                        }
                    }
                    at += g.w as i32;
                }
                None => {
                    self.frame_rect(at, y + 1, 5, 10, c);
                    at += 6;
                }
            }
        }
        at - x
    }

    /// 写一行字，横着放在画面中间。
    pub fn text_center(&mut self, font: &Font, y: i32, s: &str, c: u8) {
        let x = (super::W as i32 - font.width(s)) / 2;
        self.text(font, x, y, s, c);
    }

    /// 带一圈描边写字：先在四周八个方向用 `edge` 写，再用 `c` 写在中间。字压在花的背景上也看得清。
    pub fn text_outlined(&mut self, font: &Font, x: i32, y: i32, s: &str, c: u8, edge: u8) -> i32 {
        for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            self.text(font, x + dx, y + dy, s, edge);
        }
        self.text(font, x, y, s, c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static GLYPHS: [Glyph; 2] = [
        Glyph { ch: '!', w: 4, rows: [0x4000, 0x4000, 0x4000, 0, 0x4000, 0, 0, 0, 0, 0, 0, 0] },
        Glyph { ch: '鸭', w: 12, rows: [0x8010; 12] },
    ];
    const F: Font = Font { glyphs: &GLYPHS };

    #[test]
    fn text_draws_baked_glyphs_and_a_box_for_missing_ones() {
        assert!(F.glyph('鸭').is_some() && F.glyph('鹅').is_none());
        assert_eq!(F.width("!鸭?"), 4 + 12 + 6);
        let mut f = Frame::default();
        let w = f.text(&F, 10, 20, "!鸭?", 3);
        assert_eq!(w, 22);
        assert_eq!(f.get(11, 20), Some(3));
        assert_eq!(f.get(11, 23), Some(0));
        assert_eq!(f.get(14, 31), Some(3));
        assert_eq!(f.get(14 + 11, 31), Some(3));
        // 没烘进来的 ? 画成框：左上角在 (26, 21)。
        assert_eq!(f.get(26, 21), Some(3));
        assert_eq!(f.get(28, 25), Some(0));
        let s = F.sprite("!", 7);
        assert_eq!((s.w, s.h, s.at(1, 0), s.at(0, 0)), (4, 12, 7, 0));
    }
}
