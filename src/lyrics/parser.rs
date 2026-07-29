use crate::lyrics::LyricLine;

/// Parses LRC-formatted lyrics (`[mm:ss.xx]text` per line) into a `Vec<LyricLine>`
/// sorted by `time_ms`. Lines that aren't `[mm:ss.xx]` timestamps (metadata tags
/// like `[ar:...]`, blank lines) fail the numeric parse and are dropped.
pub fn parse_lrc(input: &str) -> Vec<LyricLine> {
    let mut lines: Vec<LyricLine> = input.lines().filter_map(parse_line).collect();
    lines.sort_by_key(|l| l.time_ms);
    lines
}

fn parse_line(line: &str) -> Option<LyricLine> {
    let line = line.trim();
    let rest = line.strip_prefix('[')?;
    let (tag, text) = rest.split_once(']')?;
    let time_ms = parse_timestamp(tag)?;
    Some(LyricLine {
        time_ms,
        text: text.to_string(),
    })
}

fn parse_timestamp(tag: &str) -> Option<u64> {
    let (mm, rest) = tag.split_once(':')?;
    let (ss, frac) = rest.split_once('.')?;
    let mm: u64 = mm.parse().ok()?;
    let ss: u64 = ss.parse().ok()?;
    let frac_ms: u64 = match frac.len() {
        2 => frac.parse::<u64>().ok()? * 10,
        3 => frac.parse::<u64>().ok()?,
        _ => return None,
    };
    Some(mm * 60_000 + ss * 1_000 + frac_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture_sorted_with_metadata_stripped() {
        let input = include_str!("../../tests/fixtures/lyrics/sample.lrc");
        let lines = parse_lrc(input);

        assert_eq!(lines.len(), 5);
        assert!(lines.windows(2).all(|w| w[0].time_ms <= w[1].time_ms));
        assert_eq!(lines[0].time_ms, 0);
        assert_eq!(lines[0].text, "First line");
        assert_eq!(lines[4].time_ms, 15_000);
        assert_eq!(lines[4].text, "Fifth line");
    }

    #[test]
    fn parses_two_digit_and_three_digit_fractions() {
        let lines = parse_lrc("[00:01.50]two digit\n[00:02.500]three digit");
        assert_eq!(lines[0].time_ms, 1_500);
        assert_eq!(lines[1].time_ms, 2_500);
    }

    #[test]
    fn ignores_non_timestamp_lines() {
        let lines = parse_lrc("[ar:Some Artist]\n[00:00.00]only line\n\nplain text, no tag");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "only line");
    }
}
