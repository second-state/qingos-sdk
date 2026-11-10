//! 精灵、地图块、摄像机：整屏页上画像素画用。
//!
//! 约定：精灵和地图块里颜色格 0 是透明，不画。所以调色板第 0 格只当背景用（`Frame::clear(0)`）。
//! 坐标有两种：世界坐标（关卡里的位置）和画面坐标（0..240、0..160）。摄像机把前者换成后者。

use super::{Frame, H, W};

/// 一张精灵：`w`×`h` 个像素，一个字节一格颜色，0 是透明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u8>,
}

impl Sprite {
    /// 从字符画读一张精灵：每行一样长，一个字符一个像素。
    /// `colors` 说哪个字符是哪格颜色。不在里面的字符（比如 `.`）是透明。
    ///
    /// ```
    /// use qapp::full::Sprite;
    /// let s = Sprite::art(&[".y.", "yoy"], &[('y', 1), ('o', 2)]);
    /// assert_eq!((s.w, s.h), (3, 2));
    /// assert_eq!(s.at(1, 1), 2);
    /// ```
    pub fn art(rows: &[&str], colors: &[(char, u8)]) -> Sprite {
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let mut px = vec![0; w * rows.len()];
        for (y, r) in rows.iter().enumerate() {
            for (x, ch) in r.chars().enumerate() {
                px[y * w + x] = colors.iter().find(|(k, _)| *k == ch).map_or(0, |c| c.1);
            }
        }
        Sprite { w: w as i32, h: rows.len() as i32, px }
    }

    /// 一个像素的颜色格。出了范围是 0。
    pub fn at(&self, x: i32, y: i32) -> u8 {
        if (0..self.w).contains(&x) && (0..self.h).contains(&y) {
            self.px[(y * self.w + x) as usize]
        } else {
            0
        }
    }

    /// 放大 `k` 倍出一张新的（标题上的大字、大怪）。
    pub fn scaled(&self, k: i32) -> Sprite {
        let k = k.max(1);
        let (w, h) = (self.w * k, self.h * k);
        let px = (0..h).flat_map(|y| (0..w).map(move |x| (x / k, y / k))).map(|(x, y)| self.at(x, y)).collect();
        Sprite { w, h, px }
    }

    /// 换几格颜色出一张新的（同一个角色的另一种颜色）。
    pub fn recolor(&self, map: &[(u8, u8)]) -> Sprite {
        let px = self.px.iter().map(|&c| map.iter().find(|(a, _)| *a == c).map_or(c, |m| m.1)).collect();
        Sprite { px, ..*self }
    }
}

impl Frame {
    /// 把精灵画在画面的 (x, y)（左上角）。`flip` 是左右翻过来画。0 不画，出了画面的部分不画。
    pub fn blit(&mut self, s: &Sprite, x: i32, y: i32, flip: bool) {
        let (x0, x1) = ((-x).max(0), (W as i32 - x).min(s.w));
        let (y0, y1) = ((-y).max(0), (H as i32 - y).min(s.h));
        for sy in y0..y1 {
            let row = (y + sy) as usize * W;
            for sx in x0..x1 {
                let c = s.px[(sy * s.w + if flip { s.w - 1 - sx } else { sx }) as usize];
                if c != 0 {
                    self.pixels[row + (x + sx) as usize] = c;
                }
            }
        }
    }

    /// 画一个空心的方框，线宽一个像素。
    pub fn frame_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u8) {
        self.rect(x, y, w, 1, c);
        self.rect(x, y + h - 1, w, 1, c);
        self.rect(x, y, 1, h, c);
        self.rect(x + w - 1, y, 1, h, c);
    }
}

/// 摄像机：世界里画面左上角的位置。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Camera {
    pub x: i32,
    pub y: i32,
}

impl Camera {
    /// 跟着世界里的一点 (tx, ty) 走。这一点在画面中间的一块框里时，摄像机不动；出了框才跟。
    /// 框是画面的中间三分之一（横）和中间一半（竖）。摄像机不出世界（`world_w`×`world_h` 个像素）。
    pub fn follow(&mut self, tx: i32, ty: i32, world_w: i32, world_h: i32) {
        let (l, r) = (W as i32 / 3, W as i32 * 2 / 3);
        let (t, b) = (H as i32 / 4, H as i32 * 3 / 4);
        if tx - self.x < l {
            self.x = tx - l;
        }
        if tx - self.x > r {
            self.x = tx - r;
        }
        if ty - self.y < t {
            self.y = ty - t;
        }
        if ty - self.y > b {
            self.y = ty - b;
        }
        self.clamp(world_w, world_h);
    }

    /// 摄像机放回世界里面。世界比画面小时贴左上。
    pub fn clamp(&mut self, world_w: i32, world_h: i32) {
        self.x = self.x.min(world_w - W as i32).max(0);
        self.y = self.y.min(world_h - H as i32).max(0);
    }

    /// 世界坐标换成画面坐标。
    pub fn to_screen(&self, wx: i32, wy: i32) -> (i32, i32) {
        (wx - self.x, wy - self.y)
    }

    /// 世界里这块方框有没有一部分在画面里。不在的不用画、不用算。
    pub fn sees(&self, wx: i32, wy: i32, w: i32, h: i32) -> bool {
        wx + w > self.x && wx < self.x + W as i32 && wy + h > self.y && wy < self.y + H as i32
    }
}

/// 地图块：一格一个字节，0 是空。`tile` 是一格多少像素（常用 8 或 16）。
/// 格里的数是 `draw` 时 `tiles` 的下标：格是 3 就画 `tiles[3]`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileMap {
    pub w: i32,
    pub h: i32,
    pub tile: i32,
    pub cells: Vec<u8>,
}

impl TileMap {
    /// 一张空的地图。
    pub fn new(w: i32, h: i32, tile: i32) -> TileMap {
        TileMap { w, h, tile, cells: vec![0; (w * h).max(0) as usize] }
    }

    /// 从字符画读：一个字符一格，`key` 说哪个字符是哪格。行不一样长时短的补空。
    pub fn parse(rows: &[&str], tile: i32, key: impl Fn(char) -> u8) -> TileMap {
        let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as i32;
        let mut m = TileMap::new(w, rows.len() as i32, tile);
        for (y, r) in rows.iter().enumerate() {
            for (x, ch) in r.chars().enumerate() {
                m.set(x as i32, y as i32, key(ch));
            }
        }
        m
    }

    /// 一格。出了地图是 0。
    pub fn get(&self, tx: i32, ty: i32) -> u8 {
        if (0..self.w).contains(&tx) && (0..self.h).contains(&ty) {
            self.cells[(ty * self.w + tx) as usize]
        } else {
            0
        }
    }

    /// 改一格。出了地图不改。
    pub fn set(&mut self, tx: i32, ty: i32, v: u8) {
        if (0..self.w).contains(&tx) && (0..self.h).contains(&ty) {
            self.cells[(ty * self.w + tx) as usize] = v;
        }
    }

    /// 世界里一个像素落在哪一格上，回那一格的值。
    pub fn at_px(&self, x: i32, y: i32) -> u8 {
        self.get(x.div_euclid(self.tile), y.div_euclid(self.tile))
    }

    /// 地图有多少像素宽、高。
    pub fn px_size(&self) -> (i32, i32) {
        (self.w * self.tile, self.h * self.tile)
    }

    /// 世界里这块方框碰到的格里，有没有一格让 `solid` 说是。碰撞用。
    pub fn hits(&self, x: i32, y: i32, w: i32, h: i32, solid: impl Fn(u8) -> bool) -> bool {
        let (tx0, tx1) = (x.div_euclid(self.tile), (x + w - 1).div_euclid(self.tile));
        let (ty0, ty1) = (y.div_euclid(self.tile), (y + h - 1).div_euclid(self.tile));
        (ty0..=ty1).any(|ty| (tx0..=tx1).any(|tx| solid(self.get(tx, ty))))
    }

    /// 照摄像机画画面里看得见的格。格的值超出 `tiles` 的不画。
    pub fn draw(&self, f: &mut Frame, tiles: &[Sprite], cam: &Camera) {
        let t = self.tile;
        let (tx0, ty0) = (cam.x.div_euclid(t), cam.y.div_euclid(t));
        for ty in ty0..=ty0 + H as i32 / t + 1 {
            for tx in tx0..=tx0 + W as i32 / t + 1 {
                let v = self.get(tx, ty) as usize;
                if v != 0 && v < tiles.len() {
                    f.blit(&tiles[v], tx * t - cam.x, ty * t - cam.y, false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blit_skips_zero_and_flips() {
        let s = Sprite::art(&["ab."], &[('a', 1), ('b', 2)]);
        let mut f = Frame::default();
        f.clear(9);
        f.blit(&s, 0, 0, false);
        assert_eq!((f.get(0, 0), f.get(1, 0), f.get(2, 0)), (Some(1), Some(2), Some(9)));
        f.blit(&s, 10, 0, true);
        assert_eq!((f.get(10, 0), f.get(11, 0), f.get(12, 0)), (Some(9), Some(2), Some(1)));
        // 出了画面不画，也不出错。
        f.blit(&s, -2, -1, false);
        f.blit(&s, 239, 159, false);
        assert_eq!(f.get(239, 159), Some(1));
        let big = s.scaled(2).recolor(&[(1, 5)]);
        assert_eq!((big.w, big.h, big.at(1, 1), big.at(2, 0), big.at(5, 1)), (6, 2, 5, 2, 0));
    }

    #[test]
    fn camera_follows_out_of_the_box_and_stays_in_the_world() {
        let mut c = Camera::default();
        c.follow(100, 80, 1000, 160);
        assert_eq!((c.x, c.y), (0, 0));
        c.follow(300, 80, 1000, 160);
        assert_eq!(c.x, 300 - 160);
        c.follow(990, 80, 1000, 160);
        assert_eq!(c.x, 1000 - 240);
        assert_eq!(c.to_screen(800, 10), (40, 10));
        assert!(c.sees(1000 - 241, 0, 2, 2) && !c.sees(0, 0, 10, 10));
    }

    #[test]
    fn tilemap_hits_and_draws_what_the_camera_sees() {
        let m = TileMap::parse(&["....", "..#.", "####"], 16, |c| (c == '#') as u8);
        assert_eq!((m.w, m.h, m.px_size()), (4, 3, (64, 48)));
        assert!(m.hits(30, 14, 4, 4, |v| v == 1));
        assert!(!m.hits(0, 0, 16, 16, |v| v == 1));
        assert_eq!(m.at_px(40, 20), 1);
        assert_eq!(m.get(-1, 0), 0);
        let tiles = [Sprite::art(&[], &[]), Sprite { w: 16, h: 16, px: vec![7; 256] }];
        let mut f = Frame::default();
        m.draw(&mut f, &tiles, &Camera { x: 8, y: 0 });
        assert_eq!(f.get(0, 32), Some(7));
        // 第 2 列那格在世界的 x=32，摄像机往右 8，画在 24。
        assert_eq!(f.get(24, 16), Some(7));
        assert_eq!(f.get(23, 16), Some(0));
    }
}
