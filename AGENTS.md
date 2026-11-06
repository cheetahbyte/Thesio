# Thesio

Thesio is Obsidian for Typst: a plain-file vault of `.typ` notes and papers. Read PRODUCT.md for direction and docs/ARCHITECTURE.md before changing the engine or the app.

## Code

- Go is gofmt-clean. The Rust crate in `typst/` keeps its compact style with rustfmt off; match the surrounding code and its comment density.
- `internal/typst` is the only Go package that touches cgo, and `typst/` the only Rust. Everything needing Typst's parser or compiler goes through the C ABI in `typst/include/thesio_typst.h`; add a function there and in `typst/src/ffi.rs` rather than reparsing in Go.
- Feature packages (`internal/app/document`, `citations`, `papers`) reach the workspace only through `internal/app/host.Host`; they never import `internal/app`. Engine packages (`internal/vault`, `project`, `library`, `pdfkit`) never import MyGo.
- MyGo views are functions of state built every frame: never store a `ui.Element`; keep `ui.Handle`, `ListState`, `ScrollState` in the state; change state from goroutines only through `host.Host.Update`.
- A document's id is its vault-relative path. Anything that renames or moves a file goes through `Workspace.PathChanged`.
- Plain files come first. Keep app state in `.thesio/` only for what a file cannot hold, and keep the vault readable without the app.
- `library.yaml` is the source of truth for citations. Never overwrite a library file that failed to parse.
- All vault paths pass through `vault.Store.Path`, which rejects anything leaving the vault.
- Icons are `theme.Icons.*`, drawn with `theme.Glyph`: an SF Symbol on macOS, the Lucide SVG in `internal/app/theme/icons/` elsewhere. Colors come from `c.Theme()` and the few tokens in `internal/app/theme`; page bars, toolbar buttons and dialogs use its shared helpers.
- Bump `vault.Format` when a change to `.thesio/` would mislead older builds. New Meta fields keep their JSON defaults optional.

## Rigor

Go is held as close to Rust's discipline as it gets; `.golangci.yml` makes the linters build failures, and the rules below cover what no linter can.

- Every error is handled or returned; one from another package is wrapped with context (`fmt.Errorf("...: %w", err)`), one from this module is returned as it is. No `_ = f()`.
- Compare errors with `errors.Is`/`errors.As`, never `==` or string matching; sentinel errors are package variables, not ad-hoc `errors.New` at the call site.
- Type assertions use the two-value form. `switch` over an enum type lists every case; a `default` that handles "the rest" is a bug.
- Nothing panics in ordinary paths: `panic` is for programmer errors that tests would catch, never for input, files or the network. No `unsafe` outside `internal/typst` and `internal/pdfkit`.
- Invalid states are hard to represent: unexported fields with a validating constructor that returns `(T, error)`; optionals as a `(T, bool)` pair or a pointer only where absence means something; values copied defensively at package boundaries.
- Shared state is guarded explicitly (a mutex or a channel), and code that is reached from goroutines has a test that runs under `-race`.
- `//nolint` carries the linter and a reason, and is rare.

## Checks

Run `scripts/check.sh` before committing: Rust tests and clippy, the library build, `gofmt`, `go vet`, `golangci-lint run`, `go test -race -cover`, `govulncheck`, and the `checkptr` tests of the unsafe packages. Install the tools once with `go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@latest golang.org/x/vuln/cmd/govulncheck@latest`. `CGO_ENABLED=1` is required for anything that imports `internal/typst`.

## User data

`~/Documents/Thesio` holds real user documents. Tests and scripted runs use a temporary vault through `THESIO_HOME`. Stop only processes running this checkout's own binary.
