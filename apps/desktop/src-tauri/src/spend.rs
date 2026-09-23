// Session spend derived from ACP usage payloads: per-turn token usage plus
// live context-window and cost ticks.
use crate::acp::Usage;

/// Running token sums for one session. End-turn `usage` covers the latest
/// turn only (verified live: 7359 input tokens then 202 the next turn), so
/// the app accumulates; turns lost to failure stay missing.
#[derive(Debug, Clone, Default)]
pub struct TokenSums {
    pub input: u64,
    pub output: u64,
}

/// Add one turn's usage to the running sums. Pure.
pub fn add_usage(sums: &mut TokenSums, usage: &Usage) {
    sums.input += usage.input_tokens;
    sums.output += usage.output_tokens;
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
    fn token_sums_accumulate_per_turn_usage() {
        let mut sums = TokenSums::default();
        for (total, input, output) in [(9154, 7359, 3), (9165, 202, 3)] {
            add_usage(&mut sums, &Usage::new(total, input, output));
        }
        assert_eq!(sums.input, 7561);
        assert_eq!(sums.output, 6);
    }

    #[test]
    fn context_pct_divides_used_by_size() {
        assert!((context_pct(9151, 200_000) - 4.5755).abs() < 0.001);
        assert_eq!(context_pct(0, 200_000), 0.0);
        assert_eq!(context_pct(10, 0), 0.0);
    }
}
