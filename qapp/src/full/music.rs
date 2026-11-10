//! 音乐：音符数据，每帧按节奏发音调。系统只认音调，不认「音乐」。
//!
//! 一首曲子分几轨，一轨占一个声道。音符写成字：`C4 E4 G4:2 r A#3 Bb3`。
//! 字母是音名，`#` 升、`b` 降，数字是八度（A4 是 440 Hz）。`r` 是休止。`:2` 是占几格，不写是 1 格。
//! 一格多少帧由曲子定。音效用 [`Music::sfx`] 发：它让音乐在那个声道上让出一会儿，响完音乐再接上。
//!
//! ```
//! use qapp::full::{Channel, Music, Song, Track};
//! let song = Song::new(8, true, vec![Track::parse(Channel::Square1, 40, "C4 E4 G4:2").unwrap()]);
//! let mut m = Music::default();
//! m.play(song);
//! m.tick(); // 每帧调一次
//! ```

use super::{tone, Channel};

/// 一个音符：MIDI 音高（60 是 C4，0 是休止）、占几格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub pitch: u8,
    pub len: u8,
}

/// MIDI 音高换成频率，四舍五入到整数 Hz。0 是休止，回 0。
pub fn hz(pitch: u8) -> u32 {
    if pitch == 0 {
        return 0;
    }
    (440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0)).round() as u32
}

/// 一轨：一个声道、一个音量、一串音符。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub ch: Channel,
    pub volume: u32,
    pub notes: Vec<Note>,
}

impl Track {
    /// 从字读一轨（写法见模块说明）。写错了回哪个词错了。
    pub fn parse(ch: Channel, volume: u32, text: &str) -> Result<Track, String> {
        let mut notes = Vec::new();
        for word in text.split_whitespace() {
            let (name, len) = match word.split_once(':') {
                Some((n, l)) => (n, l.parse::<u8>().map_err(|_| format!("长度不对：{word}"))?),
                None => (word, 1),
            };
            let pitch = if name == "r" { 0 } else { pitch_of(name).ok_or_else(|| format!("音名不对：{word}"))? };
            notes.push(Note { pitch, len: len.max(1) });
        }
        Ok(Track { ch, volume, notes })
    }

    /// 这一轨一共几格。
    pub fn steps(&self) -> u32 {
        self.notes.iter().map(|n| n.len as u32).sum()
    }
}

fn pitch_of(name: &str) -> Option<u8> {
    let mut cs = name.chars();
    let base: i32 = match cs.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = cs.collect();
    let (shift, oct) = match rest.chars().next()? {
        '#' => (1, &rest[1..]),
        'b' => (-1, &rest[1..]),
        _ => (0, &rest[..]),
    };
    let oct: i32 = oct.parse().ok()?;
    let p = (oct + 1) * 12 + base + shift;
    (1..=127).contains(&p).then_some(p as u8)
}

/// 一首曲子：一格几帧、放完要不要从头再来、几轨。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Song {
    pub frames_per_step: u32,
    pub looped: bool,
    pub tracks: Vec<Track>,
}

impl Song {
    pub fn new(frames_per_step: u32, looped: bool, tracks: Vec<Track>) -> Song {
        Song { frames_per_step: frames_per_step.max(1), looped, tracks }
    }
}

/// 放曲子的。状态都在 app 自己的内存里，随时存档时跟着存。
#[derive(Debug, Default, Clone)]
pub struct Music {
    song: Option<Song>,
    /// 从开头算放到第几帧。
    frame: u32,
    /// 每个声道还要让给音效几帧。
    hold: [u32; 4],
    /// 静音：曲子照走，不发音调。
    pub muted: bool,
}

impl Music {
    /// 从头放一首。在放的那首停掉。
    pub fn play(&mut self, song: Song) {
        self.stop();
        self.song = Some(song);
        self.frame = 0;
    }

    /// 停下，在响的音也停掉。
    pub fn stop(&mut self) {
        if let Some(s) = self.song.take() {
            for t in &s.tracks {
                tone(t.ch, 0, 0, 0);
            }
        }
    }

    /// 还在放。不循环的曲子放完就不在放了。
    pub fn playing(&self) -> bool {
        self.song.is_some()
    }

    /// 每帧调一次：到了一个音符的开头就发音调。
    pub fn tick(&mut self) {
        for h in &mut self.hold {
            *h = h.saturating_sub(1);
        }
        let Some(song) = &self.song else { return };
        let fps = song.frames_per_step;
        let total = song.tracks.iter().map(|t| t.steps()).max().unwrap_or(0) * fps;
        if total == 0 {
            self.song = None;
            return;
        }
        if self.frame >= total {
            if !song.looped {
                self.song = None;
                return;
            }
            self.frame = 0;
        }
        if self.frame.is_multiple_of(fps) {
            let step = self.frame / fps;
            for t in &song.tracks {
                let mut at = 0;
                for n in &t.notes {
                    if at == step {
                        if n.pitch != 0 && !self.muted && self.hold[t.ch as usize] == 0 {
                            // 音长短一点，两个一样的音之间听得出断开。
                            let ms = n.len as u32 * fps * 1000 / 60;
                            tone(t.ch, hz(n.pitch), ms - ms / 8, t.volume);
                        }
                        break;
                    }
                    at += n.len as u32;
                    if at > step {
                        break;
                    }
                }
            }
        }
        self.frame += 1;
    }

    /// 发一个音效：在 `ch` 上响 `ms` 毫秒，这段时间里音乐不在这个声道上发音。
    pub fn sfx(&mut self, ch: Channel, hz: u32, ms: u32, volume: u32) {
        tone(ch, hz, ms, volume);
        self.hold[ch as usize] = self.hold[ch as usize].max(ms * 60 / 1000 + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::full::native::TONES;

    #[test]
    fn note_names_and_hz() {
        assert_eq!(hz(69), 440);
        assert_eq!(hz(60), 262);
        assert_eq!(pitch_of("C4"), Some(60));
        assert_eq!(pitch_of("A#3"), Some(58));
        assert_eq!(pitch_of("Bb3"), Some(58));
        assert_eq!(pitch_of("H4"), None);
        assert!(Track::parse(Channel::Square1, 50, "C4 r:2 x9").is_err());
    }

    #[test]
    fn plays_on_the_beat_loops_and_yields_to_sfx() {
        TONES.lock().unwrap().clear();
        let t = Track::parse(Channel::Triangle, 30, "C4 r E4:2").unwrap();
        assert_eq!(t.steps(), 4);
        let mut m = Music::default();
        m.play(Song::new(10, true, vec![t]));
        for _ in 0..40 {
            m.tick();
        }
        let tones: Vec<_> = TONES.lock().unwrap().drain(..).filter(|t| t.1 != 0).collect();
        // 第 0 帧 C4，第 20 帧 E4（两格，333 毫秒的 7/8）。
        assert_eq!(tones, [(2, 262, 146, 30), (2, 330, 292, 30)]);
        // 又从头：音效占着三角波时，音乐不发。
        m.sfx(Channel::Triangle, 1000, 100, 50);
        m.tick();
        let tones: Vec<_> = TONES.lock().unwrap().drain(..).collect();
        assert_eq!(tones, [(2, 1000, 100, 50)]);
        m.play(Song::new(1, false, vec![Track::parse(Channel::Square1, 10, "C5").unwrap()]));
        m.tick();
        m.tick();
        assert!(!m.playing());
    }
}
