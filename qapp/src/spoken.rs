//! 念出来的话里的数字：卡上的 `spoken` 是给喇叭的，不能有阿拉伯数字和符号。app 自己的事，框架给工具。

/// 0 → 零 … 23450 → 二万三千四百五十。
pub fn cn_num(n: u64) -> String {
    const D: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    match n {
        0..=9 => D[n as usize].to_string(),
        10 => "十".into(),
        11..=19 => format!("十{}", D[(n - 10) as usize]),
        20..=99 => format!(
            "{}十{}",
            D[(n / 10) as usize],
            if n.is_multiple_of(10) {
                ""
            } else {
                D[(n % 10) as usize]
            }
        ),
        100..=999 => cn_big(n, 100, "百"),
        1000..=9999 => cn_big(n, 1000, "千"),
        10000..=99_999_999 => cn_big(n, 10000, "万"),
        _ => n.to_string(),
    }
}

/// 一千四百二十、二万三千四百五十、三百零五。
fn cn_big(n: u64, unit: u64, name: &str) -> String {
    let head = cn_num(n / unit);
    let rest = n % unit;
    if rest == 0 {
        format!("{head}{name}")
    } else if rest < unit / 10 {
        format!("{head}{name}零{}", cn_num(rest))
    } else {
        format!("{head}{name}{}", cn_num(rest))
    }
}

/// 温度：23.4 → 二十三，-5 → 零下五。
pub fn temp_zh(t: f64) -> String {
    let n = t.round() as i64;
    if n < 0 {
        format!("零下{}", cn_num(n.unsigned_abs()))
    } else {
        cn_num(n as u64)
    }
}

/// “CA1302” → “C A 一三零二”。
pub fn flight_no(no: &str) -> String {
    no.chars()
        .map(|c| {
            if let Some(d) = c.to_digit(10) {
                cn_num(d as u64)
            } else {
                format!("{c} ")
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// “08:05” → “八点零五分”。
pub fn clock(t: &str) -> String {
    let Some((h, m)) = t.split_once(':') else {
        return t.to_string();
    };
    let h: u64 = h.parse().unwrap_or(0);
    let m: u64 = m.parse().unwrap_or(0);
    if m == 0 {
        format!("{}点", cn_num(h))
    } else if m < 10 {
        format!("{}点零{}分", cn_num(h), cn_num(m))
    } else {
        format!("{}点{}分", cn_num(h), cn_num(m))
    }
}

/// 一段时间：1800 → 三十分钟，90 → 一分三十秒，7200 → 两小时。
pub fn duration(secs: u64) -> String {
    let two = |n: u64| if n == 2 { "两".to_string() } else { cn_num(n) };
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    let mut out = String::new();
    if h > 0 {
        out.push_str(&format!("{}小时", two(h)));
    }
    if m > 0 {
        out.push_str(&format!("{}分", cn_num(m)));
        if s == 0 && h == 0 {
            out.push('钟');
        }
    }
    if s > 0 || out.is_empty() {
        out.push_str(&format!("{}秒", cn_num(s)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_in_words() {
        assert_eq!(clock("08:05"), "八点零五分");
        assert_eq!(clock("13:20"), "十三点二十分");
        assert_eq!(flight_no("CA1302"), "C A 一三零二");
        assert_eq!(cn_num(1420), "一千四百二十");
        assert_eq!(cn_num(23450), "二万三千四百五十");
        assert_eq!(cn_num(305), "三百零五");
        assert_eq!(cn_num(1000), "一千");
        assert_eq!(temp_zh(23.4), "二十三");
        assert_eq!(temp_zh(-5.0), "零下五");
        assert_eq!(temp_zh(0.2), "零");
        assert_eq!(duration(1800), "三十分钟");
        assert_eq!(duration(90), "一分三十秒");
        assert_eq!(duration(7200), "两小时");
        assert_eq!(duration(5), "五秒");
    }
}
