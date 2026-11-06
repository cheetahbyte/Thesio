package citations

import (
	"fmt"
	"slices"
	"strings"

	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/egoist/mygo/ui/editor"
)

// What `@` offers, by the editor's kinds: sources, and labels of the document.
const (
	sourceKind = editor.CompletionConstant
	labelKind  = editor.CompletionVariable
)

// ranked is a completion with whether its key starts with the query.
type ranked struct {
	byKey bool
	item  editor.Completion
}

// CompletionItems are what `@query` offers: sources from the library and
// labels of the compiled document, in one list. What starts with query comes
// first, then whatever matches further in, sources ahead of labels in both.
// An empty query, as the popup opens, sees the library as it is on disk now.
func (s *State) CompletionItems(query string, labels []typst.Label) []editor.Completion {
	query = strings.ToLower(query)
	if query == "" {
		s.h.ReloadLibrary()
	}
	var items []ranked
	for _, row := range s.VisibleRows(query) {
		var detail []string
		for _, part := range []string{row.Authors, row.Year, row.Key} {
			if part != "" {
				detail = append(detail, part)
			}
		}
		items = append(items, ranked{strings.HasPrefix(strings.ToLower(row.Key), query), editor.Completion{
			Label:      row.Label(),
			Detail:     strings.Join(detail, " · "),
			Kind:       sourceKind,
			Insert:     "@" + row.Key,
			FilterText: "@" + row.Key + " " + row.Label(),
		}})
	}
	for _, label := range labels {
		key := strings.ToLower(label.Key)
		byKey := strings.HasPrefix(key, query)
		if byKey || strings.Contains(key, query) || strings.Contains(strings.ToLower(label.Title), query) {
			items = append(items, ranked{byKey, labelItem(label)})
		}
	}
	slices.SortStableFunc(items, func(a, b ranked) int {
		switch {
		case a.byKey == b.byKey:
			return 0
		case a.byKey:
			return -1
		}
		return 1
	})
	out := make([]editor.Completion, len(items))
	for i, r := range items {
		out[i] = r.item
	}
	return out
}

func labelItem(label typst.Label) editor.Completion {
	detail := []string{label.Kind}
	if label.Page > 0 {
		detail = append(detail, fmt.Sprintf("p. %d", label.Page))
	}
	detail = append(detail, label.Key)
	return editor.Completion{
		Label:      label.Title,
		Detail:     strings.Join(slices.DeleteFunc(detail, func(d string) bool { return d == "" }), " · "),
		Kind:       labelKind,
		Insert:     "@" + label.Key,
		FilterText: "@" + label.Key + " " + label.Title,
	}
}
