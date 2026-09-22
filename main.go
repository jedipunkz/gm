// Command gm keeps every repository in one predictable tree, and jumps between
// them. The work lives in internal/; this is only the entry point.
package main

import (
	"log"
	"os"

	"github.com/jedipunkz/gm/internal/cli"
)

// version is stamped in at release time with -ldflags -X main.version=...
var version = "dev"

func main() {
	cli.Version = version
	log.SetFlags(0)
	log.SetPrefix("gm: ")

	if err := cli.Run(os.Args[1:]); err != nil {
		if cli.IsUsageError(err) {
			os.Exit(2)
		}
		log.Fatal(err)
	}
}
