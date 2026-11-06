package app

import (
	"cmp"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/cheetahbyte/thesio/internal/app/host"
	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/cheetahbyte/thesio/internal/pdfkit"
	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/cheetahbyte/thesio/internal/vault"
	"github.com/egoist/mygo/ui"
)

const (
	// searchRecent is how many documents Recently viewed lists.
	searchRecent = 20
	// summaryChars is the body text kept for a hit's summary.
	summaryChars = 160
	// summarySep joins the summary's blocks.
	summarySep = "   "
)

type searchState struct {
	open     bool
	query    string
	selected int
	list     ui.ListState
}

type hitKind int

const (
	hitDoc hitKind = iota
	hitSource
	hitHighlight
)

// hit is a search result: a document, a source, or a paper's highlight.
type hit struct {
	kind      hitKind
	entry     vault.Entry
	row       library.Row
	paper     string
	highlight pdfkit.Highlight
}

func (ws *Workspace) openSearch() {
	ws.closeOverlays()
	ws.search = searchState{open: true}
}

func (ws *Workspace) toggleSearch() {
	if ws.search.open {
		ws.search.open = false
		return
	}
	ws.openSearch()
}

// findFold is the first case-insensitive match of needle (lower case) in hay, in hay's own offsets.
func findFold(hay, needle string) (int, int, bool) {
	if needle == "" {
		return 0, 0, false
	}
	lower := strings.ToLower(hay)
	if len(lower) == len(hay) {
		i := strings.Index(lower, needle)
		return i, i + len(needle), i >= 0
	}
	i := strings.Index(hay, needle)
	return i, i + len(needle), i >= 0
}

func findAll(hay, needle string) [][2]int {
	var out [][2]int
	for at := 0; at < len(hay); {
		s, e, ok := findFold(hay[at:], needle)
		if !ok {
			break
		}
		out = append(out, [2]int{at + s, at + e})
		at += e
	}
	return out
}

// snippet is body from the block holding the first match, with about 16
// characters before it and the following blocks after it, and where q matches in it.
func snippet(body, q string) (string, [][2]int, bool) {
	start, _, ok := findFold(body, q)
	if !ok {
		return "", nil, false
	}
	lineStart := strings.LastIndexByte(body[:start], '\n') + 1
	cut := lineStart
	if n := utf8.RuneCountInString(body[lineStart:start]); n > 16 {
		cut = start
		for range 16 {
			_, size := utf8.DecodeLastRuneInString(body[lineStart:cut])
			cut -= size
		}
	}
	var text strings.Builder
	if cut > lineStart {
		text.WriteString("…")
	}
	chars := 0
	for line := range strings.Lines(body[cut:]) {
		line = strings.TrimRight(line, " \t\r\n")
		if strings.TrimSpace(line) == "" {
			continue
		}
		if text.Len() > 0 && text.String() != "…" {
			text.WriteString(summarySep)
		}
		for _, r := range line {
			text.WriteRune(r)
			if chars++; chars >= summaryChars {
				break
			}
		}
		if chars >= summaryChars {
			break
		}
	}
	return text.String(), findAll(text.String(), q), true
}

// searchResults are documents (titles first), then sources, then highlights;
// with no query, Recently viewed.
func (ws *Workspace) searchResults() []hit {
	q := strings.ToLower(strings.TrimSpace(ws.search.query))
	var hits []hit
	if q == "" {
		var recent []vault.Entry
		for _, e := range ws.entries {
			if e.Typst {
				recent = append(recent, e)
			}
		}
		slices.SortStableFunc(recent, func(a, b vault.Entry) int {
			return cmp.Compare(max(b.Meta.Opened, b.Modified), max(a.Meta.Opened, a.Modified))
		})
		for _, e := range recent[:min(len(recent), searchRecent)] {
			hits = append(hits, hit{kind: hitDoc, entry: e})
		}
		return hits
	}
	var body []hit
	for _, e := range ws.entries {
		if e.Folder {
			continue
		}
		if strings.Contains(strings.ToLower(e.Title), q) {
			hits = append(hits, hit{kind: hitDoc, entry: e})
		} else if strings.Contains(strings.ToLower(typst.WithoutBibliographyBlock(e.Body)), q) {
			body = append(body, hit{kind: hitDoc, entry: e})
		}
	}
	hits = append(hits, body...)
	for _, r := range ws.libraryRows() {
		if r.Matches(q) {
			hits = append(hits, hit{kind: hitSource, row: r})
		}
	}
	for _, id := range ws.papers.PaperIDs() {
		for _, h := range ws.papers.Highlights(id) {
			if strings.Contains(strings.ToLower(h.Text), q) || strings.Contains(strings.ToLower(h.Comment), q) {
				hits = append(hits, hit{kind: hitHighlight, paper: id, highlight: h})
			}
		}
	}
	return hits
}

func (ws *Workspace) pickSearch(h hit) {
	ws.search.open = false
	switch h.kind {
	case hitDoc:
		ws.Open(h.entry.ID)
		if ws.doc != nil && ws.doc.ID() == h.entry.ID {
			ws.doc.Focus()
		}
	case hitSource:
		ws.ShowView(host.ViewLibrary)
		ws.cites.OpenEntry(h.row.Key)
	case hitHighlight:
		ws.OpenPaper(h.paper)
	}
}

// searchDialog is the search over the window: ↑↓ select, Enter opens, Esc closes.
func (ws *Workspace) searchDialog(c *ui.Context) {
	if !ws.search.open {
		return
	}
	t := c.Theme()
	results := ws.searchResults()
	q := strings.ToLower(strings.TrimSpace(ws.search.query))
	_, height := c.Size()
	ws.search.selected = min(ws.search.selected, max(len(results)-1, 0))
	ui.DialogBase(c, &ws.search.open, func(backdrop, panel ui.Element) {
		backdrop.Background(ui.RGBA(0, 0, 0, 0.2))
		theme.Elevate(panel.Width(540).Height(max(min(630, height-120), 200)).Radius(14).Background(t.Background).Clip(), t).Label("Search")
		ui.Row(c).Height(46).Padding(0, 8, 0, 12).AlignItems(ui.Center).Children(func() {
			// The panel is the field, as Spotlight's: no box or ring of its own.
			field := ui.SearchField(c, &ws.search.query).Placeholder("Search").AutoFocus().Grow(1).FontSize(15).
				Background(ui.Transparent).BorderWidth(0).FocusRing(false)
			if field.Changed() {
				ws.search.selected = 0
				ws.search.list.ScrollTo(0, ui.Start)
			}
			field.OnTextKey(func(k ui.TextKey) bool {
				n := len(ws.searchResults())
				switch {
				case k.Key == ui.KeyUp && n > 0:
					ws.search.selected = (ws.search.selected - 1 + n) % n
				case k.Key == ui.KeyDown && n > 0:
					ws.search.selected = (ws.search.selected + 1) % n
				case k.Key == ui.KeyEnter && ws.search.selected < n:
					ws.pickSearch(ws.searchResults()[ws.search.selected])
					return true
				default:
					return false
				}
				ws.search.list.ScrollIntoView(ws.search.selected)
				return true
			})
		})
		ui.Box(c).Height(1).Background(theme.Hairline(t))
		if q == "" && len(results) > 0 {
			ui.Text(c, "Recently viewed").FontSize(11).FontWeight(600).TextColor(theme.Caption(t)).Padding(12, 20, 2, 20)
		}
		ui.List(c, &ws.search.list, len(results), func(i int) {
			ws.searchRow(c, results[i], i, q)
		}).Grow(1).Padding(6, 8).Children(func() {
			if len(results) == 0 && q != "" {
				ui.Text(c, "No results. Try another search term.").FontSize(13).TextColor(theme.Caption(t)).Padding(8, 12)
			}
		})
		ui.Row(c).Height(36).PaddingX(20).AlignItems(ui.Center).Background(theme.Group(t)).BorderWidth(1, 0, 0, 0).BorderColor(theme.Hairline(t)).Children(func() {
			ui.Text(c, "Use ↑↓ to select, Enter to open").FontSize(12).TextColor(theme.Caption(t)).SingleLine()
		})
	})
}

func (ws *Workspace) searchRow(c *ui.Context, h hit, i int, q string) {
	t := c.Theme()
	row := ui.ButtonBase(c).Padding(8, 8, 8, 12).Gap(10).Radius(6).AlignItems(ui.Center)
	if row.Hovered() && ws.search.selected != i {
		ws.search.selected = i
	}
	if ws.search.selected == i {
		row.Background(theme.Selected(t))
	}
	if row.Clicked() {
		ws.after(func() { ws.pickSearch(h) })
	}
	var glyph theme.Icon
	var title, detail, summary string
	var summaryHits [][2]int
	switch h.kind {
	case hitDoc:
		glyph, title = theme.Icons.Doc, vault.DisplayTitle(h.entry.Title)
		summary, summaryHits, _ = snippet(typst.WithoutBibliographyBlock(h.entry.Body), q)
		now := vault.Now()
		var parts []string
		if h.entry.Meta.Opened > 0 {
			parts = append(parts, "Opened "+formatVisit(h.entry.Meta.Opened, now))
		}
		detail = strings.Join(append(parts, "Modified "+formatVisit(h.entry.Modified, now)), " · ")
	case hitSource:
		glyph, title = theme.Icons.Library, h.row.Label()
		var parts []string
		for _, p := range []string{"Source", h.row.Authors, h.row.Year, h.row.Key} {
			if p != "" {
				parts = append(parts, p)
			}
		}
		detail = strings.Join(parts, " · ")
	case hitHighlight:
		glyph, title = theme.Icons.Quote, ws.papers.PaperTitle(h.paper)
		text := h.highlight.Text
		if h.highlight.Comment != "" {
			text += "\n" + h.highlight.Comment
		}
		summary, summaryHits, _ = snippet(text, q)
		offset := 0
		if e, ok := ws.Entry(h.paper); ok && e.Meta.PageOffset != nil {
			offset = *e.Meta.PageOffset
		}
		place := "before the first page"
		if page := library.PrintedPage("", h.highlight.Page, offset); page != "" {
			place = "page " + page
		}
		detail = "Highlight · " + place
	}
	row.Label(title)
	row.Children(func() {
		ui.Box(c).Size(24, 24).Center().Children(func() { theme.Glyph(c, glyph, 18, t.TextMuted) })
		ui.Column(c).Grow(1).Basis(0).Children(func() {
			ui.RichText(c, marked(title, findAll(title, q), ui.Span{Size: 13, Weight: 600}, t.Accent)...).SingleLine().Ellipsis("…")
			if summary != "" {
				ui.RichText(c, marked(summary, summaryHits, ui.Span{Size: 12, Color: theme.Caption(t)}, t.Accent)...).MaxLines(2).Ellipsis("…")
			}
			theme.Tabular(ui.Text(c, detail).FontSize(12).TextColor(theme.Caption(t)).SingleLine().Ellipsis("…"))
		})
	})
}

// marked is text as spans of style, the ranges in hits colored.
func marked(text string, hits [][2]int, style ui.Span, color ui.Color) []ui.Span {
	var spans []ui.Span
	at := 0
	for _, h := range hits {
		if h[0] > at {
			s := style
			s.Text = text[at:h[0]]
			spans = append(spans, s)
		}
		s := style
		s.Text, s.Color = text[h[0]:h[1]], color
		spans = append(spans, s)
		at = h[1]
	}
	if at < len(text) {
		s := style
		s.Text = text[at:]
		spans = append(spans, s)
	}
	return spans
}
