//! The words and numbers of the window as the design writes them: dates ("Today, 07:59", "13 Nov, 00:05", "1 Apr 2024"), the type of a
//! file, sizes of drives, the counts of the status line. All pure: the clock is given, so a render and a test get the same text
//! whatever day it is.
use winrust_common::{Lang, lang, t, tf};

use super::backend::Clock;

/// FILETIME ticks (100 ns) in a day.
pub const DAY: i64 = 864_000_000_000;

/// (year, month 1-12, day 1-31, hour, minute) of a FILETIME in the clock's local time.
#[must_use]
pub fn civil(ft: u64, off: i64) -> (i32, u32, u32, u32, u32) {
    let local = ft as i64 + off;
    let (days, rest) = (local.div_euclid(DAY), local.rem_euclid(DAY));
    let mins = rest / 600_000_000;
    // days since 1601-01-01 -> since 1970-01-01, then the civil date (Howard Hinnant's algorithm)
    let z = days - 134_774 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((y + i64::from(m <= 2)) as i32, m, d, (mins / 60) as u32, (mins % 60) as u32)
}

/// The FILETIME of a local date and time (what the demo's made-up files carry).
#[must_use]
pub fn ticks(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> u64 {
    let y2 = i64::from(y) - i64::from(m <= 2);
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468 + 134_774;
    (days * DAY + (i64::from(hh) * 60 + i64::from(mm)) * 600_000_000) as u64
}

fn month(m: u32) -> &'static str {
    let i = (m as usize).wrapping_sub(1).min(11);
    match lang!() {
        Lang::Ru => ["янв", "фев", "мар", "апр", "мая", "июн", "июл", "авг", "сен", "окт", "ноя", "дек"][i],
        Lang::Pt => ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"][i],
        _ => ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][i],
    }
}

fn day_of(ft: u64, c: Clock) -> i64 {
    (ft as i64 + c.off).div_euclid(DAY)
}

/// `fmtDate` of the mockup: Today / Yesterday with the time, "27 Jan, 05:18" this year, "1 Apr 2024" before.
#[must_use]
pub fn date(ft: u64, c: Clock) -> String {
    if ft == 0 {
        return String::new();
    }
    let (y, m, d, hh, mm) = civil(ft, c.off);
    let ago = day_of(c.now as u64, c) - day_of(ft, c);
    let (ny, ..) = civil(c.now as u64, c.off);
    match ago {
        0 => tf!("Сегодня, {:02}:{:02}", "Today, {:02}:{:02}", ja: "今日 {:02}:{:02}", pt: "Hoje, {:02}:{:02}", zh: "今天 {:02}:{:02}", hh, mm),
        1 => tf!("Вчера, {:02}:{:02}", "Yesterday, {:02}:{:02}", ja: "昨日 {:02}:{:02}", pt: "Ontem, {:02}:{:02}", zh: "昨天 {:02}:{:02}", hh, mm),
        _ if y == ny => match lang!() {
            Lang::Ja | Lang::Zh => format!("{m}月{d}日 {hh:02}:{mm:02}"),
            _ => format!("{d} {}, {hh:02}:{mm:02}", month(m)),
        },
        _ => match lang!() {
            Lang::Ja | Lang::Zh => format!("{y}年{m}月{d}日"),
            _ => format!("{d} {} {y}", month(m)),
        },
    }
}

/// `longDate`: "26 Nov 2026, 08:12".
#[must_use]
pub fn long_date(ft: u64, c: Clock) -> String {
    if ft == 0 {
        return String::new();
    }
    let (y, m, d, hh, mm) = civil(ft, c.off);
    match lang!() {
        Lang::Ja | Lang::Zh => format!("{y}年{m}月{d}日 {hh:02}:{mm:02}"),
        _ => format!("{d} {} {y}, {hh:02}:{mm:02}", month(m)),
    }
}

/// `fmtGB`: "349 GB", "1.1 TB".
#[must_use]
pub fn gb(bytes: u64) -> String {
    let g = bytes as f64 / (1u64 << 30) as f64;
    if g >= 1000.0 { format!("{:.1} TB", g / 1024.0) } else { format!("{} GB", g.round() as u64) }
}

/// "349 GB free".
#[must_use]
pub fn free_text(bytes: u64) -> String {
    tf!("свободно {}", "{} free", ja: "空き {}", pt: "{} livres", zh: "可用 {}", gb(bytes))
}

/// The type of a file as the Type column says it.
#[must_use]
pub fn kind(name: &str, dir: bool) -> String {
    if dir {
        return t!("Папка", "Folder", ja: "フォルダー", pt: "Pasta", zh: "文件夹").into();
    }
    let file = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let ext = match file.rsplit_once('.') {
        Some((stem, e)) if !stem.is_empty() && !e.is_empty() => e.to_ascii_lowercase(),
        _ => return t!("Файл", "File", ja: "ファイル", pt: "Arquivo", zh: "文件").into(),
    };
    let known = match ext.as_str() {
        "exe" => Some(t!("Приложение", "Application")),
        "lnk" => Some(t!("Ярлык", "Shortcut")),
        "dll" => Some(t!("Расширение приложения", "App extension")),
        "toml" => Some("TOML"),
        "log" | "txt" => Some(t!("Текстовый документ", "Text document")),
        "md" => Some("Markdown"),
        "json" => Some("JSON"),
        "jpg" | "jpeg" => Some(t!("Изображение JPEG", "JPEG image")),
        "png" => Some(t!("Изображение PNG", "PNG image")),
        "pdf" => Some(t!("Документ PDF", "PDF document")),
        "zip" => Some(t!("Архив ZIP", "ZIP archive")),
        "docx" => Some(t!("Документ Word", "Word document")),
        "xlsx" => Some(t!("Книга Excel", "Excel workbook")),
        "fig" => Some(t!("Файл Figma", "Figma file")),
        "html" => Some(t!("Документ HTML", "HTML document")),
        "js" => Some(t!("Файл JavaScript", "JavaScript file")),
        "tmp" => Some(t!("Временный файл", "Temporary file")),
        _ => None,
    };
    match known {
        Some(k) if matches!(ext.as_str(), "toml" | "json") => tf!("Файл {}", "{} file", ja: "{} ファイル", pt: "Arquivo {}", zh: "{} 文件", k),
        Some(k) => k.into(),
        None => tf!("Файл {}", "{} file", ja: "{} ファイル", pt: "Arquivo {}", zh: "{} 文件", ext.to_uppercase()),
    }
}

/// "3 folders" in the plural the language wants.
#[must_use]
pub fn folders(n: usize) -> String {
    match lang!() {
        Lang::Ru => {
            let w = match (n % 10, n % 100) {
                (1, x) if x != 11 => "папка",
                (2..=4, x) if !(12..=14).contains(&x) => "папки",
                _ => "папок",
            };
            format!("{n} {w}")
        }
        Lang::Ja => format!("{n} 個のフォルダー"),
        Lang::Zh => format!("{n} 个文件夹"),
        Lang::Pt => format!("{n} pasta{}", if n == 1 { "" } else { "s" }),
        Lang::En => format!("{n} folder{}", if n == 1 { "" } else { "s" }),
    }
}

#[must_use]
pub fn files(n: usize) -> String {
    match lang!() {
        Lang::Ru => {
            let w = match (n % 10, n % 100) {
                (1, x) if x != 11 => "файл",
                (2..=4, x) if !(12..=14).contains(&x) => "файла",
                _ => "файлов",
            };
            format!("{n} {w}")
        }
        Lang::Ja => format!("{n} 個のファイル"),
        Lang::Zh => format!("{n} 个文件"),
        Lang::Pt => format!("{n} arquivo{}", if n == 1 { "" } else { "s" }),
        Lang::En => format!("{n} file{}", if n == 1 { "" } else { "s" }),
    }
}

#[must_use]
pub fn drives(n: usize) -> String {
    match lang!() {
        Lang::Ru => {
            let w = match (n % 10, n % 100) {
                (1, x) if x != 11 => "диск",
                (2..=4, x) if !(12..=14).contains(&x) => "диска",
                _ => "дисков",
            };
            format!("{n} {w}")
        }
        Lang::Ja => format!("{n} 個のドライブ"),
        Lang::Zh => format!("{n} 个驱动器"),
        Lang::Pt => format!("{n} unidade{}", if n == 1 { "" } else { "s" }),
        Lang::En => format!("{n} drive{}", if n == 1 { "" } else { "s" }),
    }
}

#[must_use]
pub fn results(n: usize) -> String {
    match lang!() {
        Lang::Ru => {
            let w = match (n % 10, n % 100) {
                (1, x) if x != 11 => "результат",
                (2..=4, x) if !(12..=14).contains(&x) => "результата",
                _ => "результатов",
            };
            format!("{n} {w}")
        }
        Lang::Ja => format!("{n} 件"),
        Lang::Zh => format!("{n} 个结果"),
        Lang::Pt => format!("{n} resultado{}", if n == 1 { "" } else { "s" }),
        Lang::En => format!("{n} result{}", if n == 1 { "" } else { "s" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_and_ticks_are_each_others_inverse() {
        for (y, m, d, hh, mm) in [(2026, 11, 26, 17, 30), (2024, 2, 29, 0, 0), (2000, 1, 1, 23, 59), (1999, 12, 31, 12, 1), (2026, 3, 1, 5, 5)] {
            assert_eq!(civil(ticks(y, m, d, hh, mm), 0), (y, m, d, hh, mm));
        }
        // the local offset moves the clock face, not the day count's origin
        assert_eq!(civil(ticks(2026, 1, 1, 23, 0), 36_000_000_000), (2026, 1, 2, 0, 0));
    }

    #[test]
    fn dates_read_like_the_mockups_in_english() {
        let c = Clock { now: ticks(2026, 11, 26, 17, 30) as i64, off: 0 };
        let at = |y, m, d, hh, mm| ticks(y, m, d, hh, mm);
        // `lang!()` is Russian in a test build: the shape is checked, not the words
        assert!(date(at(2026, 11, 26, 7, 59), c).ends_with("07:59"));
        assert!(date(at(2026, 11, 25, 21, 36), c).ends_with("21:36"));
        assert_eq!(date(at(2026, 11, 13, 0, 5), c), format!("13 {}, 00:05", month(11)));
        assert_eq!(date(at(2024, 4, 1, 0, 26), c), format!("1 {} 2024", month(4)));
        assert_eq!(date(0, c), "");
        assert_eq!(long_date(at(2026, 11, 26, 8, 12), c), format!("26 {} 2026, 08:12", month(11)));
    }

    #[test]
    fn sizes_of_drives() {
        assert_eq!(gb(349 << 30), "349 GB");
        assert_eq!(gb(1142 << 30), "1.1 TB");
        assert_eq!(gb(61 << 30), "61 GB");
    }
}
