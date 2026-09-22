package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"time"
)

// visit is how often and how recently one repository was entered.
type visit struct {
	Count int   `json:"count"`
	Last  int64 `json:"last"` // unix seconds
}

// Score weighs frequency by recency, the way z and zoxide do: a repo visited
// twice this hour outranks one visited ten times last month.
func (v visit) Score(now time.Time) float64 {
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

func frecencyFile() (string, error) {
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

// LoadFrecency reads the visit log. A missing or corrupt log is an empty one:
// losing history must never block navigation.
func LoadFrecency() map[string]visit {
	m := map[string]visit{}
	f, err := frecencyFile()
	if err != nil {
		return m
	}
	b, err := os.ReadFile(f)
	if err != nil {
		return m
	}
	_ = json.Unmarshal(b, &m)
	return m
}

// Bump records a visit to path and prunes entries whose repo is gone.
func Bump(path string) error {
	m := LoadFrecency()
	v := m[path]
	v.Count++
	v.Last = time.Now().Unix()
	m[path] = v

	for p := range m {
		if _, err := os.Stat(p); os.IsNotExist(err) {
			delete(m, p)
		}
	}

	f, err := frecencyFile()
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(f), 0o755); err != nil {
		return err
	}
	b, err := json.Marshal(m)
	if err != nil {
		return err
	}
	tmp := f + ".tmp"
	if err := os.WriteFile(tmp, b, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, f)
}
