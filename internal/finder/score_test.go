package finder

import (
	"testing"
)

func TestMatchScoreBeatsScatteredMatches(t *testing.T) {
	const run = "github.com/jedipunkz/miniecs"
	const scattered = "github.com/jedipunkz/spacex-ipo-checker"
	runScore := matchScore(run, substringMatch(run, "miniec"))
	// m-i-n from the host and user, then i, e, c spread through the name.
	scatteredScore := matchScore(scattered, []int{9, 14, 17, 29, 35, 36})
	if runScore <= scatteredScore {
		t.Errorf("contiguous match scored %v, scattered scored %v", runScore, scatteredScore)
	}
}
