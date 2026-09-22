package finder

import "strings"

// Ranking weights. Match quality decides the order; frecency only breaks ties.
// Every repository path shares a long `host/user/` prefix, so a subsequence
// scattered across that prefix must never beat a run of characters inside the
// repository name itself.
const (
	bonusBoundary    = 8  // the match starts a new word (after / - _ .)
	bonusConsecutive = 6  // the match continues the previous one
	bonusNameSeg     = 10 // the match is inside the repository name
	bonusUserSeg     = 3  // ...or at least inside the user name
	penaltyHostSeg   = 4  // the host is the same for nearly every repo: noise
	bonusWholeInName = 25 // the whole query lives in the repository name
	penaltyGapStart  = 3  // each run of skipped characters costs this...
	penaltyGapChar   = 1  // ...plus this per character skipped
	maxGapChars      = 20 // a long path must not dominate the score
)

func isBoundary(c byte) bool {
	return c == '/' || c == '-' || c == '_' || c == '.'
}

// matchScore rates one match by where its characters landed, fzy-style:
// boundaries and adjacency earn, gaps cost. idx holds byte offsets into text,
// ascending.
func matchScore(text string, idx []int) float64 {
	if len(idx) == 0 {
		return 0
	}
	name := strings.LastIndexByte(text, '/') + 1
	user := 0
	if name > 1 {
		user = strings.LastIndexByte(text[:name-1], '/') + 1
	}

	var score float64
	prev := -2
	wholeInName := true
	for _, i := range idx {
		inHost := i < user
		switch {
		case i >= name:
			score += bonusNameSeg
		case i >= user:
			score += bonusUserSeg
			wholeInName = false
		default:
			score -= penaltyHostSeg
			wholeInName = false
		}
		// A word boundary only counts where the words mean something; every
		// candidate shares the same host, so boundaries there are free points.
		if !inHost && (i == 0 || isBoundary(text[i-1])) {
			score += bonusBoundary
		}
		if i == prev+1 {
			score += bonusConsecutive
		} else if prev >= 0 {
			score -= penaltyGapStart + penaltyGapChar*float64(min(i-prev-1, maxGapChars))
		}
		prev = i
	}
	if wholeInName {
		score += bonusWholeInName
	}
	return score
}

// substringMatch returns the byte offsets of query inside text when it occurs
// literally. A typed substring is what the user meant, so it wins over any
// subsequence the fuzzy matcher would piece together from elsewhere.
func substringMatch(text, lowerQuery string) []int {
	at := strings.Index(strings.ToLower(text), lowerQuery)
	if at < 0 {
		return nil
	}
	idx := make([]int, 0, len(lowerQuery))
	for i := range lowerQuery {
		idx = append(idx, at+i)
	}
	return idx
}
