/// Case-insensitive match score; higher is better, `None` means no match.
/// Prefix beats substring beats subsequence; shorter candidates win ties so
/// `But` ranks `Button` above `ButtonGroupHeader`.
pub fn score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_ascii_lowercase();
    let c = candidate.to_ascii_lowercase();
    let len_penalty = candidate.len() as i32;
    if c.starts_with(&q) {
        return Some(3000 - len_penalty);
    }
    if c.contains(&q) {
        return Some(2000 - len_penalty);
    }
    let mut chars = c.chars();
    if q.chars().all(|qc| chars.any(|cc| cc == qc)) {
        return Some(1000 - len_penalty);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything_equally() {
        assert_eq!(score("", "Button"), Some(0));
    }

    #[test]
    fn prefix_beats_substring_beats_subsequence() {
        let prefix = score("but", "Button").unwrap();
        let substring = score("but", "BackButton").unwrap();
        let subsequence = score("bt", "Button").unwrap();
        assert!(prefix > substring, "{prefix} vs {substring}");
        assert!(substring > subsequence, "{substring} vs {subsequence}");
    }

    #[test]
    fn shorter_candidate_wins_within_a_class() {
        assert!(score("but", "Button").unwrap() > score("but", "ButtonGroupHeader").unwrap());
    }

    #[test]
    fn non_matches_return_none() {
        assert_eq!(score("xyz", "Button"), None);
        // subsequence must respect character order
        assert_eq!(score("nb", "Button"), None);
    }
}
