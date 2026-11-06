# Build the Typst library once, then run the app with live reload.
start:
    scripts/build-typst.sh
    CGO_ENABLED=1 go tool mygo dev

# An optimized app bundle and disk image in build/.
bundle:
    scripts/build-typst.sh
    CGO_ENABLED=1 go tool mygo build

check:
    scripts/check.sh
