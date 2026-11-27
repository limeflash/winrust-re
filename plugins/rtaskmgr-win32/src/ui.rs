//! The units the pages print: memory, thousands, an up time.

pub fn fmt_mem(bytes: u64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1000.0 { tf!("{} МБ", "{} MB", ja: "{} MB", pt: "{} MB", zh: "{} MB", group_thousands(mb as u64)) } else if mb >= 100.0 { tf!("{:.0} МБ", "{:.0} MB", ja: "{:.0} MB", pt: "{:.0} MB", zh: "{:.0} MB", mb) } else { tf!("{:.1} МБ", "{:.1} MB", ja: "{:.1} MB", pt: "{:.1} MB", zh: "{:.1} MB", mb) }
}

pub fn group_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}




pub fn fmt_uptime(secs: u64) -> String {
    format!("{}:{:02}:{:02}:{:02}", secs / 86400, secs / 3600 % 24, secs / 60 % 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(group_thousands(3572), "3,572");
        assert_eq!(group_thousands(12), "12");
        assert_eq!(fmt_mem(3_746_000_000), "3,572 МБ");
        assert_eq!(fmt_uptime(2 * 86400 + 18 * 3600 + 27 * 60 + 54), "2:18:27:54");
    }


}
