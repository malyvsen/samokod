// Session cost and context-window percentage from ACP `usage_update` ticks.

/// Cost to report for one `usage_update`: the incoming total when present,
/// otherwise the last known total. A missing cost means "no info", never
/// zero, so cost-less ticks keep the total while context still advances.
/// Pure.
pub fn kept_cost(kept: f64, incoming: Option<f64>) -> f64 {
    incoming.unwrap_or(kept)
}

/// Context percentage from a `usage_update`. A zero-size window yields zero.
pub fn context_pct(used: u64, size: u64) -> f64 {
    if size == 0 {
        return 0.0;
    }
    used as f64 / size as f64 * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_pct_divides_used_by_size() {
        assert!((context_pct(9151, 200_000) - 4.5755).abs() < 0.001);
        assert_eq!(context_pct(0, 200_000), 0.0);
        assert_eq!(context_pct(10, 0), 0.0);
    }

    #[test]
    fn missing_cost_keeps_last_total() {
        assert_eq!(kept_cost(1.24, None), 1.24);
        assert_eq!(kept_cost(1.24, Some(2.5)), 2.5);
    }
}
