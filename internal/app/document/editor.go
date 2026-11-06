package document

import (
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
)

var brackets = []editor.BracketPair{{Open: "(", Close: ")"}, {Open: "[", Close: "]"}, {Open: "{", Close: "}"}}

var typstLanguage = &editor.Language{
	Name:         "Typst",
	Extensions:   []string{".typ"},
	LineComment:  "//",
	BlockComment: [2]string{"/*", "*/"},
	Brackets:     brackets,
	AutoClose:    brackets,
	TabSize:      2,
	Highlighter: func(text editor.Snapshot, _ func()) editor.Highlighter {
		return newHighlighter(text)
	},
}

// appKeys are the app's shortcuts, which the editor leaves alone.
var appKeys = []editor.Binding{
	{Mods: ui.Cmd, Key: ui.KeyJ, Command: editor.NoCommand},
	{Mods: ui.Cmd, Key: ui.KeyN, Command: editor.NoCommand},
	{Mods: ui.Cmd | ui.Shift, Key: ui.KeyC, Command: editor.NoCommand},
	{Mods: ui.Cmd | ui.Shift, Key: ui.KeyN, Command: editor.NoCommand},
	{Mods: ui.Cmd | ui.Shift, Key: ui.KeyI, Command: editor.NoCommand},
	{Mods: ui.Cmd, Key: ui.KeyBackslash, Command: editor.NoCommand},
	{Mods: ui.Cmd, Key: ui.KeyBracketRight, Command: editor.NoCommand},
}

func newEditor(d *Open) *editor.State {
	return &editor.State{
		Buffer:       d.buf,
		Font:         ui.Font{Family: theme.Mono, Size: 13},
		SoftWrap:     true,
		Placeholder:  "Start writing…",
		Label:        "Document",
		Bindings:     appKeys,
		OnCompletion: d.complete,
		OnHover:      d.hover,
		OnDefinition: d.followAt,
		OnInput:      d.input,
		RenderCompletions: func(c *ui.Context, panel ui.Element, m *editor.CompletionMenu) {
			d.menuAt = d.frame
			editor.DefaultCompletions(c, panel, m)
		},
	}
}

// editorTheme is the editor's look of Typst: headings and strong text bold,
// emphasis italic.
func editorTheme(t *ui.Theme) *editor.Theme {
	th := editor.DefaultTheme(t)
	th.Tokens[editor.TokenHeading].Bold = true
	th.Tokens[editor.TokenStrong].Bold = true
	th.Tokens[editor.TokenEmphasis].Italic = true
	return th
}

// input sees each input before the editor: what is typed, pastes of images,
// and Tab through placeholders.
func (d *Open) input(ev ui.InputEvent) bool {
	switch {
	case ev.Kind == ui.InputText:
		d.typed = ev.Text
	case ev.Kind == ui.InputCommand && ev.Text == "paste", ev.Kind == ui.InputKeyDown && ev.Mods == ui.Cmd && ev.Key == ui.KeyV:
		return d.pasteImages()
	case ev.Kind == ui.InputKeyDown && ev.Key == ui.KeyTab && ev.Mods == 0 && len(d.stops) > 0 && d.menuAt < d.frame:
		d.nextStop()
		return true
	}
	return false
}

// nextStop selects the next placeholder.
func (d *Open) nextStop() {
	n := d.buf.Len()
	stop := d.stops[0]
	d.stops = d.stops[1:]
	d.selectRange(max(0, n-stop[0]), max(0, n-stop[1]))
}

// EditorView is the editor alone, as the paper panel shows notes.
func (d *Open) EditorView(c *ui.Context) {
	d.sync()
	// The theme may change in place on an appearance switch, so compare its background too.
	if t := c.Theme(); t != d.theme || d.ed.Theme == nil || d.ed.Theme.Background != t.Background {
		d.theme = t
		d.ed.Theme = editorTheme(t)
	}
	el := editor.View(c, d.ed).Fill().Bind(&d.focus)
	if files := el.DroppedFiles(); files != nil {
		d.dropped(files)
	}
}

// View is the editor and the preview side by side, diagnostics under the
// preview; notes, which have no preview, show the editor alone.
func (d *Open) View(c *ui.Context) {
	if !d.withPreview {
		d.EditorView(c)
		return
	}
	ui.SplitPercent(c, &d.split, func() { d.EditorView(c) }, func() { d.previewView(c) }).Fill()
}
