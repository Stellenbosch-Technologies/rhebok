use std::time::{SystemTime, UNIX_EPOCH};

pub fn ago(unix: Option<u64>) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    ago_at(unix, now)
}

pub fn ago_at(unix: Option<u64>, now: u64) -> String {
    let Some(unix) = unix.filter(|u| *u != 0) else {
        return "never".to_string();
    };
    let s = now.saturating_sub(unix);
    if s < 60 {
        format!("{s}s ago")
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else if s < 86400 {
        format!("{}h ago", s / 3600)
    } else {
        format!("{}d ago", s / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::ago_at;

    #[test]
    fn ago_reads_like_the_web_dashboard() {
        assert_eq!(ago_at(None, 1000), "never");
        assert_eq!(ago_at(Some(0), 1000), "never");
        assert_eq!(ago_at(Some(2000), 1000), "0s ago");
        assert_eq!(ago_at(Some(1000 - 59), 1000), "59s ago");
        assert_eq!(ago_at(Some(1000 - 60), 1000), "1m ago");
        assert_eq!(ago_at(Some(10_000 - 3599), 10_000), "59m ago");
        assert_eq!(ago_at(Some(10_000 - 3600), 10_000), "1h ago");
        assert_eq!(ago_at(Some(100_000 - 86_399), 100_000), "23h ago");
        assert_eq!(ago_at(Some(1_000_000 - 3 * 86_400), 1_000_000), "3d ago");
    }
}
