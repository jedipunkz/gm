//! Ranking. The fuzzy matcher finds the candidates; match_score decides the
//! order they are shown in.

/// Ranking weights. Match quality decides the order; frecency only breaks
/// ties. Every repository path shares a long `host/user/` prefix, so a
/// subsequence scattered across that prefix must never beat a run of
/// characters inside the repository name itself.
const BONUS_BOUNDARY: f64 = 8.0; // the match starts a new word (after / - _ .)
const BONUS_CONSECUTIVE: f64 = 6.0; // the match continues the previous one
const BONUS_NAME_SEG: f64 = 10.0; // the match is inside the repository name
const BONUS_USER_SEG: f64 = 3.0; // ...or at least inside the user name
const PENALTY_HOST_SEG: f64 = 4.0; // the host is the same for nearly every repo: noise
const BONUS_WHOLE_IN_NAME: f64 = 25.0; // the whole query lives in the repository name
const PENALTY_GAP_START: f64 = 3.0; // each run of skipped characters costs this...
const PENALTY_GAP_CHAR: f64 = 1.0; // ...plus this per character skipped
const MAX_GAP_CHARS: usize = 20; // a long path must not dominate the score

fn is_boundary(c: u8) -> bool {
    matches!(c, b'/' | b'-' | b'_' | b'.')
}

/// match_score rates one match by where its characters landed, fzy-style:
/// boundaries and adjacency earn, gaps cost. idx holds byte offsets into
/// text, ascending.
pub fn match_score(text: &str, idx: &[usize]) -> f64 {
    if idx.is_empty() {
        return 0.0;
    }
    let b = text.as_bytes();
    let name = text.rfind('/').map_or(0, |i| i + 1);
    let user = if name > 1 {
        text[..name - 1].rfind('/').map_or(0, |i| i + 1)
    } else {
        0
    };

    let mut score = 0.0;
    let mut prev: Option<usize> = None;
    let mut whole_in_name = true;
    for &i in idx {
        let in_host = i < user;
        if i >= name {
            score += BONUS_NAME_SEG;
        } else if i >= user {
            score += BONUS_USER_SEG;
            whole_in_name = false;
        } else {
            score -= PENALTY_HOST_SEG;
            whole_in_name = false;
        }
        // A word boundary only counts where the words mean something; every
        // candidate shares the same host, so boundaries there are free points.
        if !in_host && (i == 0 || is_boundary(b[i - 1])) {
            score += BONUS_BOUNDARY;
        }
        match prev {
            Some(p) if i == p + 1 => score += BONUS_CONSECUTIVE,
            Some(p) => {
                score -=
                    PENALTY_GAP_START + PENALTY_GAP_CHAR * (i - p - 1).min(MAX_GAP_CHARS) as f64
            }
            None => {}
        }
        prev = Some(i);
    }
    if whole_in_name {
        score += BONUS_WHOLE_IN_NAME;
    }
    score
}

/// substring_match returns the byte offsets of query inside text when it
/// occurs literally. A typed substring is what the user meant, so it wins over
/// any subsequence the fuzzy matcher would piece together from elsewhere.
pub fn substring_match(text: &str, lower_query: &str) -> Option<Vec<usize>> {
    if text.is_ascii() {
        let at = text.to_lowercase().find(lower_query)?;
        return Some(lower_query.char_indices().map(|(i, _)| at + i).collect());
    }

    let query: Vec<char> = lower_query.chars().collect();
    if query.is_empty() {
        return Some(Vec::new());
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    chars
        .windows(query.len())
        .find(|window| {
            window
                .iter()
                .zip(&query)
                .all(|((_, candidate), query)| equal_fold(*candidate, *query))
        })
        .map(|window| window.iter().map(|&(offset, _)| offset).collect())
}

/// Match is one candidate the fuzzy matcher kept.
pub struct Match {
    pub index: usize,
    pub matched: Vec<usize>, // byte offsets, ascending
    pub score: i64,
}

// The weights of github.com/sahilm/fuzzy, which the Go finder used and whose
// choices this reproduces: which rows match, and where their characters land.
const FIRST_CHAR_MATCH_BONUS: i64 = 10;
const MATCH_FOLLOWING_SEPARATOR_BONUS: i64 = 20;
const CAMEL_CASE_MATCH_BONUS: i64 = 20;
const ADJACENT_MATCH_BONUS: i64 = 5;
const UNMATCHED_LEADING_CHAR_PENALTY: i64 = -5;
const MAX_UNMATCHED_LEADING_CHAR_PENALTY: i64 = -15;

fn is_separator(c: char) -> bool {
    "/-_ .\\".contains(c)
}

fn equal_fold(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// find is sahilm/fuzzy's FindFrom: every candidate holding the pattern's
/// characters in order, ignoring case, best first. Equal scores keep the
/// candidates' own order.
pub fn find(pattern: &str, data: &[&str]) -> Vec<Match> {
    let runes: Vec<char> = pattern.chars().collect();
    if runes.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for (index, full) in data.iter().enumerate() {
        let s = full.split('\0').next().unwrap_or("");
        let chars: Vec<(usize, char)> = s.char_indices().collect();
        let mut m = Match {
            index,
            matched: Vec::with_capacity(runes.len()),
            score: 0,
        };

        let mut pattern_index = 0;
        let mut best_score = -1;
        let mut matched_index: Option<usize> = None;
        let mut curr_adjacent_bonus = 0;
        let mut last = '\0';
        let mut last_index = 0;
        for (k, &(j, candidate)) in chars.iter().enumerate() {
            if pattern_index < runes.len() && equal_fold(candidate, runes[pattern_index]) {
                let mut score = 0;
                if j == 0 {
                    score += FIRST_CHAR_MATCH_BONUS;
                }
                if last.is_lowercase() && candidate.is_uppercase() {
                    score += CAMEL_CASE_MATCH_BONUS;
                }
                if j != 0 && is_separator(last) {
                    score += MATCH_FOLLOWING_SEPARATOR_BONUS;
                }
                if let Some(&last_match) = m.matched.last() {
                    let bonus = if last_match == last_index {
                        curr_adjacent_bonus * 2 + ADJACENT_MATCH_BONUS
                    } else {
                        0
                    };
                    score += bonus;
                    curr_adjacent_bonus += bonus;
                }
                if score > best_score {
                    best_score = score;
                    matched_index = Some(j);
                }
            }
            let next_p = runes.get(pattern_index + 1).copied();
            let next_c = chars.get(k + 1).map(|&(_, c)| c);
            // A pattern character is settled on once the next one could
            // start, or the text has run out.
            let settle = match (next_p, next_c) {
                (_, None) => true,
                (Some(p), Some(c)) => equal_fold(p, c),
                (None, Some(_)) => false,
            };
            if settle && let Some(mi) = matched_index {
                if m.matched.is_empty() {
                    let penalty = mi as i64 * UNMATCHED_LEADING_CHAR_PENALTY;
                    best_score += penalty.max(MAX_UNMATCHED_LEADING_CHAR_PENALTY);
                }
                m.score += best_score;
                m.matched.push(mi);
                best_score = -1;
                matched_index = None;
                pattern_index += 1;
            }
            last_index = j;
            last = candidate;
        }
        m.score += m.matched.len() as i64 - s.len() as i64;
        if m.matched.len() == runes.len() {
            matches.push(m);
        }
    }
    matches.sort_by_key(|m| std::cmp::Reverse(m.score)); // stable
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_beats_scattered_matches() {
        let run = "github.com/jedipunkz/miniecs";
        let scattered = "github.com/jedipunkz/spacex-ipo-checker";
        let run_score = match_score(run, &substring_match(run, "miniec").unwrap());
        // m-i-n from the host and user, then i, e, c spread through the name.
        let scattered_score = match_score(scattered, &[9, 14, 17, 29, 35, 36]);
        assert!(
            run_score > scattered_score,
            "{run_score} <= {scattered_score}"
        );
    }

    #[test]
    fn substring_match_returns_original_offsets_after_unicode_case_mapping() {
        for (text, expected) in [
            ("İmatch", vec![2, 3, 4, 5, 6]),
            ("Kmatch", vec![3, 4, 5, 6, 7]),
        ] {
            let positions = substring_match(text, "match").unwrap();

            assert_eq!(positions, expected);
            assert!(
                positions
                    .iter()
                    .all(|&position| text.is_char_boundary(position))
            );
            let _ = match_score(text, &positions);
        }
    }

    #[test]
    fn find_keeps_ordered_subsequences() {
        let data = [
            "github.com/acme/alpha",
            "github.com/acme/bravo",
            "github.com/other/charlie",
        ];
        let got: Vec<usize> = find("brv", &data).iter().map(|m| m.index).collect();
        assert_eq!(got, vec![1]);
        assert!(find("zzz", &data).is_empty());
        // A character is settled on where the next one could start, so the
        // "a" is acme's: the literal run is substring_match's to find.
        let m = &find("ALP", &data)[0];
        assert_eq!((m.index, m.matched.as_slice()), (0, &[11, 17, 18][..]));
    }
}
