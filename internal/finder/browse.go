package finder

import (
	"os"
	"os/exec"
	"runtime"
	"strings"
)

// osReleasePath is where the kernel version string lives on Linux. It is a
// variable so the tests can point isWSL at a file they control.
var osReleasePath = "/proc/sys/kernel/osrelease"

// isWSL reports whether this Linux is running under WSL, where a browser is
// a Windows program rather than something on $PATH.
func isWSL() bool {
	if os.Getenv("WSL_DISTRO_NAME") != "" {
		return true
	}
	b, err := os.ReadFile(osReleasePath)
	return err == nil && strings.Contains(strings.ToLower(string(b)), "microsoft")
}

// openURL hands a URL to the platform's browser. It is a variable so the
// tests can watch what the finder would have opened.
var openURL = func(url string) error {
	// Detached, in every branch: the browser outlives the finder, and its
	// output must not land on the terminal the TUI is drawing on.
	if runtime.GOOS == "darwin" {
		return exec.Command("open", url).Start()
	}
	if isWSL() {
		// A WSL distribution usually has no xdg-open. wslview is the one
		// wslu installs; explorer.exe is always there, and hands the URL to
		// the Windows default browser. Neither re-parses its argument the
		// way "cmd /c start" and "powershell -Command" would.
		for _, opener := range []string{"wslview", "explorer.exe"} {
			if p, err := exec.LookPath(opener); err == nil {
				return exec.Command(p, url).Start()
			}
		}
	}
	return exec.Command("xdg-open", url).Start()
}
