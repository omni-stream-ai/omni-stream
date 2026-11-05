//! Task-local calendar rules; no country-specific business logic.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calendar {
    /// ISO weekdays, Monday=1 through Sunday=7; empty means every day.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    #[serde(default)]
    pub include_dates: Vec<DateOverride>,
    #[serde(default)]
    pub exclude_dates: Vec<DateOverride>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DateOverride {
    pub date: String,
    pub source: String,
    pub reason: String,
}

/// Gregorian civil date to days since 1970-01-01.
pub fn day(value: &str) -> Result<i64, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return Err("日期须为 YYYY-MM-DD".into());
    }
    let year: i64 = value[..4].parse().map_err(|_| "无效年份")?;
    let month: i64 = value[5..7].parse().map_err(|_| "无效月份")?;
    let date: i64 = value[8..].parse().map_err(|_| "无效日期")?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    if !(1970..=9999).contains(&year) || date < 1 || date > max {
        return Err("无效公历日期".into());
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * m + 2) / 5 + date - 1;
    Ok(era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468)
}
impl Calendar {
    pub fn validate(&self) -> Result<(), String> {
        if self.weekdays.iter().any(|d| !(1..=7).contains(d)) {
            return Err("星期必须为 1（周一）到 7（周日）".into());
        }
        if self.include_dates.len() + self.exclude_dates.len() > 4096 {
            return Err("日期覆盖最多 4096 项".into());
        }
        for entry in self.include_dates.iter().chain(&self.exclude_dates) {
            day(&entry.date)?;
            if entry.source.trim().is_empty() || entry.reason.trim().is_empty() {
                return Err("日期覆盖必须说明来源和原因".into());
            }
        }
        Ok(())
    }
    pub fn allows(&self, local_day: i64) -> bool {
        let has = |entries: &[DateOverride]| {
            entries
                .iter()
                .any(|entry| day(&entry.date).ok() == Some(local_day))
        };
        if has(&self.exclude_dates) {
            return false;
        }
        if has(&self.include_dates) {
            return true;
        }
        self.weekdays.is_empty()
            || self
                .weekdays
                .contains(&((local_day + 3).rem_euclid(7) as u8 + 1))
    }
    pub fn previous(&self, local_day: i64) -> Result<i64, String> {
        (1..=7 * 4097)
            .map(|offset| local_day - offset)
            .find(|date| self.allows(*date))
            .ok_or_else(|| "找不到上一个有效日期".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gregorian_validation_and_weekdays() {
        assert_eq!(day("1970-01-01").unwrap(), 0);
        assert!(day("2025-02-29").is_err());
        assert!(day("2024-02-29").is_ok());
        assert!(day("2026-13-01").is_err());
        let cal = Calendar {
            weekdays: vec![1, 2, 3, 4, 5],
            ..Default::default()
        };
        assert!(cal.allows(day("2026-11-06").unwrap()));
        assert!(!cal.allows(day("2026-11-07").unwrap()));
        assert_eq!(
            cal.previous(day("2026-11-09").unwrap()).unwrap(),
            day("2026-11-06").unwrap()
        );
    }
    #[test]
    fn exclude_wins_over_include_and_weekdays() {
        let entry = DateOverride {
            date: "2026-11-07".into(),
            source: "user".into(),
            reason: "override".into(),
        };
        let mut cal = Calendar {
            weekdays: vec![1],
            include_dates: vec![entry.clone()],
            ..Default::default()
        };
        assert!(cal.allows(day(&entry.date).unwrap()));
        cal.exclude_dates.push(entry.clone());
        assert!(!cal.allows(day(&entry.date).unwrap()));
        cal.validate().unwrap();
    }
}
