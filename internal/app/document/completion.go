package document

import (
	"slices"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/egoist/mygo/ui/editor"
)

// newest runs one question at a time, and only the newest: asking again makes
// every question still waiting answer nothing.
type newest struct {
	latest  atomic.Uint64
	running sync.Mutex
}

func (n *newest) ticket() uint64 { return n.latest.Add(1) }

func (n *newest) run(ticket uint64, f func()) bool {
	n.running.Lock()
	defer n.running.Unlock()
	if n.latest.Load() != ticket {
		return false
	}
	f()
	return true
}

type offerKind int

const (
	offerTypst offerKind = iota
	offerCitation
	offerNoteLink
)

// offer is the completions shown: the editor replaces the text from at to the
// caret with an item's Insert, and what it leaves undone happens once the
// insert appears there.
type offer struct {
	kind  offerKind
	at    int
	items map[string]offered
}

type offered struct {
	// tail is text after the caret the item replaces too.
	tail string
	// fields are placeholders in the insert, the first selected after it.
	fields [][2]int
}

// complete answers the editor's request: a citation after `@`, a note link
// after `[[`, Typst code after `#` or a field's dot, or anything Typst knows
// when asked with Ctrl+Space.
func (d *Open) complete(r editor.CompletionRequest) {
	explicit := r.Explicit && !d.autoAsk
	text := d.Text()
	caret := r.Offset
	if caret < 0 || caret > len(text) {
		return
	}
	line := text[strings.LastIndexByte(text[:caret], '\n')+1 : caret]
	if d.Citations != nil && strings.Contains(line, "@") {
		if start, end, ok := typst.CitationAt(text, caret); ok {
			items := d.Citations(strings.ToLower(text[start+1:caret]), d.Labels())
			for i := range items {
				items[i].FilterText = insertOf(items[i]) + " " + items[i].Label + " " + items[i].FilterText
			}
			d.show(offerCitation, start, text[caret:max(caret, end)], items, nil)
			return
		}
	}
	if d.NoteLinks != nil && strings.Contains(line, "[[") {
		if start, end, ok := typst.NoteLinkAt(text, caret); ok {
			items := d.NoteLinks(strings.ToLower(text[start+2 : caret]))
			for i := range items {
				items[i].FilterText = "[[" + items[i].Label + " " + items[i].Detail + " " + items[i].FilterText
			}
			d.show(offerNoteLink, start, text[caret:max(caret, end)], items, nil)
			return
		}
	}
	w := wordStart(text, caret)
	opens := w > 0 && (text[w-1] == '#' || text[w-1] == '.') && typst.CodeOpens(text, w)
	k := d.s.knowledge()
	if !explicit && !opens || k == nil {
		d.closeCompletions()
		return
	}
	ticket, version, path := d.asking.ticket(), r.Version, d.id
	go func() {
		var found []typst.Suggestion
		if !d.asking.run(ticket, func() { found = k.Complete(path, text, caret, explicit) }) {
			return
		}
		d.h.Update(func() {
			if d.closed || d.buf.Version() != version || d.asking.latest.Load() != ticket {
				return
			}
			d.showSuggestions(text, caret, found)
		})
	}()
}

func (d *Open) closeCompletions() {
	d.offer = nil
	d.ed.ShowCompletions(-1, nil)
}

func insertOf(c editor.Completion) string {
	if c.Insert != "" {
		return c.Insert
	}
	return c.Label
}

// show lists items replacing the text from at to the caret, and tail after it.
func (d *Open) show(kind offerKind, at int, tail string, items []editor.Completion, fields [][][2]int) {
	if len(items) == 0 {
		d.closeCompletions()
		return
	}
	o := &offer{kind: kind, at: at, items: map[string]offered{}}
	for i, item := range items {
		it := offered{tail: tail}
		if i < len(fields) {
			it.fields = fields[i]
		}
		o.items[insertOf(item)] = it
	}
	d.offer = o
	d.ed.ShowCompletions(at, items)
}

var completionKinds = map[string]editor.CompletionKind{
	"Snippet":   editor.CompletionSnippet,
	"Function":  editor.CompletionFunction,
	"Type":      editor.CompletionType,
	"Parameter": editor.CompletionField,
	"Value":     editor.CompletionConstant,
	"File":      editor.CompletionModule,
	"Package":   editor.CompletionModule,
	"Label":     editor.CompletionConstant,
	"Font":      editor.CompletionText,
}

// showSuggestions lists Typst's suggestions, those replacing from where the
// first does.
func (d *Open) showSuggestions(text string, caret int, found []typst.Suggestion) {
	if len(found) == 0 {
		d.closeCompletions()
		return
	}
	from := found[0].From
	if from < 0 || from > caret {
		d.closeCompletions()
		return
	}
	var items []editor.Completion
	var fields [][][2]int
	for _, s := range found {
		if s.From != from {
			continue
		}
		items = append(items, editor.Completion{Label: s.Title, Kind: completionKinds[s.Kind], Insert: s.Insert, Documentation: s.Detail})
		fields = append(fields, s.Fields)
	}
	tail := ""
	if t := found[0].Tail; t > 0 && caret+t <= len(text) {
		tail = text[caret : caret+t]
	}
	d.show(offerTypst, from, tail, items, fields)
}

// takeAccepted finishes an accepted completion: the text after the caret it
// replaces, its first placeholder selected, and a citation announced.
func (d *Open) takeAccepted() {
	o := d.offer
	if o == nil {
		return
	}
	text := d.Text()
	caret := d.ed.Cursor()
	if caret < o.at {
		d.offer = nil
		return
	}
	insert := text[o.at:caret]
	it, ok := o.items[insert]
	if !ok {
		return
	}
	d.offer = nil
	if it.tail != "" && strings.HasPrefix(text[caret:], it.tail) {
		d.buf.Edit(editor.Edit{Start: caret, End: caret + len(it.tail)})
		d.selectRange(caret, caret)
	}
	if len(it.fields) > 0 {
		n := d.buf.Len()
		first := it.fields[0]
		d.selectRange(o.at+first[0], o.at+first[1])
		d.stops = nil
		for _, f := range it.fields[1:] {
			d.stops = append(d.stops, [2]int{n - (o.at + f[0]), n - (o.at + f[1])})
		}
	}
	if o.kind == offerCitation && d.OnCitationCompleted != nil {
		key := strings.TrimPrefix(insert, "@")
		if !slices.ContainsFunc(d.Labels(), func(l typst.Label) bool { return l.Key == key }) {
			d.OnCitationCompleted()
		}
	}
}
