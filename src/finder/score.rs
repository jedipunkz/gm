//! Ranking. The fuzzy matcher finds the candidates; match_score decides the
//! order they are shown in.

/// Ranking weights. Match quality decides the order; frecency only breaks
/// ties. Every repository path shares a long `host/user/` prefix, so a
/// subsequence scattered across that prefix must never beat a run of
/// characters inside the repository name itself.
const BONUS_BOUNDARY: f64 = 8.0; // the match starts a new word (after / - _ . space)
const BONUS_CONSECUTIVE: f64 = 6.0; // the match continues the previous one
const BONUS_NAME_SEG: f64 = 10.0; // the match is inside the repository name
const BONUS_USER_SEG: f64 = 3.0; // ...or at least inside the user name
const PENALTY_HOST_SEG: f64 = 4.0; // the host is the same for nearly every repo: noise
const BONUS_WHOLE_IN_NAME: f64 = 25.0; // the whole query lives in the repository name
const BONUS_NAME_IS_QUERY: f64 = 25.0; // the query spells the repository name whole
const PENALTY_GAP_START: f64 = 3.0; // each run of skipped characters costs this...
const PENALTY_GAP_CHAR: f64 = 1.0; // ...plus this per character skipped
const MAX_GAP_CHARS: usize = 20; // a long path must not dominate the score

fn is_boundary(c: u8) -> bool {
    matches!(c, b'/' | b'-' | b'_' | b'.' | b' ')
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
/// any subsequence the fuzzy matcher would piece together from elsewhere. A
/// label can hold the query more than once: the occurrence match_score ranks
/// highest wins, so "gm" lands on the repository it names, not in the user
/// name an earlier "gm" happens to sit in.
pub fn substring_match(text: &str, lower_query: &str) -> Option<Vec<usize>> {
    // consider keeps the first of equally good landings, which is what the
    // lists showed before the best landing was sought at all.
    let mut best: Option<(f64, Vec<usize>)> = None;
    let mut consider = |pos: Vec<usize>| {
        let score = match_score(text, &pos);
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, pos));
        }
    };

    if text.is_ascii() {
        // An ASCII text lowers bytewise, so the query can be found without
        // lowering the whole text into a fresh string.
        let needle = lower_query.as_bytes();
        if needle.is_empty() {
            return Some(Vec::new());
        }
        let hay = text.as_bytes();
        for (at, window) in hay.windows(needle.len()).enumerate() {
            if window
                .iter()
                .zip(needle)
                .all(|(a, b)| a.eq_ignore_ascii_case(b))
            {
                consider((at..at + needle.len()).collect());
            }
        }
        return best.map(|(_, pos)| pos);
    }

    let query: Vec<char> = lower_query.chars().collect();
    if query.is_empty() {
        return Some(Vec::new());
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for window in chars.windows(query.len()) {
        if window
            .iter()
            .zip(&query)
            .all(|((_, candidate), query)| equal_fold(*candidate, *query))
        {
            consider(window.iter().map(|&(offset, _)| offset).collect());
        }
    }
    best.map(|(_, pos)| pos)
}

/// rank scores one label the filter sorts by: where match_score put the
/// characters, promoted when the query spells the label's own name whole.
/// No history is needed for "gm" to mean github.com/x/gm over the
/// repositories that merely carry "gm" inside a longer name or user.
pub fn rank(text: &str, pos: &[usize], lower_query: &str) -> f64 {
    let mut score = match_score(text, pos);
    let name = text.rfind('/').map_or(0, |i| i + 1);
    let name_of = &text[name..];
    if name_of.eq_ignore_ascii_case(lower_query)
        || (!name_of.is_ascii() && name_of.to_lowercase() == lower_query)
    {
        score += BONUS_NAME_IS_QUERY;
    }
    score
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
        let mut chars = s.char_indices().peekable();
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
        while let Some((j, candidate)) = chars.next() {
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
            let next_c = chars.peek().map(|&(_, c)| c);
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

    // The query can sit in a label more than once; "gm" in gmdev/gm belongs
    // where it names the repository, not in the user name before it.
    #[test]
    fn substring_match_picks_the_meaningful_occurrence() {
        assert_eq!(
            substring_match("github.com/gmdev/gm", "gm").unwrap(),
            vec![17, 18]
        );
        // An equal landing keeps the first, and none keeps None.
        assert_eq!(
            substring_match("github.com/acme/note-note", "note").unwrap(),
            vec![16, 17, 18, 19]
        );
        assert_eq!(
            substring_match("github.com/acme/alpha", "zzz"),
            Option::<Vec<usize>>::None
        );
    }

    // "gm" means github.com/x/gm: the repository the query names whole
    // outranks the ones that carry it inside a longer name or a user, with
    // no visit history to say otherwise.
    #[test]
    fn a_name_the_query_equals_outranks_partial_hits() {
        let whole = "github.com/jedipunkz/gm";
        let partial = "github.com/gmdev/tools";
        let query = "gm";
        assert!(
            rank(whole, &substring_match(whole, query).unwrap(), query)
                > rank(partial, &substring_match(partial, query).unwrap(), query)
        );
        // The same holds after a case fold outside ASCII.
        let folded = "github.com/x/Tök";
        let positions = substring_match(folded, "tök").unwrap();
        assert!(rank(folded, &positions, "tök") > match_score(folded, &positions));
    }

    // The PR list is worded in spaces, and a query after one is at a word's
    // start the way a query after a slash is.
    #[test]
    fn a_space_starts_a_word() {
        let after_space = match_score("#123 fix here", &[5, 6, 7]);
        let mid_word = match_score("#123 tfix here", &[6, 7, 8]);
        assert!(after_space > mid_word, "{after_space} <= {mid_word}");
    }

    // Edge paths the callers reach with an empty query or an unhittable text:
    // nothing to rank is zero, and a query with nothing to search finds
    // nothing.
    #[test]
    fn empty_inputs_answer_empty() {
        assert_eq!(match_score("github.com/x/y", &[]), 0.0);
        assert_eq!(
            substring_match("github.com/x/y", ""),
            Some(Vec::new()),
            "the costliest false hit is none at all"
        );
        assert_eq!(
            substring_match("", "zz"),
            Option::<Vec<usize>>::None,
            "an empty text cannot hold a query"
        );
        assert!(find("", &["github.com/x/y"]).is_empty());
    }
}

// Property tests. Fuzzy filters run on whatever a repository is named and
// whatever it is the user types, both of which are Unicode nobody prereads:
// so no sequence of characters may panic the matcher, and every match it
// reports must point at real places in its text. The example tests above
// hold the cases someone thought of; these hold for everything else.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// labels a candidate may carry: the repository paths gm ranks mostly,
    /// with their \0 tail bytes, and any garble proptest can invent.
    fn label() -> impl Strategy<Value = String> + 'static {
        prop_oneof![
            any::<String>(),
            "(github\\.com/[a-z]{1,10}/[a-zA-Z0-9][a-zA-Z0-9._ -]{0,12}(\\x00[0-9-]{0,12})?)",
            "(github\\.com/[a-z]{1,10}/){0,2}[a-zA-Z0-9][a-zA-Z0-9._ -]{0,13}(\\x00[0-9-]{0,12})?",
        ]
    }

    proptest! {
        // 256 cases a head; every proptest in this module runs the same.
        #![proptest_config(ProptestConfig::with_cases(256))]

        // No pattern may panic the matcher, whatever the labels. And when
        // the pattern matches, the match is a real answer: one position per
        // pattern character, pointing at the letters it matched, at
        // ascending char boundaries inside its own text, best scores first
        // and on top the candidates' own order.
        #[test]
        fn find_answers_well_formed_matches(
            pattern in any::<String>(),
            labels in proptest::collection::vec(label(), 0..8),
        ) {
            let texts: Vec<&str> = labels.iter().map(String::as_str).collect();
            let runes: Vec<char> = pattern.chars().collect();
            let matches = find(&pattern, &texts);
            prop_assert!(
                matches.windows(2).all(|w| w[0].score >= w[1].score),
                "scores out of order for {:?}",
                pattern
            );
            for m in &matches {
                let text = texts[m.index];
                prop_assert_eq!(
                    m.matched.len(),
                    runes.len(),
                    "positions against the pattern in {:?}",
                    text
                );
                let in_order = m.matched.windows(2).all(|w| w[0] < w[1]);
                prop_assert!(in_order, "positions out of order in {:?}", text);
                for (i, r) in m.matched.iter().zip(&runes) {
                    prop_assert!(*i < text.len(), "position {} out of {:?}", i, text);
                    prop_assert!(text.is_char_boundary(*i), "mid-char {} in {:?}", i, text);
                    prop_assert!(
                        text[*i..]
                            .chars()
                            .next()
                            .is_some_and(|c| equal_fold(c, *r)),
                        "position {} in {:?} is not {:?}'s",
                        i, text, r
                    );
                }
            }
        }

        // A literal match points at the query's letters, one position each,
        // in range, on whole characters, strictly ascending.
        #[test]
        fn substring_match_points_at_the_query(
            text in any::<String>(),
            query in any::<String>(),
        ) {
            // The callers hand a lowercased query; fold the same way here.
            let lower = query.to_lowercase();
            if let Some(pos) = substring_match(&text, &lower) {
                for place in pos.iter().zip(lower.chars()) {
                    prop_assert!(
                        place.0 < &text.len(),
                        "position {} out of {:?}",
                        place.0,
                        text
                    );
                    prop_assert!(
                        text.is_char_boundary(*place.0),
                        "mid-char {} in {:?}",
                        place.0,
                        text
                    );
                    prop_assert!(
                        text[*place.0..]
                            .chars()
                            .next()
                            .is_some_and(|t| equal_fold(t, place.1)),
                        "position {} in {:?} is not {:?}'s",
                        place.0,
                        text,
                        place.1
                    );
                }
                prop_assert!(
                    pos.windows(2).all(|w| w[0] < w[1]),
                    "positions out of order in {:?}",
                    text
                );
                prop_assert_eq!(
                    pos.len(),
                    lower.chars().count(),
                    "length against {:?}",
                    text
                );
            }
        }
    }
}
