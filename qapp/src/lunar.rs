//! 农历：公历日期换成农历月日、干支年、生肖。给 app 用（系统不画农历，要的 app 自己钉卡）。
//! 查表法，1900 到 2050。节气用通式算（寿星公式），1901 到 2100，个别年份差一天。

use chrono::NaiveDate;

/// 每年一个数：低 4 位是闰几月（0 没有），第 4 到 15 位是十二个月大小（1 = 30 天，最高位是正月），
/// 第 16 位是闰月大小。基准 1900-01-31 是庚子年正月初一。
const INFO: [u32; 151] = [
    0x04bd8, 0x04ae0, 0x0a570, 0x054d5, 0x0d260, 0x0d950, 0x16554, 0x056a0, 0x09ad0, 0x055d2, // 1900
    0x04ae0, 0x0a5b6, 0x0a4d0, 0x0d250, 0x1d255, 0x0b540, 0x0d6a0, 0x0ada2, 0x095b0, 0x14977, // 1910
    0x04970, 0x0a4b0, 0x0b4b5, 0x06a50, 0x06d40, 0x1ab54, 0x02b60, 0x09570, 0x052f2, 0x04970, // 1920
    0x06566, 0x0d4a0, 0x0ea50, 0x06e95, 0x05ad0, 0x02b60, 0x186e3, 0x092e0, 0x1c8d7, 0x0c950, // 1930
    0x0d4a0, 0x1d8a6, 0x0b550, 0x056a0, 0x1a5b4, 0x025d0, 0x092d0, 0x0d2b2, 0x0a950, 0x0b557, // 1940
    0x06ca0, 0x0b550, 0x15355, 0x04da0, 0x0a5b0, 0x14573, 0x052b0, 0x0a9a8, 0x0e950, 0x06aa0, // 1950
    0x0aea6, 0x0ab50, 0x04b60, 0x0aae4, 0x0a570, 0x05260, 0x0f263, 0x0d950, 0x05b57, 0x056a0, // 1960
    0x096d0, 0x04dd5, 0x04ad0, 0x0a4d0, 0x0d4d4, 0x0d250, 0x0d558, 0x0b540, 0x0b6a0, 0x195a6, // 1970
    0x095b0, 0x049b0, 0x0a974, 0x0a4b0, 0x0b27a, 0x06a50, 0x06d40, 0x0af46, 0x0ab60, 0x09570, // 1980
    0x04af5, 0x04970, 0x064b0, 0x074a3, 0x0ea50, 0x06b58, 0x05ac0, 0x0ab60, 0x096d5, 0x092e0, // 1990
    0x0c960, 0x0d954, 0x0d4a0, 0x0da50, 0x07552, 0x056a0, 0x0abb7, 0x025d0, 0x092d0, 0x0cab5, // 2000
    0x0a950, 0x0b4a0, 0x0baa4, 0x0ad50, 0x055d9, 0x04ba0, 0x0a5b0, 0x15176, 0x052b0, 0x0a930, // 2010
    0x07954, 0x06aa0, 0x0ad50, 0x05b52, 0x04b60, 0x0a6e6, 0x0a4e0, 0x0d260, 0x0ea65, 0x0d530, // 2020
    0x05aa0, 0x076a3, 0x096d0, 0x04afb, 0x04ad0, 0x0a4d0, 0x1d0b6, 0x0d250, 0x0d520, 0x0dd45, // 2030
    0x0b5a0, 0x056d0, 0x055b2, 0x049b0, 0x0a577, 0x0a4b0, 0x0aa50, 0x1b255, 0x06d20, 0x0ada0, // 2040
    0x14b63, // 2050
];

const STEMS: [&str; 10] = ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
const BRANCHES: [&str; 12] = ["子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥"];
const ANIMALS: [&str; 12] = ["鼠", "牛", "虎", "兔", "龙", "蛇", "马", "羊", "猴", "鸡", "狗", "猪"];
const MONTHS: [&str; 12] = ["正", "二", "三", "四", "五", "六", "七", "八", "九", "十", "冬", "腊"];
const DAYS: [&str; 30] = [
    "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十", "十一", "十二", "十三", "十四", "十五",
    "十六", "十七", "十八", "十九", "二十", "廿一", "廿二", "廿三", "廿四", "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lunar {
    pub year: i32,
    /// 1 到 12。
    pub month: u32,
    pub leap: bool,
    /// 1 到 30。
    pub day: u32,
}

fn leap_month(y: i32) -> u32 {
    INFO[(y - 1900) as usize] & 0xf
}

fn leap_days(y: i32) -> u32 {
    if leap_month(y) == 0 {
        0
    } else if INFO[(y - 1900) as usize] & 0x10000 != 0 {
        30
    } else {
        29
    }
}

fn month_days(y: i32, m: u32) -> u32 {
    if INFO[(y - 1900) as usize] & (0x10000 >> m) != 0 {
        30
    } else {
        29
    }
}

fn year_days(y: i32) -> u32 {
    (1..=12).map(|m| month_days(y, m)).sum::<u32>() + leap_days(y)
}

/// 表外的日期是 None。
pub fn from_date(d: NaiveDate) -> Option<Lunar> {
    let base = NaiveDate::from_ymd_opt(1900, 1, 31)?;
    let mut offset = (d - base).num_days();
    if offset < 0 {
        return None;
    }
    let mut year = 1900;
    while year <= 2050 && offset >= year_days(year) as i64 {
        offset -= year_days(year) as i64;
        year += 1;
    }
    if year > 2050 {
        return None;
    }
    let lm = leap_month(year);
    let mut month = 1;
    let mut leap = false;
    loop {
        let days = if leap { leap_days(year) } else { month_days(year, month) };
        if offset < days as i64 {
            break;
        }
        offset -= days as i64;
        if !leap && month == lm {
            leap = true;
        } else {
            leap = false;
            month += 1;
        }
    }
    Some(Lunar { year, month, leap, day: offset as u32 + 1 })
}

impl Lunar {
    /// 「八月初六」「闰二月初一」。
    pub fn month_day(&self) -> String {
        format!("{}{}月{}", if self.leap { "闰" } else { "" }, MONTHS[self.month as usize - 1], DAYS[self.day as usize - 1])
    }

    /// 「丙午马年」。
    pub fn year_name(&self) -> String {
        let i = (self.year - 1900 + 36).rem_euclid(60) as usize;
        format!("{}{}{}年", STEMS[i % 10], BRANCHES[i % 12], ANIMALS[i % 12])
    }
}

/// 二十四节气，从小寒数起（公历一月的第一个）。
pub const TERMS: [&str; 24] = [
    "小寒", "大寒", "立春", "雨水", "惊蛰", "春分", "清明", "谷雨", "立夏", "小满", "芒种", "夏至",
    "小暑", "大暑", "立秋", "处暑", "白露", "秋分", "寒露", "霜降", "立冬", "小雪", "大雪", "冬至",
];
// 寿星公式的世纪常数：日 = [Y × 0.2422 + C] − [Y / 4]，Y 是年份后两位；一二月的用 [(Y − 1) / 4]。
const C20: [f64; 24] = [6.11, 20.84, 4.6295, 19.4599, 6.3826, 21.4155, 5.59, 20.888, 6.318, 21.86, 6.5, 22.2, 7.928, 23.65, 8.35, 23.95, 8.44, 23.822, 9.098, 24.218, 8.218, 23.08, 7.9, 22.6];
const C21: [f64; 24] = [5.4055, 20.12, 3.87, 18.73, 5.63, 20.646, 4.81, 20.1, 5.52, 21.04, 5.678, 21.37, 7.108, 22.83, 7.5, 23.13, 7.646, 23.042, 8.318, 23.438, 7.438, 22.36, 7.18, 21.94];

/// 每个节气一句话（自己写的，不是古诗），给屏上当辅助文字。
pub fn term_line(name: &str) -> &'static str {
    match name {
        "小寒" => "天冷了，出门多穿一件",
        "大寒" => "一年最冷，快过年了",
        "立春" => "春天开头，风还凉",
        "雨水" => "开始下雨，地气回暖",
        "惊蛰" => "春雷响了，虫子醒了",
        "春分" => "昼夜一样长，宜出门走走",
        "清明" => "天清气明，祭祖踏青",
        "谷雨" => "雨生百谷，该种地了",
        "立夏" => "夏天开头，白天长了",
        "小满" => "麦子灌浆，雨水多起来",
        "芒种" => "收麦种稻，最忙的时候",
        "夏至" => "白天最长，天热起来",
        "小暑" => "热浪来了，少在太阳下",
        "大暑" => "一年最热，多喝水",
        "立秋" => "秋天开头，早晚凉了",
        "处暑" => "暑气退了，晚上好睡",
        "白露" => "露水白了，夜里添衣",
        "秋分" => "昼夜一样长，秋高气爽",
        "寒露" => "露水凉了，别再贪凉",
        "霜降" => "开始打霜，柿子红了",
        "立冬" => "冬天开头，收好秋衣",
        "小雪" => "天要下雪，围炉的季节",
        "大雪" => "雪大起来，路滑慢走",
        "冬至" => "白天最短，吃饺子的日子",
        _ => "",
    }
}

/// 某年第 i 个节气（0 小寒 … 23 冬至）是几月几日。1901 到 2100 之外是 None。
pub fn term_date(year: i32, i: usize) -> Option<NaiveDate> {
    let c = match year {
        1901..=2000 => C20,
        2001..=2100 => C21,
        _ => return None,
    };
    let y = (year % 100) as f64;
    let month = (i / 2 + 1) as u32;
    let leap_fix = if month <= 2 { ((y as i32 - 1) / 4) as f64 } else { (y / 4.0).floor() };
    let day = (y * 0.2422 + c[i]).floor() - leap_fix;
    NaiveDate::from_ymd_opt(year, month, day as u32)
}

/// 今天是哪个节气（是就给名字），不是就给下一个节气的名字和日期。
pub fn term_of(d: NaiveDate) -> Option<(String, NaiveDate)> {
    use chrono::Datelike;
    for y in [d.year(), d.year() + 1] {
        for (i, name) in TERMS.iter().enumerate() {
            let t = term_date(y, i)?;
            if t >= d {
                return Some((name.to_string(), t));
            }
        }
    }
    None
}

/// 现在在哪个节气里：今天或之前最近的那个（9 月 16 日在白露里）。给名字、在二十四个里的序号、那天的日期。
pub fn term_now(d: NaiveDate) -> Option<(String, usize, NaiveDate)> {
    use chrono::Datelike;
    for y in [d.year(), d.year() - 1] {
        for i in (0..24).rev() {
            let t = term_date(y, i)?;
            if t <= d {
                return Some((TERMS[i].to_string(), i, t));
            }
        }
    }
    None
}

/// 节气在哪个季节：立春到谷雨是春，立夏到大暑是夏，立秋到霜降是秋，其余是冬。
pub fn season_of(term_index: usize) -> &'static str {
    match term_index {
        2..=7 => "春",
        8..=13 => "夏",
        14..=19 => "秋",
        _ => "冬",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_now_is_the_one_we_are_in() {
        let (n, i, t) = term_now(NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()).unwrap();
        assert_eq!((n.as_str(), season_of(i)), ("白露", "秋"));
        assert!(t <= NaiveDate::from_ymd_opt(2026, 9, 16).unwrap());
        assert_eq!(term_now(NaiveDate::from_ymd_opt(2026, 1, 2).unwrap()).unwrap().0, "冬至");
    }

    fn l(y: i32, m: u32, d: u32) -> Lunar {
        from_date(NaiveDate::from_ymd_opt(y, m, d).unwrap()).unwrap()
    }

    #[test]
    fn spring_festivals() {
        for (y, m, d, name) in [(1900, 1, 31, "庚子鼠年"), (2000, 2, 5, "庚辰龙年"), (2024, 2, 10, "甲辰龙年"), (2025, 1, 29, "乙巳蛇年"), (2026, 2, 17, "丙午马年")] {
            let x = l(y, m, d);
            assert_eq!((x.month, x.leap, x.day), (1, false, 1), "{y}-{m}-{d}");
            assert_eq!(x.year_name(), name);
            assert_eq!(x.month_day(), "正月初一");
        }
    }

    #[test]
    fn leap_month_and_known_days() {
        assert_eq!(l(2023, 3, 22).month_day(), "闰二月初一");
        assert_eq!(l(2026, 9, 16).month_day(), "八月初六");
    }

    #[test]
    fn solar_terms() {
        let d = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(term_date(2026, 18), Some(d(2026, 10, 8))); // 寒露
        assert_eq!(term_date(2026, 23), Some(d(2026, 12, 22))); // 冬至
        assert_eq!(term_date(2024, 2), Some(d(2024, 2, 4))); // 立春
        assert_eq!(term_date(1999, 5), Some(d(1999, 3, 21))); // 春分
        assert_eq!(term_of(d(2026, 10, 8)), Some(("寒露".into(), d(2026, 10, 8))));
        assert_eq!(term_of(d(2026, 10, 11)), Some(("霜降".into(), d(2026, 10, 23))));
        assert_eq!(term_of(d(2026, 12, 23)).map(|t| t.0), Some("小寒".into()));
        for name in TERMS {
            assert!(!term_line(name).is_empty(), "{name}");
        }
        assert_eq!(l(2025, 10, 6).month_day(), "八月十五");
        assert_eq!(l(2024, 12, 31).month_day(), "腊月初一");
    }

    #[test]
    fn outside_table_is_none() {
        assert!(from_date(NaiveDate::from_ymd_opt(1899, 12, 31).unwrap()).is_none());
        assert!(from_date(NaiveDate::from_ymd_opt(2051, 6, 1).unwrap()).is_none());
    }
}
