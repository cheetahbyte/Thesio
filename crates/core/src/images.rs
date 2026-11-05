//! Images taken into the vault: what Typst can show, the name a copy takes, and
//! the figure that shows it. A copy goes in `images/` beside its document (`Store::add_image`).

use crate::quote::block_glue;
use crate::typst_source::{callee, quoted, walk};
use anyhow::{Context, Result};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use typst::syntax::{LinkedNode, Source, SyntaxKind};
use typst::visualize::{ExchangeFormat, ImageFormat, RasterFormat, VectorFormat};

pub const IMAGES_DIR: &str = "images";
/// What a pasted image is called, having no name of its own.
const PASTED_STEM: &str = "pasted";
const PNG: &str = "png";
const SVG: &str = "svg";
const GZIP_HEADER: [u8; 2] = [0x1f, 0x8b];

/// The extensions Typst reads as images.
const EXTENSIONS: &[&str] = &[PNG, "jpg", "jpeg", "gif", "webp", SVG, "pdf"];

/// An image on its way into the vault.
pub enum Incoming {
    File(PathBuf),
    /// Bytes from the clipboard, with no name.
    Pasted(Vec<u8>),
}

/// An image's bytes as they will be stored.
pub(crate) struct Copied {
    pub stem: String,
    pub extension: String,
    pub bytes: Vec<u8>,
}

/// What Typst would read `bytes` as. Typst calls anything gzipped an SVG, and
/// the extension is what tells it apart, which a dropped archive would get wrong.
fn detect(bytes: &[u8]) -> Option<ImageFormat> {
    ImageFormat::detect(bytes).filter(|_| !bytes.starts_with(&GZIP_HEADER))
}

/// The extensions of `format`, the usual one first.
fn extensions_of(format: ImageFormat) -> &'static [&'static str] {
    match format {
        ImageFormat::Raster(RasterFormat::Exchange(ExchangeFormat::Png)) => &[PNG],
        ImageFormat::Raster(RasterFormat::Exchange(ExchangeFormat::Jpg)) => &["jpg", "jpeg"],
        ImageFormat::Raster(RasterFormat::Exchange(ExchangeFormat::Gif)) => &["gif"],
        ImageFormat::Raster(RasterFormat::Exchange(ExchangeFormat::Webp)) => &["webp"],
        ImageFormat::Raster(RasterFormat::Pixel(_)) => &[],
        ImageFormat::Vector(VectorFormat::Svg) => &[SVG],
        ImageFormat::Vector(VectorFormat::Pdf) => &["pdf"],
    }
}

/// Whether `path` is named like an image Typst can show. The contents decide once it is read.
pub fn named_as_image(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| EXTENSIONS.contains(&extension.to_lowercase().as_str()))
}

fn read_file(path: &Path) -> Result<Copied> {
    let name = path.file_name().map_or(String::new(), |name| name.to_string_lossy().into_owned());
    let bytes = std::fs::read(path).with_context(|| format!("Could not read {name}"))?;
    let known = detect(&bytes).map(extensions_of).filter(|known| !known.is_empty());
    let Some(known) = known else { anyhow::bail!("{name} is not an image Typst can show: use PNG, JPEG, GIF, WebP, SVG, or PDF") };
    // Typst goes by the extension, so a misnamed file would not compile under its own.
    let given = path.extension().map(|extension| extension.to_string_lossy().into_owned()).filter(|given| known.contains(&given.to_lowercase().as_str()));
    let stem = path.file_stem().map_or(String::new(), |stem| stem.to_string_lossy().into_owned());
    Ok(Copied { stem, extension: given.unwrap_or(known[0].to_string()), bytes })
}

/// Bitmaps become PNG whatever the clipboard held them as. SVG is not a bitmap and stays.
fn read_pasted(bytes: &[u8]) -> Result<Copied> {
    let (extension, bytes) = match detect(bytes) {
        Some(ImageFormat::Raster(RasterFormat::Exchange(ExchangeFormat::Png))) => (PNG, bytes.to_vec()),
        Some(ImageFormat::Vector(VectorFormat::Svg)) => (SVG, bytes.to_vec()),
        _ => {
            let image = image::load_from_memory(bytes).context("Could not read the pasted image")?;
            let mut png = Cursor::new(vec![]);
            image.write_to(&mut png, image::ImageFormat::Png).context("Could not save the pasted image as PNG")?;
            (PNG, png.into_inner())
        }
    };
    Ok(Copied { stem: PASTED_STEM.to_string(), extension: extension.to_string(), bytes })
}

impl Incoming {
    pub(crate) fn read(&self) -> Result<Copied> {
        match self {
            Incoming::File(path) => read_file(path),
            Incoming::Pasted(bytes) => read_pasted(bytes),
        }
    }
}

/// Where a figure goes for a caret at `at`: past the figure around it, as the
/// caret sits in a caption right after an insert and a figure cannot go in there.
pub fn figure_place(text: &str, at: usize) -> usize {
    let source = Source::detached(text);
    let mut place = None;
    walk(&LinkedNode::new(source.root()), &mut |node| {
        let range = node.range();
        if place.is_none() && node.kind() == SyntaxKind::FuncCall && range.start < at && at < range.end && callee(node).as_deref() == Some("figure") { place = Some(range.end); }
    });
    place.unwrap_or(at)
}

/// A figure for each path, as text of its own paragraphs to replace what lies
/// between `before` and `after`, and where in it each empty caption starts.
pub fn figures(paths: &[String], before: &str, after: &str) -> (String, Vec<usize>) {
    let mut text = block_glue(before).to_string();
    let mut captions = vec![];
    for (i, path) in paths.iter().enumerate() {
        if i > 0 { text.push_str("\n\n"); }
        text.push_str(&format!("#figure(\n  image({}),\n  caption: [", quoted(path)));
        captions.push(text.len());
        text.push_str("],\n)");
    }
    text.push_str(if after.starts_with("\n\n") { "" } else if after.is_empty() || after.starts_with('\n') { "\n" } else { "\n\n" });
    (text, captions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::Compiler;
    use crate::store::Store;
    use image::{DynamicImage, RgbImage};

    const SVG_FILE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    const PDF_FILE: &str = "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 72 72]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n";

    fn vault(name: &str) -> Store {
        let root = std::env::temp_dir().join(format!("thesio-images-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Store::open_at(root).unwrap().0
    }

    fn shaded(format: image::ImageFormat, shade: u8) -> Vec<u8> {
        let mut out = Cursor::new(vec![]);
        DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 4, image::Rgb([shade; 3]))).write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    fn encoded(format: image::ImageFormat) -> Vec<u8> { shaded(format, 0) }

    /// An outside file, in a folder of its own for each test.
    fn outside(store: &Store, name: &str, bytes: &[u8]) -> Incoming {
        let path = store.root().with_extension("outside").join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        Incoming::File(path)
    }

    #[test]
    fn typst_shows_every_format_a_drop_accepts() {
        let mut store = vault("formats");
        let files: Vec<(&str, Vec<u8>)> = vec![
            ("a.png", encoded(image::ImageFormat::Png)),
            ("b.jpg", encoded(image::ImageFormat::Jpeg)),
            ("c.jpeg", encoded(image::ImageFormat::Jpeg)),
            ("d.gif", encoded(image::ImageFormat::Gif)),
            ("e.webp", encoded(image::ImageFormat::WebP)),
            ("f.svg", SVG_FILE.into()),
            ("g.pdf", PDF_FILE.into()),
        ];
        assert_eq!(files.len(), EXTENSIONS.len());
        let mut paths = vec![];
        for (name, bytes) in &files {
            assert!(named_as_image(Path::new(name)));
            paths.push(store.add_image("main.typ", &outside(&store, name, bytes)).unwrap());
        }
        let (text, _) = figures(&paths, "", "");
        store.save("main.typ", &text).unwrap();
        let compilation = Compiler::new(store.root()).compile("main.typ").unwrap();
        let errors: Vec<&String> = compilation.diagnostics.iter().filter(|d| d.error).map(|d| &d.message).collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(compilation.document.is_some());
    }

    #[test]
    fn a_format_typst_cannot_show_is_refused_and_leaves_nothing() {
        let mut store = vault("refused");
        for (name, bytes) in [("a.bmp", encoded(image::ImageFormat::Bmp)), ("b.tiff", encoded(image::ImageFormat::Tiff)), ("c.txt", b"text".to_vec()), ("d.svg.gz", vec![0x1f, 0x8b, 8, 0])] {
            let error = store.add_image("main.typ", &outside(&store, name, &bytes)).unwrap_err().to_string();
            assert!(error.contains(name), "{error}");
        }
        assert!(!store.root().join(IMAGES_DIR).exists());
    }

    #[test]
    fn a_copy_goes_beside_its_document_and_the_path_is_relative_to_it() {
        let mut store = vault("beside");
        let path = store.add_image("thesis/chapters/one.typ", &outside(&store, "plot.png", &encoded(image::ImageFormat::Png))).unwrap();
        assert_eq!(path, "images/plot.png");
        assert!(store.root().join("thesis/chapters/images/plot.png").is_file());
    }

    #[test]
    fn the_same_content_is_reused_and_other_content_never_replaces_a_file() {
        let mut store = vault("names");
        let (one, two) = (shaded(image::ImageFormat::Png, 0), shaded(image::ImageFormat::Png, 200));
        let mut add = |bytes: &[u8]| { let image = outside(&store, "plot.png", bytes); store.add_image("main.typ", &image).unwrap() };
        assert_eq!(add(&one), "images/plot.png");
        assert_eq!(add(&one), "images/plot.png");
        assert_eq!(add(&two), "images/plot 1.png");
        // The numbered name counts too: a second drop makes no third copy.
        assert_eq!(add(&two), "images/plot 1.png");
        assert_eq!(std::fs::read(store.root().join("images/plot.png")).unwrap(), one);
        assert_eq!(std::fs::read(store.root().join("images/plot 1.png")).unwrap(), two);
        assert_eq!(std::fs::read_dir(store.root().join(IMAGES_DIR)).unwrap().count(), 2);
    }

    #[test]
    fn a_copy_is_named_for_what_it_holds() {
        let mut store = vault("misnamed");
        let (gif, jpeg) = (encoded(image::ImageFormat::Gif), encoded(image::ImageFormat::Jpeg));
        assert_eq!(store.add_image("main.typ", &outside(&store, "plot.png", &gif)).unwrap(), "images/plot.gif");
        assert_eq!(store.add_image("main.typ", &outside(&store, "Photo.JPG", &jpeg)).unwrap(), "images/Photo.JPG");
    }

    #[test]
    fn a_folder_with_the_image_name_is_passed_over() {
        let mut store = vault("folder");
        std::fs::create_dir_all(store.root().join("images/plot.png")).unwrap();
        assert_eq!(store.add_image("main.typ", &outside(&store, "plot.png", &encoded(image::ImageFormat::Png))).unwrap(), "images/plot 1.png");
    }

    #[test]
    fn a_name_cannot_write_outside_the_folder() {
        let mut store = vault("name");
        let path = store.add_image("main.typ", &outside(&store, "..a:b.png", &encoded(image::ImageFormat::Png))).unwrap();
        assert_eq!(path, "images/a-b.png");
    }

    #[test]
    fn pasted_bitmaps_are_saved_as_png() {
        let mut store = vault("pasted");
        for format in [image::ImageFormat::Png, image::ImageFormat::Tiff, image::ImageFormat::Bmp, image::ImageFormat::Jpeg] {
            let path = store.add_image("main.typ", &Incoming::Pasted(encoded(format))).unwrap();
            assert_eq!(path.rsplit_once('.').unwrap().1, PNG);
            let saved = std::fs::read(store.root().join(&path)).unwrap();
            assert_eq!(image::guess_format(&saved).unwrap(), image::ImageFormat::Png);
        }
        assert_eq!(store.add_image("main.typ", &Incoming::Pasted(SVG_FILE.into())).unwrap(), "images/pasted.svg");
        assert!(store.add_image("main.typ", &Incoming::Pasted(b"nothing".to_vec())).is_err());
    }

    #[test]
    fn a_move_rewrites_the_path_and_leaves_the_image() {
        let mut store = vault("moved");
        store.create_folder(None, "other").unwrap();
        let id = store.create_from(None, "doc", "").unwrap();
        let path = store.add_image(&id, &outside(&store, "plot.png", &encoded(image::ImageFormat::Png))).unwrap();
        store.save(&id, &figures(&[path], "", "").0).unwrap();
        let moved = store.set_parent(&id, Some("other")).unwrap();
        assert!(store.load(&moved.id).unwrap().contains("image(\"../images/plot.png\")"));
        assert!(store.root().join("images/plot.png").is_file());
    }

    #[test]
    fn a_figure_stands_in_a_paragraph_of_its_own_with_each_caption_found() {
        let paths = ["images/a.png".to_string(), "images/b \"c\".png".to_string()];
        let (text, captions) = figures(&paths, "Before", "After");
        assert_eq!(text, "\n\n#figure(\n  image(\"images/a.png\"),\n  caption: [],\n)\n\n#figure(\n  image(\"images/b \\\"c\\\".png\"),\n  caption: [],\n)\n\n");
        for caption in &captions { assert_eq!(&text[caption - 1..caption + 1], "[]"); }
        assert_eq!(captions.len(), 2);
        assert_eq!(figures(&paths[..1], "Line\n\n", "\n\nNext").0, "#figure(\n  image(\"images/a.png\"),\n  caption: [],\n)");
        assert_eq!(figures(&paths[..1], "Line\n", "\nNext").0, "\n#figure(\n  image(\"images/a.png\"),\n  caption: [],\n)\n");
        assert_eq!(figures(&paths[..1], "", "").0, "#figure(\n  image(\"images/a.png\"),\n  caption: [],\n)\n");
    }

    #[test]
    fn a_figure_goes_past_the_figure_around_the_caret() {
        let text = "Before\n\n#figure(\n  image(\"a.png\"),\n  caption: [],\n)\n\nAfter #figure2()";
        let caption = text.find("[]").unwrap() + 1;
        let end = text.find(")\n\nAfter").unwrap() + 1;
        assert_eq!(figure_place(text, caption), end);
        assert_eq!(figure_place(text, 3), 3);
        assert_eq!(figure_place(text, end), end);
        assert_eq!(figure_place(text, text.len() - 2), text.len() - 2);
    }

    #[test]
    fn only_image_names_count_as_images() {
        assert!(named_as_image(Path::new("/a/B.JPG")));
        assert!(!named_as_image(Path::new("/a/notes.typ")) && !named_as_image(Path::new("/a/photo.heic")) && !named_as_image(Path::new("/a/png")));
    }
}
