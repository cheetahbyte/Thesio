package vault

import (
	"bytes"
	"errors"
	"fmt"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"

	_ "golang.org/x/image/bmp"
	_ "golang.org/x/image/tiff"
	_ "golang.org/x/image/webp"
)

// Images taken into the vault go in `images/` beside their document (Store.AddImage).
const ImagesDir = "images"

// pastedStem is what a pasted image is called, having no name of its own.
const pastedStem = "pasted"

var gzipHeader = []byte{0x1f, 0x8b}

// imageExtensions are the extensions Typst reads as images.
var imageExtensions = []string{"png", "jpg", "jpeg", "gif", "webp", "svg", "pdf"}

// DetectImage tells what Typst reads bytes as: png, jpg, gif, webp, svg or pdf.
// The app sets it to typst.ImageFormat; AddImage fails while it is nil.
var DetectImage func(data []byte) (ext string, ok bool)

// Incoming is an image on its way into the vault: a File outside the vault, or
// Pasted bytes from the clipboard, with no name.
type Incoming struct {
	File   string
	Pasted []byte
}

type copied struct {
	stem, ext string
	data      []byte
}

// detect is what Typst would read data as. Typst calls anything gzipped an SVG,
// and the extension is what tells it apart, which a dropped archive would get wrong.
func detect(data []byte) (string, bool) {
	if DetectImage == nil || bytes.HasPrefix(data, gzipHeader) { return "", false }
	return DetectImage(data)
}

// extensionsOf are the extensions of a detected format, the usual one first.
func extensionsOf(format string) []string {
	if format == "jpg" { return []string{"jpg", "jpeg"} }
	if slices.Contains(imageExtensions, format) { return []string{format} }
	return nil
}

// NamedAsImage reports whether path is named like an image Typst can show. The
// contents decide once it is read.
func NamedAsImage(path string) bool {
	_, ext, ok := splitName(filepath.Base(path))
	return ok && slices.Contains(imageExtensions, strings.ToLower(ext))
}

func readImageFile(path string) (copied, error) {
	name := filepath.Base(path)
	data, err := os.ReadFile(path)
	if err != nil { return copied{}, fmt.Errorf("Could not read %s: %w", name, err) }
	format, _ := detect(data)
	known := extensionsOf(format)
	if len(known) == 0 { return copied{}, fmt.Errorf("%s is not an image Typst can show: use PNG, JPEG, GIF, WebP, SVG, or PDF", name) }
	// Typst goes by the extension, so a misnamed file would not compile under its own.
	stem, given, ok := splitName(name)
	if !ok || !slices.Contains(known, strings.ToLower(given)) { given = known[0] }
	return copied{stem: stem, ext: given, data: data}, nil
}

// readPasted makes bitmaps PNG whatever the clipboard held them as. SVG is not a bitmap and stays.
func readPasted(data []byte) (copied, error) {
	switch format, _ := detect(data); format {
	case "png", "svg":
		return copied{stem: pastedStem, ext: format, data: data}, nil
	}
	img, _, err := image.Decode(bytes.NewReader(data))
	if err != nil { return copied{}, fmt.Errorf("Could not read the pasted image: %w", err) }
	var out bytes.Buffer
	if err := png.Encode(&out, img); err != nil { return copied{}, fmt.Errorf("Could not save the pasted image as PNG: %w", err) }
	return copied{stem: pastedStem, ext: "png", data: out.Bytes()}, nil
}

func (in Incoming) read() (copied, error) {
	if in.File != "" { return readImageFile(in.File) }
	if in.Pasted != nil { return readPasted(in.Pasted) }
	return copied{}, errors.New("No image given")
}

// Figures is a figure for each path, as text of its own paragraphs to replace
// what lies between before and after, and where in it each empty caption starts.
func Figures(paths []string, before, after string) (text string, captions []int) {
	var out strings.Builder
	out.WriteString(BlockGlue(before))
	for i, path := range paths {
		if i > 0 { out.WriteString("\n\n") }
		fmt.Fprintf(&out, "#figure(\n  image(%s),\n  caption: [", Quoted(path))
		captions = append(captions, out.Len())
		out.WriteString("],\n)")
	}
	switch {
	case strings.HasPrefix(after, "\n\n"):
	case after == "" || strings.HasPrefix(after, "\n"): out.WriteString("\n")
	default: out.WriteString("\n\n")
	}
	return out.String(), captions
}
