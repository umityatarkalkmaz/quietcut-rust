//! Half-open time intervals `[start, end)` in seconds and the set algebra the
//! silence pipeline needs.

/// A time range in seconds. `start` is always meant to be <= `end`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub start: f64,
    pub end: f64,
}

impl Interval {
    pub fn new(start: f64, end: f64) -> Self {
        Self { start, end }
    }

    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Sorts intervals and merges every overlapping or touching pair.
pub fn merge_intervals(intervals: &[Interval]) -> Vec<Interval> {
    let mut sorted: Vec<Interval> = intervals
        .iter()
        .copied()
        .filter(|iv| !iv.is_empty())
        .collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));

    let mut merged: Vec<Interval> = Vec::with_capacity(sorted.len());
    for interval in sorted {
        match merged.last_mut() {
            Some(last) if interval.start <= last.end => last.end = last.end.max(interval.end),
            _ => merged.push(interval),
        }
    }
    merged
}

/// Returns the ranges covered by both inputs.
pub fn intersect_intervals(left: &[Interval], right: &[Interval]) -> Vec<Interval> {
    let left = merge_intervals(left);
    let right = merge_intervals(right);

    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        let start = left[i].start.max(right[j].start);
        let end = left[i].end.min(right[j].end);
        if end > start {
            result.push(Interval::new(start, end));
        }
        if left[i].end < right[j].end {
            i += 1;
        } else {
            j += 1;
        }
    }
    result
}

/// Pulls both edges of every interval inwards by `padding` seconds and drops
/// the ones that collapse.
pub fn shrink_intervals(intervals: &[Interval], padding: f64) -> Vec<Interval> {
    intervals
        .iter()
        .map(|iv| Interval::new(iv.start + padding, iv.end - padding))
        .filter(|iv| !iv.is_empty())
        .collect()
}

/// Keeps intervals lasting at least `min_duration` seconds.
pub fn filter_intervals(intervals: &[Interval], min_duration: f64) -> Vec<Interval> {
    intervals
        .iter()
        .copied()
        .filter(|iv| iv.duration() >= min_duration)
        .collect()
}

/// Clips intervals to `span`, dropping anything that falls outside it.
pub fn clamp_intervals(intervals: &[Interval], span: Interval) -> Vec<Interval> {
    intervals
        .iter()
        .map(|iv| Interval::new(iv.start.max(span.start), iv.end.min(span.end)))
        .filter(|iv| !iv.is_empty())
        .collect()
}

/// Returns the parts of `span` that the given intervals do not cover.
pub fn complement_intervals(intervals: &[Interval], span: Interval) -> Vec<Interval> {
    let covered = merge_intervals(&clamp_intervals(intervals, span));

    let mut result = Vec::new();
    let mut cursor = span.start;
    for interval in covered {
        if interval.start > cursor {
            result.push(Interval::new(cursor, interval.start));
        }
        cursor = cursor.max(interval.end);
    }
    if cursor < span.end {
        result.push(Interval::new(cursor, span.end));
    }
    result
}

/// Adds up the durations of the given intervals.
pub fn sum_durations(intervals: &[Interval]) -> f64 {
    intervals.iter().map(Interval::duration).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intervals(pairs: &[(f64, f64)]) -> Vec<Interval> {
        pairs.iter().map(|&(s, e)| Interval::new(s, e)).collect()
    }

    fn pairs(intervals: &[Interval]) -> Vec<(f64, f64)> {
        intervals.iter().map(|iv| (iv.start, iv.end)).collect()
    }

    #[test]
    fn merge_intervals_sorts_and_joins_overlaps() {
        let input = intervals(&[(10.0, 12.0), (0.0, 5.0), (4.0, 6.0), (12.0, 13.0)]);
        assert_eq!(pairs(&merge_intervals(&input)), [(0.0, 6.0), (10.0, 13.0)]);
    }

    #[test]
    fn merge_intervals_drops_empty_ranges() {
        let input = intervals(&[(3.0, 3.0), (5.0, 4.0), (1.0, 2.0)]);
        assert_eq!(pairs(&merge_intervals(&input)), [(1.0, 2.0)]);
    }

    #[test]
    fn merge_intervals_keeps_nested_ranges_whole() {
        let input = intervals(&[(0.0, 10.0), (2.0, 3.0)]);
        assert_eq!(pairs(&merge_intervals(&input)), [(0.0, 10.0)]);
    }

    #[test]
    fn intersect_intervals_returns_common_ranges() {
        let mic = intervals(&[(0.0, 5.0), (8.0, 20.0)]);
        let discord = intervals(&[(3.0, 10.0), (15.0, 16.0)]);
        assert_eq!(
            pairs(&intersect_intervals(&mic, &discord)),
            [(3.0, 5.0), (8.0, 10.0), (15.0, 16.0)]
        );
    }

    #[test]
    fn intersect_intervals_ignores_touching_edges() {
        let left = intervals(&[(0.0, 5.0)]);
        let right = intervals(&[(5.0, 9.0)]);
        assert!(intersect_intervals(&left, &right).is_empty());
    }

    #[test]
    fn intersect_intervals_with_empty_side_is_empty() {
        assert!(intersect_intervals(&intervals(&[(0.0, 5.0)]), &[]).is_empty());
    }

    #[test]
    fn shrink_intervals_trims_both_edges() {
        let input = intervals(&[(10.0, 12.0), (20.0, 20.2)]);
        assert_eq!(pairs(&shrink_intervals(&input, 0.15)), [(10.15, 11.85)]);
    }

    #[test]
    fn filter_intervals_applies_minimum_duration() {
        let input = intervals(&[(0.0, 0.5), (1.0, 2.0), (3.0, 3.6)]);
        assert_eq!(
            pairs(&filter_intervals(&input, 0.6)),
            [(1.0, 2.0), (3.0, 3.6)]
        );
    }

    #[test]
    fn complement_intervals_returns_keep_segments() {
        let silences = intervals(&[(5.0, 8.0), (12.0, 15.0)]);
        let keep = complement_intervals(&silences, Interval::new(0.0, 20.0));
        assert_eq!(pairs(&keep), [(0.0, 5.0), (8.0, 12.0), (15.0, 20.0)]);
    }

    #[test]
    fn complement_intervals_handles_leading_and_trailing_silence() {
        let silences = intervals(&[(0.0, 4.0), (18.0, 20.0)]);
        let keep = complement_intervals(&silences, Interval::new(0.0, 20.0));
        assert_eq!(pairs(&keep), [(4.0, 18.0)]);
    }

    #[test]
    fn complement_intervals_of_full_silence_is_empty() {
        let silences = intervals(&[(0.0, 25.0)]);
        assert!(complement_intervals(&silences, Interval::new(0.0, 20.0)).is_empty());
    }

    #[test]
    fn complement_intervals_without_silence_returns_whole_span() {
        let keep = complement_intervals(&[], Interval::new(0.0, 20.0));
        assert_eq!(pairs(&keep), [(0.0, 20.0)]);
    }

    #[test]
    fn clamp_intervals_clips_to_span() {
        let input = intervals(&[(-3.0, 2.0), (18.0, 25.0), (30.0, 40.0)]);
        let clamped = clamp_intervals(&input, Interval::new(0.0, 20.0));
        assert_eq!(pairs(&clamped), [(0.0, 2.0), (18.0, 20.0)]);
    }

    #[test]
    fn sum_durations_adds_every_interval() {
        assert_eq!(sum_durations(&intervals(&[(0.0, 2.5), (10.0, 11.5)])), 4.0);
    }
}
