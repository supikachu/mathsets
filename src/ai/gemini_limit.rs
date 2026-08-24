//! Gemini 免费档限流（官方维度：RPM / 输入 TPM / RPD）。
//!
//! 文档：<https://ai.google.dev/gemini-api/docs/rate-limits>
//! 具体数字以 AI Studio → Rate Limit（按项目、免费层级）为准。
//!
//! 内置配额对齐 2026-08 Free 控制台：
//! | 模型 | RPM | TPM | RPD |
//! | 3.7 / 3.6 / 3 / 2.5 Flash | 5 | 250,000 | 20 |
//! | 2.5 Flash-Lite | 10 | 250,000 | 20 |
//! | Flash TTS | 3 | 10,000 | 10 |
//! | Pro 等 | 免费档不可用 |
//!
//! 覆盖：`GEMINI_FREE_RPM` / `GEMINI_FREE_TPM` / `GEMINI_FREE_RPD`

use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{Datelike, Duration as ChronoDuration, FixedOffset, TimeZone, Utc};

use super::provider::AiError;

pub const GEMINI_RPD_USER_MESSAGE: &str =
    "Gemini 免费额度今日请求次数已用尽（RPD），将于太平洋时间午夜重置";

pub const GEMINI_UNAVAILABLE_USER_MESSAGE: &str =
    "该 Gemini 模型在免费档不可用，请改用 Flash（如 gemini-3.7-flash）或开通付费";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeminiFreeQuota {
    pub rpm: u32,
    pub tpm: u32,
    pub rpd: u32,
}

struct Hit {
    at: Instant,
    tokens: u32,
}

struct Window {
    hits: VecDeque<Hit>,
    rpd_day: i32,
    rpd_count: u32,
}

static WINDOW: LazyLock<Mutex<Window>> = LazyLock::new(|| {
    Mutex::new(Window {
        hits: VecDeque::new(),
        rpd_day: pacific_day_id(),
        rpd_count: 0,
    })
});

pub fn is_gemini_base(base_url: &str) -> bool {
    let u = base_url.to_ascii_lowercase();
    u.contains("generativelanguage.googleapis.com") || u.contains("ai.google.dev")
}

/// 官方免费档配额；环境变量可覆盖（与 AI Studio 控制台对齐）。
pub fn free_quota_for_model(model: &str) -> GeminiFreeQuota {
    let mut q = builtin_free_quota(model);
    if let Some(v) = env_u32("GEMINI_FREE_RPM") {
        q.rpm = v.max(1);
    }
    if let Some(v) = env_u32("GEMINI_FREE_TPM") {
        q.tpm = v.max(1);
    }
    if let Some(v) = env_u32("GEMINI_FREE_RPD") {
        q.rpd = v.max(1);
    }
    q
}

fn builtin_free_quota(model: &str) -> GeminiFreeQuota {
    let m = model.to_ascii_lowercase().replace('_', "-");
    if m.contains("tts") {
        GeminiFreeQuota {
            rpm: 3,
            tpm: 10_000,
            rpd: 10,
        }
    } else if m.contains("flash-lite") || m.contains("flashlite") {
        GeminiFreeQuota {
            rpm: 10,
            tpm: 250_000,
            rpd: 20,
        }
    } else if m.contains("pro") && !m.contains("flash") {
        GeminiFreeQuota {
            rpm: 0,
            tpm: 0,
            rpd: 0,
        }
    } else {
        // 3.7 / 3.6 / 3 / 2.5 Flash 及默认
        GeminiFreeQuota {
            rpm: 5,
            tpm: 250_000,
            rpd: 20,
        }
    }
}

fn env_u32(key: &str) -> Option<u32> {
    std::env::var(key).ok()?.parse().ok()
}

/// 太平洋时间（固定 UTC−8，夏令时早 1 小时重置，更保守）
pub fn pacific_day_id() -> i32 {
    let tz = FixedOffset::west_opt(8 * 3600).expect("UTC-8");
    tz.from_utc_datetime(&Utc::now().naive_utc())
        .date_naive()
        .num_days_from_ce()
}

fn seconds_until_pacific_midnight() -> u64 {
    let tz = FixedOffset::west_opt(8 * 3600).expect("UTC-8");
    let now = tz.from_utc_datetime(&Utc::now().naive_utc());
    let tomorrow = now.date_naive() + ChronoDuration::days(1);
    let midnight = tomorrow.and_hms_opt(0, 0, 0).expect("midnight");
    let next = tz.from_local_datetime(&midnight).single().expect("tz");
    (next - now).num_seconds().max(1) as u64
}

/// 粗估输入 token：中文按 ~1 token/字；超长 base64 图按 258 token/张计，避免 TPM 被图片字节撑爆。
pub fn estimate_input_tokens(v: &serde_json::Value) -> u32 {
    fn walk(v: &serde_json::Value, acc: &mut u32) {
        match v {
            serde_json::Value::String(s) => {
                if s.starts_with("data:image") || (s.len() > 8_000 && s.starts_with("iVBOR")) {
                    *acc = acc.saturating_add(258);
                } else {
                    *acc = acc.saturating_add(s.chars().count() as u32);
                }
            }
            serde_json::Value::Array(a) => {
                for x in a {
                    walk(x, acc);
                }
            }
            serde_json::Value::Object(m) => {
                for x in m.values() {
                    walk(x, acc);
                }
            }
            _ => {}
        }
    }
    let mut n = 0u32;
    walk(v, &mut n);
    n.max(1)
}

fn try_reserve(now: Instant, tokens: u32, q: GeminiFreeQuota) -> Result<(), ReserveFail> {
    let mut w = WINDOW.lock().expect("gemini window");
    let day = pacific_day_id();
    if w.rpd_day != day {
        w.rpd_day = day;
        w.rpd_count = 0;
    }
    while let Some(front) = w.hits.front() {
        if now.saturating_duration_since(front.at) >= Duration::from_secs(60) {
            w.hits.pop_front();
        } else {
            break;
        }
    }
    if q.rpm == 0 || q.rpd == 0 || q.tpm == 0 {
        return Err(ReserveFail::Unavailable);
    }
    if w.rpd_count >= q.rpd {
        return Err(ReserveFail::Daily);
    }
    if w.hits.len() as u32 >= q.rpm {
        let wait = Duration::from_secs(60).saturating_sub(now.saturating_duration_since(w.hits[0].at));
        return Err(ReserveFail::Wait(wait.max(Duration::from_millis(200))));
    }
    let tpm: u32 = w.hits.iter().map(|h| h.tokens).sum();
    if tpm.saturating_add(tokens) > q.tpm {
        let wait = Duration::from_secs(60)
            .saturating_sub(now.saturating_duration_since(w.hits[0].at));
        return Err(ReserveFail::Wait(wait.max(Duration::from_millis(200))));
    }
    let min_gap = Duration::from_millis((60_000 / q.rpm.max(1) as u64).max(1));
    if let Some(last) = w.hits.back() {
        let elapsed = now.saturating_duration_since(last.at);
        if elapsed < min_gap {
            return Err(ReserveFail::Wait(min_gap - elapsed));
        }
    }
    w.hits.push_back(Hit { at: now, tokens });
    w.rpd_count = w.rpd_count.saturating_add(1);
    Ok(())
}

enum ReserveFail {
    Wait(Duration),
    Daily,
    Unavailable,
}

/// 在发起 Gemini 请求前占用免费档配额；超 RPD 直接失败，超 RPM/TPM 则等待窗口滑过。
pub async fn acquire(model: &str, input_tokens: u32) -> Result<(), AiError> {
    let q = free_quota_for_model(model);
    loop {
        match try_reserve(Instant::now(), input_tokens, q) {
            Ok(()) => {
                tracing::debug!(
                    model,
                    rpm = q.rpm,
                    tpm = q.tpm,
                    rpd = q.rpd,
                    tokens = input_tokens,
                    "Gemini 免费档已放行"
                );
                return Ok(());
            }
            Err(ReserveFail::Unavailable) => {
                tracing::warn!(model, "{GEMINI_UNAVAILABLE_USER_MESSAGE}");
                return Err(AiError::Upstream(403, GEMINI_UNAVAILABLE_USER_MESSAGE.into()));
            }
            Err(ReserveFail::Daily) => {
                tracing::warn!(
                    model,
                    rpd = q.rpd,
                    reset_in = seconds_until_pacific_midnight(),
                    "{GEMINI_RPD_USER_MESSAGE}"
                );
                return Err(AiError::Upstream(429, GEMINI_RPD_USER_MESSAGE.into()));
            }
            Err(ReserveFail::Wait(d)) => {
                let wait = d.min(Duration::from_secs(60));
                tracing::info!(
                    model,
                    wait_ms = wait.as_millis() as u64,
                    rpm = q.rpm,
                    "Gemini 免费档 RPM/TPM 窗口等待"
                );
                tokio::time::sleep(wait).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_lite_and_pro_quotas() {
        let lite = builtin_free_quota("gemini-2.5-flash-lite");
        assert_eq!(lite, GeminiFreeQuota { rpm: 10, tpm: 250_000, rpd: 20 });
        let flash37 = builtin_free_quota("gemini-3.7-flash");
        assert_eq!(flash37, GeminiFreeQuota { rpm: 5, tpm: 250_000, rpd: 20 });
        let flash25 = builtin_free_quota("gemini-2.5-flash");
        assert_eq!(flash25, flash37);
        let tts = builtin_free_quota("gemini-2.5-flash-tts");
        assert_eq!(tts, GeminiFreeQuota { rpm: 3, tpm: 10_000, rpd: 10 });
        let pro = builtin_free_quota("gemini-3.1-pro");
        assert_eq!(pro.rpd, 0);
        assert_eq!(pro.rpm, 0);
    }

    #[test]
    fn estimate_skips_data_image() {
        let v = serde_json::json!({
            "model": "gemini-2.5-flash",
            "messages": [{
                "content": [
                    {"type": "text", "text": "你好"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAA"}}
                ]
            }]
        });
        let n = estimate_input_tokens(&v);
        assert!(n >= 258 + 2, "{n}");
        assert!(n < 10_000, "{n}");
    }

    #[test]
    fn gemini_base_detect() {
        assert!(is_gemini_base(
            "https://generativelanguage.googleapis.com/v1beta/openai"
        ));
        assert!(!is_gemini_base("https://api.deepseek.com"));
    }
}
