//! Age / duration parsing for job purge (`5d`, `12h`, `30m`, `90s`).

use crate::error::{GnosisError, Result};
use chrono::Duration;

/// Parse a human age like `5d`, `12h`, `30m`, or `90s` into a chrono [`Duration`].
pub fn parse_age(input: &str) -> Result<Duration> {
    let s = input.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err(GnosisError::Job(
            "age required (e.g. 5d, 12h, 30m, 90s)".into(),
        ));
    }

    let (num_str, unit) = if let Some(u) = s
        .chars()
        .last()
        .filter(|c| matches!(c, 'd' | 'h' | 'm' | 's'))
    {
        (&s[..s.len() - 1], u)
    } else {
        return Err(GnosisError::Job(format!(
            "invalid age '{input}' (expected e.g. 5d, 12h, 30m, 90s)"
        )));
    };

    let n: i64 = num_str.trim().parse().map_err(|_| {
        GnosisError::Job(format!(
            "invalid age '{input}' (expected e.g. 5d, 12h, 30m, 90s)"
        ))
    })?;
    if n <= 0 {
        return Err(GnosisError::Job(
            "age must be a positive duration (e.g. 5d)".into(),
        ));
    }

    let dur = match unit {
        'd' => Duration::days(n),
        'h' => Duration::hours(n),
        'm' => Duration::minutes(n),
        's' => Duration::seconds(n),
        _ => unreachable!(),
    };
    Ok(dur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units() {
        assert_eq!(parse_age("5d").unwrap(), Duration::days(5));
        assert_eq!(parse_age("12H").unwrap(), Duration::hours(12));
        assert_eq!(parse_age("30m").unwrap(), Duration::minutes(30));
        assert_eq!(parse_age("90s").unwrap(), Duration::seconds(90));
    }

    #[test]
    fn rejects_bad() {
        assert!(parse_age("").is_err());
        assert!(parse_age("5").is_err());
        assert!(parse_age("0d").is_err());
        assert!(parse_age("-1h").is_err());
        assert!(parse_age("5w").is_err());
    }
}
