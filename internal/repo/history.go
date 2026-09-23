package repo

import (
	"encoding/json"
	"os"
	"path/filepath"
	"time"
)

// Visit is how often and how recently one repository was entered.
type Visit struct {
	Count int   `json:"count"`
	Last  int64 `json:"last"` // unix seconds
}

// Score weighs frequency by recency, the way z and zoxide do: a repository
// visited twice this hour outranks one visited ten times last month.
func (v Visit) Score(now time.Time) float64 {
	age := now.Sub(time.Unix(v.Last, 0))
	switch {
	case age < time.Hour:
		return float64(v.Count) * 4
	case age < 24*time.Hour:
		return float64(v.Count) * 2
	case age < 7*24*time.Hour:
		return float64(v.Count) * 0.5
	default:
		return float64(v.Count) * 0.25
	}
}

// History is the visit log. It holds its own file path, so a test can point it
// at a temporary directory instead of the user's state directory.
type History struct {
	Path   string
	visits map[string]Visit
}

// HistoryFile is where the log lives:
//
//	$XDG_STATE_HOME/gm/frecency.json, else ~/.local/state/gm/frecency.json
func HistoryFile() (string, error) {
	dir := os.Getenv("XDG_STATE_HOME")
	if dir == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		dir = filepath.Join(home, ".local", "state")
	}
	return filepath.Join(dir, "gm", "frecency.json"), nil
}

// LoadHistory reads the visit log. A missing, unreadable or corrupt log is an
// empty one: losing history must never block navigation.
func LoadHistory() *History {
	path, _ := HistoryFile()
	return OpenHistory(path)
}

// OpenHistory reads the log at an explicit path.
func OpenHistory(path string) *History {
	h := &History{Path: path, visits: map[string]Visit{}}
	b, err := os.ReadFile(path)
	if err != nil {
		return h
	}
	_ = json.Unmarshal(b, &h.visits)
	return h
}

// Visit reports what is known about one repository; the zero Visit means
// never entered.
func (h *History) Visit(path string) Visit { return h.visits[path] }

// Bump records a visit and prunes entries whose repository is gone. It applies
// both to the log as it is on disk right now, not to the copy read when this
// History was opened: the finder holds that copy for a whole session, and
// every visit another gm wrote in the meantime would otherwise be written back
// out of existence.
func (h *History) Bump(path string) error {
	if h.Path != "" {
		h.visits = OpenHistory(h.Path).visits
	}

	v := h.visits[path]
	v.Count++
	v.Last = time.Now().Unix()
	h.visits[path] = v

	for p := range h.visits {
		if _, err := os.Stat(p); os.IsNotExist(err) {
			delete(h.visits, p)
		}
	}
	return h.save()
}

// save writes through a temporary file, so an interrupted write cannot leave
// a half-written log behind.
func (h *History) save() error {
	if h.Path == "" {
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(h.Path), 0o755); err != nil {
		return err
	}
	b, err := json.Marshal(h.visits)
	if err != nil {
		return err
	}
	// A unique name, so two gm processes saving at once cannot write and
	// rename the same temporary file.
	f, err := os.CreateTemp(filepath.Dir(h.Path), "frecency-*.tmp")
	if err != nil {
		return err
	}
	tmp := f.Name()
	_, err = f.Write(b)
	if cerr := f.Close(); err == nil {
		err = cerr
	}
	if err == nil {
		err = os.Chmod(tmp, 0o644)
	}
	if err == nil {
		err = os.Rename(tmp, h.Path)
	}
	if err != nil {
		_ = os.Remove(tmp) // no half-written log left next to the real one
	}
	return err
}

// Bump records a visit in the user's log. Commands use it for the one-line
// case where no History is being carried around.
func Bump(path string) error { return LoadHistory().Bump(path) }
