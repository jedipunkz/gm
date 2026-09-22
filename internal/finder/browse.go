package finder

import (
	"os/exec"
	"runtime"
)

// openURL hands a URL to the platform's browser. It is a variable so the
// tests can watch what the finder would have opened.
var openURL = func(url string) error {
	opener := "xdg-open"
	if runtime.GOOS == "darwin" {
		opener = "open"
	}
	// Detached: the browser outlives the finder, and its output must not
	// land on the terminal the TUI is drawing on.
	return exec.Command(opener, url).Start()
}
