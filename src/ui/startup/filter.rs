#[derive(Copy, Clone)]
pub(super) enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}

impl Op {
    pub(super) fn matches(self, value: f64, target: f64) -> bool {
        match self {
            Op::Ge => value >= target,
            Op::Gt => value > target,
            Op::Le => value <= target,
            Op::Lt => value < target,
            Op::Eq => (value - target).abs() < 0.05,
        }
    }
}

pub(super) fn parse_time_filter(s: &str) -> Option<(Op, f64)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
        (Op::Ge, r)
    } else if let Some(r) = s.strip_prefix("<=") {
        (Op::Le, r)
    } else if let Some(r) = s.strip_prefix('>') {
        (Op::Gt, r)
    } else if let Some(r) = s.strip_prefix('<') {
        (Op::Lt, r)
    } else if let Some(r) = s.strip_prefix('=') {
        (Op::Eq, r)
    } else {
        (Op::Ge, s)
    };
    let rest = rest.trim().trim_end_matches('s').trim();
    rest.parse::<f64>().ok().map(|v| (op, v))
}

pub(crate) fn format_boot_time(ms: Option<u64>) -> String {
    match ms {
        None => "—".into(),
        Some(0) => "<1 ms".into(),
        Some(ms) if ms < 1_000 => format!("{ms} ms"),
        Some(ms) if ms < 60_000 => format!("{:.2} s", ms as f64 / 1_000.0),
        Some(ms) => {
            let total_s = ms / 1_000;
            let min = total_s / 60;
            let s = total_s % 60;
            format!("{min}m {s}s")
        }
    }
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;
