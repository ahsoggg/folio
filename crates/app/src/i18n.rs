//! Two languages, side by side at the point of use: `tr("Открыть", "Open")`.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ru,
    En,
}

impl Lang {
    pub fn from_code(code: &str) -> Lang {
        if code.eq_ignore_ascii_case("en") {
            Lang::En
        } else {
            Lang::Ru
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Lang::En
    } else {
        Lang::Ru
    }
}

pub fn tr(ru: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::Ru => ru,
        Lang::En => en,
    }
}

/// `trf("Страница {0} из {1}", "Page {0} of {1}", &[&a, &b])`.
pub fn trf(ru: &'static str, en: &'static str, args: &[&dyn std::fmt::Display]) -> String {
    let mut s = tr(ru, en).to_owned();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), &a.to_string());
    }
    s
}

/// Russian plural: 1 страница, 2 страницы, 5 страниц.
pub fn plural_ru(n: usize, one: &'static str, few: &'static str, many: &'static str) -> &'static str {
    let (m10, m100) = (n % 10, n % 100);
    if m10 == 1 && m100 != 11 {
        one
    } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
        few
    } else {
        many
    }
}

pub fn human_size(n: u64) -> String {
    let units = [tr("Б", "B"), tr("КБ", "KB"), tr("МБ", "MB"), tr("ГБ", "GB")];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{n} {}", units[0])
    } else {
        format!("{v:.1} {}", units[i])
    }
}

/// "12 мин" / "1 ч 5 мин" from a number of minutes.
pub fn duration_text(minutes: usize) -> String {
    if minutes < 1 {
        return tr("< 1 мин", "< 1 min").to_owned();
    }
    if minutes < 60 {
        return format!("{minutes} {}", tr("мин", "min"));
    }
    format!(
        "{} {} {} {}",
        minutes / 60,
        tr("ч", "h"),
        minutes % 60,
        tr("мин", "min")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_filled() {
        set_lang(Lang::En);
        assert_eq!(
            trf("Страница {0} из {1}", "Page {0} of {1}", &[&3, &10]),
            "Page 3 of 10"
        );
        set_lang(Lang::Ru);
        assert_eq!(
            trf("Страница {0} из {1}", "Page {0} of {1}", &[&3, &10]),
            "Страница 3 из 10"
        );
    }

    #[test]
    fn russian_plurals() {
        let f = |n| plural_ru(n, "страница", "страницы", "страниц");
        assert_eq!(
            (f(1), f(2), f(5), f(11), f(21), f(24), f(112)),
            (
                "страница",
                "страницы",
                "страниц",
                "страниц",
                "страница",
                "страницы",
                "страниц"
            )
        );
    }

    #[test]
    fn durations_and_sizes() {
        set_lang(Lang::Ru);
        assert_eq!(duration_text(0), "< 1 мин");
        assert_eq!(duration_text(75), "1 ч 15 мин");
        assert_eq!(human_size(1536), "1.5 КБ");
    }
}
