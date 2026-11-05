start:
    cargo run -p thesio

# Build a release and replace /Applications/Thesio.app.
install:
    mbx build --release --locked
    scripts/bundle.sh
    rm -rf /Applications/Thesio.app
    ditto dist-preview/Thesio.app /Applications/Thesio.app
