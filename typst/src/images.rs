//! What Typst reads image bytes as, and where a figure goes for a caret.

use crate::typst_source::{callee, walk};
use typst::syntax::{LinkedNode, Source, SyntaxKind};
use typst::visualize::{ExchangeFormat, ImageFormat, RasterFormat, VectorFormat};

const GZIP_HEADER: [u8; 2] = [0x1f, 0x8b];

/// What Typst would read `bytes` as. Typst calls anything gzipped an SVG, and
/// the extension is what tells it apart, which a dropped archive would get wrong.
fn detect(bytes: &[u8]) -> Option<ImageFormat> {
    ImageFormat::detect(bytes).filter(|_| !bytes.starts_with(&GZIP_HEADER))
}

/// The extensions of `format`, the usual one first.
fn extensions_of(format: ImageFormat) -> &'static [&'static str] {
    match format {
        ImageFormat::Raster(RasterFormat::Exchange(ExchangeFormat::Png)) => &["png"],
        ImageFormat::Raster(RasterFormat::Exchange(ExchangeFormat::Jpg)) => &["jpg", "jpeg"],
        ImageFormat::Raster(RasterFormat::Exchange(ExchangeFormat::Gif)) => &["gif"],
        ImageFormat::Raster(RasterFormat::Exchange(ExchangeFormat::Webp)) => &["webp"],
        ImageFormat::Raster(RasterFormat::Pixel(_)) => &[],
        ImageFormat::Vector(VectorFormat::Svg) => &["svg"],
        ImageFormat::Vector(VectorFormat::Pdf) => &["pdf"],
    }
}

/// The usual extension of what Typst reads `bytes` as, unless it cannot show them.
pub fn image_format(bytes: &[u8]) -> Option<&'static str> {
    detect(bytes).and_then(|format| extensions_of(format).first().copied())
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

#[cfg(test)]
mod tests {
    use super::*;

    const SVG_FILE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#;
    const PDF_FILE: &str = "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n";
    const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    #[test]
    fn bytes_are_named_for_what_typst_reads_them_as() {
        assert_eq!(image_format(PNG_HEADER), Some("png"));
        assert_eq!(image_format(b"\xff\xd8\xff\xe0\0\x10JFIF"), Some("jpg"));
        assert_eq!(image_format(b"GIF89a"), Some("gif"));
        assert_eq!(image_format(SVG_FILE.as_bytes()), Some("svg"));
        assert_eq!(image_format(PDF_FILE.as_bytes()), Some("pdf"));
        assert_eq!(image_format(b"text"), None);
        assert_eq!(image_format(&[0x1f, 0x8b, 8, 0]), None, "a gzipped file");
        assert_eq!(image_format(b"BM\0\0"), None, "Typst shows no BMP");
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
}
