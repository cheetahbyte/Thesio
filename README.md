# Thesio

A local-first workspace for academic research and writing. See [PRODUCT.md](PRODUCT.md) for the pitch.

Thesio edits plain `.typ` files in a folder you own, with a live preview compiled by [Typst](https://typst.app/). Notes and papers share one vault, so `#include` and citations work across everything.

The app is Go with [MyGo](https://mygo.egoist.dev) native UI. Typst, typst-ide and Hayagriva are a Rust static library (`typst/`) behind a C ABI, linked with cgo.

## Build

Requires Go 1.27 or later, a Rust toolchain (`typst/rust-toolchain.toml` pins the version), and on macOS the Xcode command line tools for cgo.

```sh
scripts/build-typst.sh            # the Typst library, once and after changes to typst/
CGO_ENABLED=1 go run .            # the app on ~/Documents/Thesio
CGO_ENABLED=1 go tool mygo dev    # with live reload
CGO_ENABLED=1 go tool mygo build  # Thesio.app and a disk image in build/
```

With [just](https://github.com/casey/just): `just start`, `just bundle`, `just check`.

The default vault is `~/Documents/Thesio`. Set `THESIO_HOME` to use another folder, as in `THESIO_HOME=$(mktemp -d) go run .`; `testdata/sample-vault` is a small vault to try.

`scripts/build-typst.sh` builds the library for this machine. For a universal macOS app, build both architectures first:

```sh
scripts/build-typst.sh aarch64-apple-darwin x86_64-apple-darwin
CGO_ENABLED=1 go tool mygo build -platform darwin/universal
```

## Release

Pushing a tag `vX.Y.Z` runs `.github/workflows/release.yml`. It builds the universal macOS app, Linux x64 and arm64, and Windows x64, each on its own runner, and uploads them with signed updates to a draft release of the tag.

1. Set `version` in `mygo.json` to `X.Y.Z`. The workflow fails when the tag does not match it.
2. Move the `Unreleased` entries of `CHANGELOG.md` under `## X.Y.Z`. The update window shows that section.
3. Commit, then tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
4. Review the draft release on GitHub and publish it. Installed apps then update to it.

The repository needs the secret `MYGO_UPDATER_PRIVATE_KEY`, the private key matching `updates.publicKey` in `mygo.json`:

```sh
gh secret set MYGO_UPDATER_PRIVATE_KEY < ~/Library/Application\ Support/mygo/update-keys/mygo-update.key
```

The macOS app is signed with the Developer ID in `mygo.json` and notarized. The workflow needs these secrets: `MACOS_CERTIFICATE` (the "Developer ID Application" certificate with its key, exported from Keychain Access as a `.p12`, base64-encoded), `MACOS_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_TEAM_ID` (`ZMF32TXKBT`) and `APPLE_APP_PASSWORD` (an app-specific password from account.apple.com):

```sh
base64 -i certificate.p12 | gh secret set MACOS_CERTIFICATE
gh secret set MACOS_CERTIFICATE_PASSWORD
gh secret set APPLE_ID
gh secret set APPLE_TEAM_ID
gh secret set APPLE_APP_PASSWORD
```

A local `go tool mygo build` signs with the same identity and notarizes with the keychain profile `notary`: create it once with `xcrun notarytool store-credentials notary --apple-id … --team-id ZMF32TXKBT --password …`, or pass `-skip-notarize`. Windows builds are unsigned; SmartScreen warns about the installer. To sign them, add `WINDOWS_CERTIFICATE` and `WINDOWS_CERTIFICATE_PASSWORD`, enable the commented step in the workflow and set `windows.certificate` in `mygo.json` as [MyGo's guide](https://github.com/egoist/mygo/blob/main/docs/github-actions.md) describes.

## Docs

Conventions are in [AGENTS.md](AGENTS.md). The architecture and vault format are described in [docs/](docs/).

## License

MIT, see [LICENSE](LICENSE). Third-party code and assets are listed in [THIRD_PARTY.md](THIRD_PARTY.md).
