use chrono::NaiveDate;
use regex::Regex;
use std::sync::OnceLock;

fn dmy_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // The `regex` crate has no backreferences, so we enumerate the three
    // allowed separators (`_`, `-`, `.`) explicitly to enforce that the same
    // separator is used on both sides.
    RE.get_or_init(|| {
        Regex::new(
            r"(\d{2})_(\d{2})_(\d{4})|(\d{2})-(\d{2})-(\d{4})|(\d{2})\.(\d{2})\.(\d{4})",
        )
        .expect("valid DMY regex")
    })
}

/// Rewrite the first detected DMY date in `name` to its canonical `YYYYMMDD`
/// form for use as a sort key. The displayed name is never mutated.
///
/// Heuristic: when both segments could be a day or a month (both <= 12) we
/// interpret as DMY because S3 folder names in this tool's user base follow
/// that convention. When the first segment is clearly a day (> 12) it is
/// definitely DMY. When the second segment is > 12 while the first is <= 12,
/// the string looks like MDY — we deliberately skip rewriting in that case so
/// those names fall back to lexicographic ordering rather than being
/// misinterpreted as DMY.
pub fn date_aware_sort_key(name: &str) -> String {
    let re = dmy_regex();
    let Some(caps) = re.captures(name) else {
        return name.to_string();
    };

    // Exactly one of the three alternation groups matched; pick whichever did.
    let (d_str, m_str, y_str) = if caps.get(1).is_some() {
        (&caps[1], &caps[2], &caps[3])
    } else if caps.get(4).is_some() {
        (&caps[4], &caps[5], &caps[6])
    } else {
        (&caps[7], &caps[8], &caps[9])
    };

    let day: u32 = d_str.parse().unwrap_or(0);
    let month: u32 = m_str.parse().unwrap_or(0);
    let year: i32 = y_str.parse().unwrap_or(0);

    if !(1900..=2100).contains(&year) {
        return name.to_string();
    }

    if day <= 12 && month > 12 {
        return name.to_string();
    }

    if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
        return name.to_string();
    }

    if NaiveDate::from_ymd_opt(year, month, day).is_none() {
        return name.to_string();
    }

    let m = caps.get(0).unwrap();
    let canonical = format!("{:04}{:02}{:02}", year, month, day);
    let mut out = String::with_capacity(name.len());
    out.push_str(&name[..m.start()]);
    out.push_str(&canonical);
    out.push_str(&name[m.end()..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by_key(|a| date_aware_sort_key(a));
        v
    }

    #[test]
    fn pure_dmy_orders_chronologically() {
        let got = sorted(vec!["01_05_2026", "02_04_2026", "01_04_2026"]);
        assert_eq!(got, vec!["01_04_2026", "02_04_2026", "01_05_2026"]);
    }

    #[test]
    fn separators_underscore_dash_dot() {
        assert_eq!(date_aware_sort_key("01_04_2026"), "20260401");
        assert_eq!(date_aware_sort_key("01-04-2026"), "20260401");
        assert_eq!(date_aware_sort_key("01.04.2026"), "20260401");
    }

    #[test]
    fn mixed_separators_do_not_match() {
        assert_eq!(date_aware_sort_key("01-04_2026"), "01-04_2026");
        assert_eq!(date_aware_sort_key("01.04-2026"), "01.04-2026");
    }

    #[test]
    fn embedded_date_in_filename() {
        let got = sorted(vec!["release_02_04_2026.zip", "release_01_04_2026.zip"]);
        assert_eq!(
            got,
            vec!["release_01_04_2026.zip", "release_02_04_2026.zip"]
        );
        assert_eq!(
            date_aware_sort_key("release_01_04_2026.zip"),
            "release_20260401.zip"
        );
    }

    #[test]
    fn invalid_date_is_not_rewritten() {
        assert_eq!(date_aware_sort_key("31_02_2026"), "31_02_2026");
        assert_eq!(date_aware_sort_key("31_04_2026"), "31_04_2026");
    }

    #[test]
    fn ambiguous_treated_as_dmy() {
        assert_eq!(date_aware_sort_key("04_05_2026"), "20260504");
    }

    #[test]
    fn mdy_like_is_skipped() {
        assert_eq!(date_aware_sort_key("05_13_2026"), "05_13_2026");
    }

    #[test]
    fn definitely_dmy_first_gt_twelve() {
        assert_eq!(date_aware_sort_key("13_04_2026"), "20260413");
    }

    #[test]
    fn non_date_names_sort_lexicographically() {
        let got = sorted(vec!["zeta", "alpha", "mike"]);
        assert_eq!(got, vec!["alpha", "mike", "zeta"]);
    }

    #[test]
    fn mixed_dated_and_non_dated() {
        let got = sorted(vec![
            "report.txt",
            "01_05_2026",
            "alpha",
            "02_04_2026",
            "release_01_04_2026.zip",
        ]);
        assert_eq!(
            got,
            vec![
                "02_04_2026",
                "01_05_2026",
                "alpha",
                "release_01_04_2026.zip",
                "report.txt",
            ]
        );
    }

    #[test]
    fn year_out_of_range_not_rewritten() {
        assert_eq!(date_aware_sort_key("01_04_1899"), "01_04_1899");
        assert_eq!(date_aware_sort_key("01_04_2101"), "01_04_2101");
    }
}
