package vault

import (
	"bytes"
	"image"
	"image/color"
	"image/gif"
	"image/jpeg"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"golang.org/x/image/bmp"
	"golang.org/x/image/tiff"
)

const svgFile = `<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>`
const pdfFile = "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n"

// webpFile has WebP's header only: x/image cannot encode WebP, and only the header is sniffed here.
const webpFile = "RIFF\x1a\x00\x00\x00WEBPVP8 "

func init() { DetectImage = sniff }

// sniff stands in for typst.ImageFormat, calling anything gzipped an SVG as Typst does.
func sniff(data []byte) (string, bool) {
	for _, h := range []struct{ prefix, ext string }{{"\x89PNG", "png"}, {"\xff\xd8\xff", "jpg"}, {"GIF8", "gif"}, {"%PDF-", "pdf"}, {"\x1f\x8b", "svg"}} {
		if bytes.HasPrefix(data, []byte(h.prefix)) {
			return h.ext, true
		}
	}
	if len(data) >= 12 && string(data[:4]) == "RIFF" && string(data[8:12]) == "WEBP" {
		return "webp", true
	}
	if text := strings.TrimSpace(string(data)); strings.HasPrefix(text, "<svg") || strings.HasPrefix(text, "<?xml") {
		return "svg", true
	}
	return "", false
}

type encoder func(io.Writer, image.Image) error

var (
	encodePNG  encoder = png.Encode
	encodeJPEG encoder = func(w io.Writer, m image.Image) error { return jpeg.Encode(w, m, nil) }
	encodeGIF  encoder = func(w io.Writer, m image.Image) error { return gif.Encode(w, m, nil) }
	encodeBMP  encoder = bmp.Encode
	encodeTIFF encoder = func(w io.Writer, m image.Image) error { return tiff.Encode(w, m, nil) }
)

func shaded(encode encoder, shade uint8) []byte {
	m := image.NewRGBA(image.Rect(0, 0, 4, 4))
	for i := range 16 {
		m.Set(i%4, i/4, color.RGBA{shade, shade, shade, 255})
	}
	var out bytes.Buffer
	if err := encode(&out, m); err != nil {
		panic(err)
	}
	return out.Bytes()
}

func encoded(encode encoder) []byte { return shaded(encode, 0) }

// outside is a file outside the vault.
func outside(t *testing.T, name string, data []byte) Incoming {
	path := filepath.Join(t.TempDir(), name)
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatal(err)
	}
	return Incoming{File: path}
}

func TestEveryFormatADropAcceptsIsCopiedUnderItsExtension(t *testing.T) {
	store := vault(t)
	files := []struct {
		name string
		data []byte
	}{
		{"a.png", encoded(encodePNG)}, {"b.jpg", encoded(encodeJPEG)}, {"c.jpeg", encoded(encodeJPEG)}, {"d.gif", encoded(encodeGIF)},
		{"e.webp", []byte(webpFile)}, {"f.svg", []byte(svgFile)}, {"g.pdf", []byte(pdfFile)},
	}
	if len(files) != len(imageExtensions) {
		t.Fatal("a format is untested")
	}
	for _, f := range files {
		if !NamedAsImage(f.name) {
			t.Fatal(f.name)
		}
		path := must(store.AddImage("main.typ", outside(t, f.name, f.data)))
		if path != "images/"+f.name {
			t.Fatal(path)
		}
		if saved := must(os.ReadFile(filepath.Join(store.Root(), path))); !bytes.Equal(saved, f.data) {
			t.Fatal(f.name)
		}
	}
}

func TestAFormatTypstCannotShowIsRefusedAndLeavesNothing(t *testing.T) {
	store := vault(t)
	for _, f := range []struct {
		name string
		data []byte
	}{{"a.bmp", encoded(encodeBMP)}, {"b.tiff", encoded(encodeTIFF)}, {"c.txt", []byte("text")}, {"d.svg.gz", []byte{0x1f, 0x8b, 8, 0}}} {
		if _, err := store.AddImage("main.typ", outside(t, f.name, f.data)); err == nil || !strings.Contains(err.Error(), f.name) {
			t.Fatal(f.name, err)
		}
	}
	if _, err := os.Stat(filepath.Join(store.Root(), ImagesDir)); err == nil {
		t.Fatal("images/ was made")
	}
}

func TestACopyGoesBesideItsDocumentAndThePathIsRelativeToIt(t *testing.T) {
	store := vault(t)
	if path := must(store.AddImage("thesis/chapters/one.typ", outside(t, "plot.png", encoded(encodePNG)))); path != "images/plot.png" {
		t.Fatal(path)
	}
	if info, err := os.Stat(filepath.Join(store.Root(), "thesis/chapters/images/plot.png")); err != nil || !info.Mode().IsRegular() {
		t.Fatal(err)
	}
}

func TestTheSameContentIsReusedAndOtherContentNeverReplacesAFile(t *testing.T) {
	store := vault(t)
	one, two := shaded(encodePNG, 0), shaded(encodePNG, 200)
	add := func(data []byte) string { return must(store.AddImage("main.typ", outside(t, "plot.png", data))) }
	for _, c := range []struct {
		data []byte
		want string
	}{{one, "images/plot.png"}, {one, "images/plot.png"}, {two, "images/plot 1.png"}, {two, "images/plot 1.png"}} {
		if got := add(c.data); got != c.want {
			t.Fatal(got)
		}
	}
	if !bytes.Equal(must(os.ReadFile(filepath.Join(store.Root(), "images/plot.png"))), one) || !bytes.Equal(must(os.ReadFile(filepath.Join(store.Root(), "images/plot 1.png"))), two) {
		t.Fatal("replaced")
	}
	if items := must(os.ReadDir(filepath.Join(store.Root(), ImagesDir))); len(items) != 2 {
		t.Fatal(items)
	}
}

func TestACopyIsNamedForWhatItHolds(t *testing.T) {
	store := vault(t)
	if path := must(store.AddImage("main.typ", outside(t, "plot.png", encoded(encodeGIF)))); path != "images/plot.gif" {
		t.Fatal(path)
	}
	if path := must(store.AddImage("main.typ", outside(t, "Photo.JPG", encoded(encodeJPEG)))); path != "images/Photo.JPG" {
		t.Fatal(path)
	}
}

func TestAFolderWithTheImageNameIsPassedOver(t *testing.T) {
	store := vault(t)
	if err := os.MkdirAll(filepath.Join(store.Root(), "images/plot.png"), 0o755); err != nil {
		t.Fatal(err)
	}
	if path := must(store.AddImage("main.typ", outside(t, "plot.png", encoded(encodePNG)))); path != "images/plot 1.png" {
		t.Fatal(path)
	}
}

func TestANameCannotWriteOutsideTheFolder(t *testing.T) {
	store := vault(t)
	if path := must(store.AddImage("main.typ", outside(t, "..a:b.png", encoded(encodePNG)))); path != "images/a-b.png" {
		t.Fatal(path)
	}
}

func TestPastedBitmapsAreSavedAsPNG(t *testing.T) {
	store := vault(t)
	for _, encode := range []encoder{encodePNG, encodeTIFF, encodeBMP, encodeJPEG} {
		path := must(store.AddImage("main.typ", Incoming{Pasted: encoded(encode)}))
		if !strings.HasSuffix(path, ".png") {
			t.Fatal(path)
		}
		if _, err := png.Decode(bytes.NewReader(must(os.ReadFile(filepath.Join(store.Root(), path))))); err != nil {
			t.Fatal(path, err)
		}
	}
	if path := must(store.AddImage("main.typ", Incoming{Pasted: []byte(svgFile)})); path != "images/pasted.svg" {
		t.Fatal(path)
	}
	if _, err := store.AddImage("main.typ", Incoming{Pasted: []byte("nothing")}); err == nil {
		t.Fatal("took nothing")
	}
}

func TestAMoveRewritesThePathAndLeavesTheImage(t *testing.T) {
	store := vault(t)
	must(store.CreateFolder("", "other"))
	id := must(store.CreateFrom("", "doc", ""))
	path := must(store.AddImage(id, outside(t, "plot.png", encoded(encodePNG))))
	text, _ := Figures([]string{path}, "", "")
	if err := store.Save(id, text); err != nil {
		t.Fatal(err)
	}
	moved := must(store.SetParent(id, "other"))
	if text := must(store.Load(moved.ID)); !strings.Contains(text, `image("../images/plot.png")`) {
		t.Fatal(text)
	}
	if _, err := os.Stat(filepath.Join(store.Root(), "images/plot.png")); err != nil {
		t.Fatal(err)
	}
}

func TestAFigureStandsInAParagraphOfItsOwnWithEachCaptionFound(t *testing.T) {
	paths := []string{"images/a.png", `images/b "c".png`}
	text, captions := Figures(paths, "Before", "After")
	if text != "\n\n#figure(\n  image(\"images/a.png\"),\n  caption: [],\n)\n\n#figure(\n  image(\"images/b \\\"c\\\".png\"),\n  caption: [],\n)\n\n" {
		t.Fatalf("%q", text)
	}
	if len(captions) != 2 {
		t.Fatal(captions)
	}
	for _, c := range captions {
		if text[c-1:c+1] != "[]" {
			t.Fatal(c)
		}
	}
	for _, c := range []struct{ before, after, want string }{
		{"Line\n\n", "\n\nNext", "#figure(\n  image(\"images/a.png\"),\n  caption: [],\n)"},
		{"Line\n", "\nNext", "\n#figure(\n  image(\"images/a.png\"),\n  caption: [],\n)\n"},
		{"", "", "#figure(\n  image(\"images/a.png\"),\n  caption: [],\n)\n"},
	} {
		if got, _ := Figures(paths[:1], c.before, c.after); got != c.want {
			t.Fatalf("%q", got)
		}
	}
}

func TestOnlyImageNamesCountAsImages(t *testing.T) {
	if !NamedAsImage("/a/B.JPG") {
		t.Fatal("B.JPG")
	}
	if slices.ContainsFunc([]string{"/a/notes.typ", "/a/photo.heic", "/a/png"}, NamedAsImage) {
		t.Fatal("not images")
	}
}
