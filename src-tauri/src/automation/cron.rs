// cron 求值（design §4）：纯函数，零新依赖。
//
// 支持 5 字段（分 时 日 月 周），在 IANA 时区按「固定 UTC 偏移」求值。
//
// > 已知语义差：本模块对 DST 时区按**标准时偏移**求值，不做夏令时切换。
// > 主要原因用户是 Asia/Shanghai（无 DST），表外时区一律按 UTC 求值。
// > 与 codeg（chrono-tz 精确）的差别仅此一项，勿当 bug 修。
//
// 周字段采用 UI 语义：0 或 7 = 周日，1 = 周一 … 6 = 周六（与 JS Date.getDay 一致）。
// 内部统一成 0=周日..6=周六（codeg 踩过这里差一天的坑）。

/// 搜索上限：约 4 年。超出返回 None（`0 0 30 2 *` 这类永不命中的表达式）。
pub const MAX_SEARCH_DAYS: i64 = 366 * 4;

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;

/// 时区表：`(IANA 名, UTC 偏移分钟, 是否有夏令时)`
///
/// 表外时区按 UTC（偏移 0）求值；`has_dst` 只用于 UI 提示，不参与计算。
pub const TZ_TABLE: &[(&str, i32, bool)] = &[
    ("UTC", 0, false),
    ("Etc/UTC", 0, false),
    ("Asia/Shanghai", 480, false),
    ("Asia/Chongqing", 480, false),
    ("Asia/Hong_Kong", 480, false),
    ("Asia/Taipei", 480, false),
    ("Asia/Singapore", 480, false),
    ("Asia/Tokyo", 540, false),
    ("Asia/Seoul", 540, false),
    ("Asia/Kolkata", 330, false),
    ("Asia/Dubai", 240, false),
    ("Europe/London", 0, true),
    ("Europe/Berlin", 60, true),
    ("Europe/Paris", 60, true),
    ("Europe/Moscow", 180, false),
    ("America/New_York", -300, true),
    ("America/Chicago", -360, true),
    ("America/Los_Angeles", -480, true),
    ("America/Sao_Paulo", -180, false),
    ("Australia/Sydney", 600, true),
];

/// 时区是否在支持表内
pub fn tz_supported(tz: &str) -> bool {
    TZ_TABLE.iter().any(|(name, _, _)| *name == tz)
}

/// 时区是否有夏令时（仅用于 UI 提示）
pub fn tz_has_dst(tz: &str) -> bool {
    TZ_TABLE
        .iter()
        .find(|(name, _, _)| *name == tz)
        .map(|(_, _, dst)| *dst)
        .unwrap_or(false)
}

/// 时区 → 偏移分钟。表外 / 空串按 UTC。
pub fn tz_offset_minutes(tz: &str) -> i32 {
    TZ_TABLE
        .iter()
        .find(|(name, _, _)| *name == tz.trim())
        .map(|(_, off, _)| *off)
        .unwrap_or(0)
}

// ---------- 日期换算（纯整数，无浮点） ----------

/// civil 日期（年 / 月 / 日）→ 距 1970-01-01 的天数（Howard Hinnant 算法）
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = ((m as i64) + 9) % 12; // 3..=14 → 0..=11（3 月为 0）
    let doy = (153 * mp + 2) / 5 + (d as i64) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// 距 1970-01-01 的天数 → civil 日期
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 1970-01-01 是周四（内部 0=周日）
fn weekday_from_days(days: i64) -> u32 {
    // 1970-01-01 = 4 (周四)
    (((days % 7) + 7 + 4) % 7) as u32
}

// ---------- 字段解析 ----------

/// 一个 cron 字段的值集合（0..=max）
#[derive(Debug, Clone)]
pub(crate) struct FieldSet {
    values: Vec<bool>,
    wildcard: bool,
}

impl FieldSet {
    fn matches(&self, v: u32) -> bool {
        self.values.get(v as usize).copied().unwrap_or(false)
    }
}

/// 解析单个字段：支持 `*`、`a`、`a-b`、`a-b/step`、`*/step`、`a/step`、逗号列表
fn parse_field(raw: &str, min: u32, max: u32, name: &str) -> Result<FieldSet, String> {
    let field = raw.trim();
    if field.is_empty() {
        return Err(format!("cron {}字段为空", name));
    }
    let mut values = vec![false; (max + 1) as usize];
    let wildcard = field == "*";
    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(format!("cron {}字段含空项", name));
        }
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s
                    .trim()
                    .parse()
                    .map_err(|_| format!("cron {}字段步长非法: {}", name, part))?;
                if step == 0 {
                    return Err(format!("cron {}字段步长不能为 0", name));
                }
                (r.trim(), step)
            }
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            let a: u32 = parse_num(a, min, max, name)?;
            let b: u32 = parse_num(b, min, max, name)?;
            if a > b {
                return Err(format!("cron {}字段区间倒序: {}", name, part));
            }
            (a, b)
        } else {
            let a = parse_num(range, min, max, name)?;
            // `a/step` = 从 a 到 max
            if part.contains('/') {
                (a, max)
            } else {
                (a, a)
            }
        };
        let mut v = lo;
        while v <= hi {
            values[v as usize] = true;
            v += step;
        }
    }
    Ok(FieldSet { values, wildcard })
}

fn parse_num(raw: &str, min: u32, max: u32, name: &str) -> Result<u32, String> {
    let n: u32 = raw
        .trim()
        .parse()
        .map_err(|_| format!("cron {}字段取值非法: {}", name, raw))?;
    if n < min || n > max {
        return Err(format!("cron {}字段取值超出 {}-{}: {}", name, min, max, n));
    }
    Ok(n)
}

/// 周字段重映射：UI 语义 0/7=周日 → 内部 0=周日..6=周六
///
/// 逐项重写（`7` → `0`），区间与步长按项整体处理；越界值报错。
pub fn remap_dow_field(field: &str) -> Result<String, String> {
    let field = field.trim();
    if field.is_empty() {
        return Err("cron 周字段为空".to_string());
    }
    if field == "*" {
        return Ok("*".to_string());
    }
    let mut out: Vec<String> = Vec::new();
    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("cron 周字段含空项".to_string());
        }
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s
                    .trim()
                    .parse()
                    .map_err(|_| format!("cron 周字段步长非法: {}", part))?;
                if step == 0 {
                    return Err("cron 周字段步长不能为 0".to_string());
                }
                (r.trim(), Some(step))
            }
            None => (part, None),
        };
        let remap = |raw: &str| -> Result<u32, String> {
            let n: u32 = raw
                .trim()
                .parse()
                .map_err(|_| format!("cron 周字段取值非法: {}", raw))?;
            if n > 7 {
                return Err(format!("周字段取值超出 0-7: {}", n));
            }
            Ok(if n == 7 { 0 } else { n })
        };
        let body = if range == "*" {
            "*".to_string()
        } else if let Some((a, b)) = range.split_once('-') {
            let a = remap(a)?;
            let b = remap(b)?;
            format!("{}-{}", a, b)
        } else {
            format!("{}", remap(range)?)
        };
        out.push(match step {
            Some(s) => format!("{}/{}", body, s),
            None => body,
        });
    }
    Ok(out.join(","))
}

/// 规范化后的 cron（5 字段，周字段已重映射；用于解析，不落盘）
#[derive(Debug, Clone)]
pub struct CronSchedule {
    pub minute: FieldSet,
    pub hour: FieldSet,
    pub dom: FieldSet,
    pub month: FieldSet,
    pub dow: FieldSet,
}

impl CronSchedule {
    /// 检查某个本地墙上时间是否命中
    fn matches(&self, _year: i64, mo: u32, d: u32, h: u32, mi: u32, wd: u32) -> bool {
        if !self.minute.matches(mi) || !self.hour.matches(h) || !self.month.matches(mo) {
            return false;
        }
        let dom_ok = self.dom.matches(d);
        let dow_ok = self.dow.matches(wd);
        // Vixie cron 语义：dom 与 dow 都「非通配」时取 OR
        if self.dom.wildcard || self.dow.wildcard {
            dom_ok && dow_ok
        } else {
            dom_ok || dow_ok
        }
    }
}

/// 解析 5 字段 cron 表达式（周字段按 UI 语义重映射）
pub fn parse_cron(expr: &str) -> Result<CronSchedule, String> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err("cron 表达式为空".to_string());
    }
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    let (minute, hour, dom, month, dow) = match fields.len() {
        5 => (fields[0], fields[1], fields[2], fields[3], fields[4]),
        n => {
            return Err(format!(
                "cron 需要 5 个字段（分 时 日 月 周），实际 {} 个",
                n
            ))
        }
    };
    Ok(CronSchedule {
        minute: parse_field(minute, 0, 59, "分")?,
        hour: parse_field(hour, 0, 23, "时")?,
        dom: parse_field(dom, 1, 31, "日")?,
        month: parse_field(month, 1, 12, "月")?,
        dow: parse_field(&remap_dow_field(dow)?, 0, 6, "周")?,
    })
}

/// 校验 cron + 时区是否可用（不做求值）
pub fn validate_cron(cron: &str, timezone: &str) -> Result<(), String> {
    parse_cron(cron)?;
    if timezone.trim().is_empty() {
        return Err("时区不能为空".to_string());
    }
    Ok(())
}

/// 下次触发时刻（UTC 毫秒），严格晚于 `after_ms`。
///
/// 在 `timezone` 的本地墙上时间上逐分钟搜索（cron 最小粒度），
/// 超过 `MAX_SEARCH_DAYS` 仍无解 → `None`。
pub fn compute_next_run(
    cron: &str,
    timezone: &str,
    after_ms: i64,
) -> Result<Option<i64>, String> {
    let schedule = parse_cron(cron)?;
    let offset_ms = (tz_offset_minutes(timezone) as i64) * MINUTE_MS;

    // 本地墙上时间从下一分钟开始
    let local_ms = after_ms + offset_ms;
    let mut cursor = (local_ms / MINUTE_MS) * MINUTE_MS + MINUTE_MS;
    let limit = cursor + MAX_SEARCH_DAYS * DAY_MS;

    while cursor <= limit {
        let local_days = cursor.div_euclid(DAY_MS);
        let (year, mo, d) = civil_from_days(local_days);
        let wd = weekday_from_days(local_days);
        let minute_of_day = ((cursor.rem_euclid(DAY_MS)) / MINUTE_MS) as u32;
        let h = minute_of_day / 60;
        let mi = minute_of_day % 60;

        // 月不匹配时直接跳到下个月 1 号 00:00，避免逐分钟空转
        if !schedule.month.matches(mo) {
            let (ny, nm) = if mo == 12 { (year + 1, 1) } else { (year, mo + 1) };
            cursor = (days_from_civil(ny, nm, 1)) * DAY_MS;
            continue;
        }
        // 日不匹配（且周日也不匹配时）跳到下一天 00:00
        if !schedule.matches(year, mo, d, h, mi, wd) {
            if !schedule.dom.matches(d) && !schedule.dow.matches(wd) {
                cursor = (local_days + 1) * DAY_MS;
            } else {
                cursor += MINUTE_MS;
            }
            continue;
        }
        return Ok(Some(cursor - offset_ms));
    }
    Ok(None)
}

/// 「下次运行」预览与调度器共用的入口（保证两者不可能分叉）
pub fn next_run_for(cron: &str, timezone: &str, now_ms: i64) -> Result<Option<i64>, String> {
    compute_next_run(cron, timezone, now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i64, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        days_from_civil(y, mo, d) * DAY_MS + (h as i64) * 3_600_000 + (mi as i64) * 60_000
    }

    fn parts(ms: i64) -> (i64, u32, u32, u32, u32, u32) {
        let days = ms.div_euclid(DAY_MS);
        let (y, mo, d) = civil_from_days(days);
        let mod_mins = (ms.rem_euclid(DAY_MS) / MINUTE_MS) as u32;
        (y, mo, d, mod_mins / 60, mod_mins % 60, weekday_from_days(days))
    }

    #[test]
    fn civil_roundtrip() {
        for (y, m, d) in [(1970, 1, 1), (2026, 6, 21), (2000, 2, 29), (1969, 12, 31)] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d));
        }
        // 1970-01-01 是周四（内部 4）
        assert_eq!(weekday_from_days(0), 4);
    }

    #[test]
    fn dow_mapping_matches_ui_semantics() {
        assert_eq!(remap_dow_field("1").unwrap(), "1"); // 周一
        assert_eq!(remap_dow_field("7").unwrap(), "0"); // 周日
        assert_eq!(remap_dow_field("0").unwrap(), "0"); // 周日
        assert_eq!(remap_dow_field("*").unwrap(), "*");
        assert_eq!(remap_dow_field("1-5").unwrap(), "1-5"); // 工作日
        assert!(remap_dow_field("8").is_err());
        assert!(remap_dow_field("Mon").is_err());
    }

    #[test]
    fn five_fields_accepted_six_rejected() {
        assert!(parse_cron("0 9 * * *").is_ok());
        assert!(parse_cron("*/5 * * * *").is_ok());
        assert!(parse_cron("0 9 * * 1-5").is_ok());
        assert!(parse_cron("0 0 9 * * *").is_err(), "6 字段必须拒绝");
        assert!(parse_cron("0 9 * *").is_err());
        assert!(parse_cron("60 9 * * *").is_err());
        assert!(parse_cron("0 24 * * *").is_err());
    }

    #[test]
    fn weekly_cron_lands_on_intended_weekday() {
        // 2026-06-21 是周日，窗口覆盖整周
        let after = utc(2026, 6, 21, 0, 0);
        let cases = [
            ("0 9 * * 0", 0u32), // 周日
            ("0 9 * * 1", 1),    // 周一
            ("0 9 * * 2", 2),
            ("0 9 * * 3", 3),
            ("0 9 * * 4", 4),
            ("0 9 * * 5", 5),
            ("0 9 * * 6", 6),
            ("0 9 * * 7", 0), // 7 = 周日的别名
        ];
        for (cron, want_wd) in cases {
            let next = compute_next_run(cron, "UTC", after).unwrap().unwrap();
            let (_, _, _, h, mi, wd) = parts(next);
            assert_eq!((h, mi), (9, 0), "cron `{}` 时刻不对", cron);
            assert_eq!(wd, want_wd, "cron `{}` 落在错误的星期", cron);
        }
    }

    #[test]
    fn weekdays_preset_excludes_weekend() {
        let after = utc(2026, 6, 21, 0, 0); // 周日
        let mut cursor = after;
        for _ in 0..10 {
            let next = compute_next_run("0 9 * * 1-5", "UTC", cursor).unwrap().unwrap();
            let (_, _, _, _, _, wd) = parts(next);
            assert!(wd >= 1 && wd <= 5, "工作日预设不应命中周末 (wd={})", wd);
            cursor = next;
        }
    }

    #[test]
    fn every_minute_advances_by_one_minute() {
        let after = utc(2026, 6, 21, 10, 30) + 15_000;
        let next = compute_next_run("* * * * *", "UTC", after).unwrap().unwrap();
        assert_eq!(next, after - 15_000 + 60_000, "应落在下一分钟的整点");
    }

    #[test]
    fn daily_cron_uses_timezone_offset() {
        // Asia/Shanghai 09:00 = UTC 01:00
        let after = utc(2026, 6, 21, 0, 0);
        let next = compute_next_run("0 9 * * *", "Asia/Shanghai", after)
            .unwrap()
            .unwrap();
        let (_, _, _, h, mi, _) = parts(next);
        assert_eq!((h, mi), (1, 0), "上海 9 点应等于 UTC 1 点");
    }

    #[test]
    fn impossible_date_returns_none() {
        // 2 月 30 日永不存在
        assert!(compute_next_run("0 0 30 2 *", "UTC", utc(2026, 1, 1, 0, 0))
            .unwrap()
            .is_none());
    }

    #[test]
    fn dom_and_dow_both_restricted_takes_or() {
        // 6 月 1 日是周一：dom=1 或 dow=5 都应命中
        let after = utc(2026, 5, 31, 23, 0);
        let next = compute_next_run("0 0 1 * 5", "UTC", after).unwrap().unwrap();
        let (_, mo, d, _, _, _) = parts(next);
        assert!(mo == 6 && d == 1, "dom=1 与 dow=5 应取并集, got {}-{}", mo, d);
    }

    #[test]
    fn unknown_timezone_falls_back_to_utc() {
        assert_eq!(tz_offset_minutes("Mars/Olympus"), 0);
        assert!(!tz_supported("Mars/Olympus"));
        assert!(tz_supported("Asia/Shanghai"));
        assert!(!tz_has_dst("Asia/Shanghai"));
        assert!(tz_has_dst("America/New_York"));
    }
}
