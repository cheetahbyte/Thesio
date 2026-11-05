start:
    cargo run -p thesio

# Build an optimized release and replace /Applications/Thesio.app.
install:
    CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_STRIP=symbols mbx build --release --locked
    scripts/bundle.sh
    rm -rf /Applications/Thesio.app
    ditto dist-preview/Thesio.app /Applications/Thesio.app
