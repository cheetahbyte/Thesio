package papers

import (
	"net/url"
	"strconv"

	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/pdfkit"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

const panelWidth = 340

// ensureWebView makes the viewer's web view, once per window.
func (s *State) ensureWebView() {
	if s.web != nil && !s.web.IsDestroyed() {
		return
	}
	s.web, s.shown = nil, ""
	w := s.h.Window()
	if w == nil {
		return
	}
	web, err := w.NewWebView(mygo.WebViewOptions{})
	if err != nil {
		s.h.ToastError("Could not show the paper: " + err.Error())
		return
	}
	s.web = web
}

// show loads the current paper into the web view if it is not there yet,
// and goes to page (zero-based) when it is not negative.
func (s *State) show(page int) {
	if s.web == nil || s.current == "" {
		return
	}
	path, err := s.h.Store().Path(s.current)
	if err != nil {
		return
	}
	// LoadFile grants the web view read access to the folder, which a sandboxed build needs.
	if s.shown != path {
		if err := s.web.Page().LoadFile(path); err != nil {
			s.h.ToastError("Could not show the paper: " + err.Error())
			return
		}
		s.shown, s.version = path, 0
	}
	if page >= 0 {
		s.web.Page().LoadURL(pageURL(path, page, s.version))
	}
}

// reload shows the current paper again after its file changed, at page.
func (s *State) reload(page int) {
	if e := s.cache[s.current]; e != nil {
		e.read = false
	}
	if s.web == nil || s.shown == "" {
		s.show(page)
		return
	}
	// A new query makes WebKit read the file again; a new fragment alone would not.
	s.version++
	s.web.Page().LoadURL(pageURL(s.shown, max(page, 0), s.version))
}

// pageURL is the file at a zero-based page, as WebKit's PDF viewer reads `#page=`.
func pageURL(path string, page, version int) string {
	u := url.URL{Scheme: "file", Path: path, Fragment: "page=" + strconv.Itoa(page+1)}
	if version > 0 {
		u.RawQuery = "v=" + strconv.Itoa(version)
	}
	return u.String()
}

func (s *State) openExternally() {
	if path, err := s.h.Store().Path(s.current); err == nil {
		s.h.OpenExternally(path)
	}
}

// PaperView is the Paper view: the PDF in a web view, and the notes and highlights panel.
func (s *State) PaperView(c *ui.Context) {
	if s.current == "" {
		return
	}
	t := c.Theme()
	ui.Column(c).Fill().AlignItems(ui.Stretch).Children(func() {
		s.header(c, s.PaperTitle(s.current), 0, func() {
			ui.Toolbar(c, func() {
				if toolButton(c, theme.Icons.Highlight, "Highlight Selection").Clicked() {
					s.highlightSelection()
				}
				panel := toolButton(c, theme.Icons.Panel, "Notes and Highlights")
				if !s.panelHidden {
					panel.Background(theme.Selected(t))
				}
				if panel.Clicked() {
					s.panelHidden = !s.panelHidden
				}
				if toolButton(c, theme.Icons.External, "Open in Default App").Clicked() {
					s.openExternally()
				}
			}).Label("Paper").Gap(2)
		})
		ui.Row(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
			if s.web != nil {
				ui.WebView(c, s.web).Grow(1).MinWidth(0)
			} else {
				ui.Column(c).Grow(1).MinWidth(0).Center().Padding(24).Children(func() {
					ui.Text(c, "This PDF could not be shown here. Try opening it in your default app.").TextColor(theme.Caption(t)).TextAlign(ui.Center)
				})
			}
			if !s.panelHidden {
				s.panel(c)
			}
		})
	})
}

func toolButton(c *ui.Context, icon theme.Icon, label string) ui.Element {
	return theme.IconButton(c, icon, label)
}

func (s *State) panel(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Width(panelWidth).Shrink(0).AlignItems(ui.Stretch).BorderStartWidth(1).BorderColor(theme.Hairline(t)).Background(t.Background).Children(func() {
		ui.Row(c).Padding(8, 16, 12).Justify(ui.Center).Children(func() {
			ui.Segmented(c, &s.tab, "Notes", "Highlights").Label("Panel")
		})
		switch s.tab {
		case tabNotes:
			ui.Column(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
				if s.notes != nil {
					s.notes.doc.EditorView(c)
				}
			})
		default:
			s.highlightsList(c)
		}
	})
}

func (s *State) highlightsList(c *ui.Context) {
	list := s.Highlights(s.current)
	if len(list) == 0 {
		theme.EmptyState(c, theme.Icons.Highlight, "No highlights yet", "Select text in the paper, copy it, then press the highlighter.", nil)
		return
	}
	ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).Padding(0, 16, 16).Gap(8).AlignItems(ui.Stretch).Children(func() {
			for _, h := range list {
				s.highlightCard(c.Key(keyOf(h)), h)
			}
		})
	})
}

func (s *State) highlightCard(c *ui.Context, h pdfkit.Highlight) {
	t := c.Theme()
	page := s.highlightPage(s.current, h)
	if page == "" {
		page = "Before the first page"
	} else {
		page = "Page " + page
	}
	ui.Column(c).Padding(10, 12).Gap(4).Radius(10).Background(theme.Group(t)).AlignItems(ui.Stretch).Children(func() {
		ui.Row(c).Gap(6).Children(func() {
			theme.Tabular(ui.Text(c, page).FontSize(12).TextColor(theme.Caption(t)).Grow(1))
			if ui.ButtonBase(c).Label("Insert as Quote").Children(func() {
				ui.Text(c, "Insert as Quote").FontSize(12).TextColor(t.Accent)
			}).Clicked() {
				s.openQuote(h)
			}
			del := ui.ButtonBase(c).Label("Delete Highlight").Tooltip("Delete Highlight").Size(22, 22).Margin(-3, -4, -3, 0).Radius(6).Center()
			if del.Hovered() {
				del.Background(theme.Hover(t))
			}
			if del.Children(func() { theme.Glyph(c, theme.Icons.Delete, 12, theme.Caption(t)) }).Clicked() {
				s.removeHighlight(h)
			}
		})
		if ui.ButtonBase(c).Justify(ui.Start).Label("Go to the highlight").Children(func() {
			ui.Text(c, h.Text).FontSize(13).Grow(1)
		}).Clicked() {
			s.show(h.Page)
		}
		if s.editing != nil && *s.editing == keyOf(h) {
			in := ui.TextInput(c, &s.comment).Placeholder("Add a comment").AutoFocus().FontSize(12)
			in.OnTextKey(func(k ui.TextKey) bool {
				if k.Key == ui.KeyEscape {
					s.editing = nil
					return true
				}
				return false
			})
			if in.Submitted() {
				s.setComment(h, s.comment)
			}
			return
		}
		text, color := h.Comment, theme.Caption(t)
		if text == "" {
			text, color = "Add a comment", t.Accent
		}
		if ui.ButtonBase(c).Justify(ui.Start).Label("Edit the comment").Children(func() {
			ui.Text(c, text).FontSize(12).TextColor(color).Grow(1)
		}).Clicked() {
			key := keyOf(h)
			s.editing, s.comment = &key, h.Comment
		}
	})
}
