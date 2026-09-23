package finder

import (
	"os"
	"path/filepath"
	"testing"
)

func TestIsWSL(t *testing.T) {
	// A file the test controls, so the result does not depend on the kernel
	// the tests happen to run on.
	dir := t.TempDir()
	write := func(s string) string {
		p := filepath.Join(dir, "osrelease")
		if err := os.WriteFile(p, []byte(s), 0o600); err != nil {
			t.Fatalf("WriteFile: %v", err)
		}
		return p
	}
	old := osReleasePath
	defer func() { osReleasePath = old }()

	for _, tc := range []struct {
		name, env, osrelease string
		want                 bool
	}{
		{"wsl2 kernel", "", "6.6.87.2-microsoft-standard-WSL2\n", true},
		{"plain linux", "", "6.8.0-45-generic\n", false},
		{"env alone", "Ubuntu-26.04", "6.8.0-45-generic\n", true},
		{"no osrelease file", "", "", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			t.Setenv("WSL_DISTRO_NAME", tc.env)
			if tc.osrelease == "" {
				osReleasePath = filepath.Join(dir, "missing")
			} else {
				osReleasePath = write(tc.osrelease)
			}
			if got := isWSL(); got != tc.want {
				t.Errorf("isWSL() = %v, want %v", got, tc.want)
			}
		})
	}
}
