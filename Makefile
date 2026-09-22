BIN := gm

.PHONY: all build install test vet fmt check clean

all: build

build:
	go build -o $(BIN) .

install:
	go install .

test:
	go test ./...

vet:
	go vet ./...

fmt:
	gofmt -w .

check: vet test

clean:
	$(RM) $(BIN)
