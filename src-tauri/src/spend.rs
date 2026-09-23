// Session cost and context-window percentage from ACP `usage_update` ticks.

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
}
