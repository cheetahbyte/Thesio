package document

import (
	"strings"
	"time"

	"github.com/cheetahbyte/thesio/internal/typst"
	"github.com/egoist/mygo/ui/editor"
)

// tokenKinds maps typst.Highlight's tags to the editor's token kinds.
var tokenKinds = map[string]editor.TokenKind{
	"comment":        editor.TokenComment,
	"punctuation":    editor.TokenPunctuation,
	"escape":         editor.TokenEscape,
	"strong":         editor.TokenStrong,
	"emph":           editor.TokenEmphasis,
	"link":           editor.TokenLink,
	"raw":            editor.TokenCode,
	"label":          editor.TokenType,
	"ref":            editor.TokenType,
	"heading":        editor.TokenHeading,
	"list-marker":    editor.TokenPunctuation,
	"list-term":      editor.TokenStrong,
	"math-delimiter": editor.TokenOperator,
	"math-operator":  editor.TokenOperator,
	"keyword":        editor.TokenKeyword,
	"operator":       editor.TokenOperator,
	"number":         editor.TokenNumber,
	"string":         editor.TokenString,
	"function":       editor.TokenFunction,
	"interpolated":   editor.TokenVariable,
	"error":          editor.TokenInvalid,
}

// highlighter highlights a buffer with Typst's parser: an edit marks the text
// stale, and the next question parses it whole, once.
type highlighter struct {
	text  editor.Snapshot
	dirty bool
	lines [][]editor.Token
	// block is the lines of the generated bibliography, which fold as one.
	block, blockEnd int
	hasBlock        bool
	folds           map[int]int
}

func newHighlighter(text editor.Snapshot) *highlighter {
	return &highlighter{text: text, dirty: true}
}

func (h *highlighter) Edit(_, text editor.Snapshot, _ []editor.Change) {
	h.text = text
	h.dirty = true
}

func (h *highlighter) Highlight(int, time.Duration) bool {
	h.refresh()
	return true
}

func (h *highlighter) Tokens(i int, _ string, dst []editor.Token) ([]editor.Token, bool) {
	h.refresh()
	if i < len(h.lines) {
		dst = append(dst, h.lines[i]...)
	}
	return dst, true
}

// FoldEnd folds the generated bibliography under its first line, and other
// lines over those indented more than they are.
func (h *highlighter) FoldEnd(line int) (int, bool) {
	h.refresh()
	if h.hasBlock && line == h.block {
		return h.blockEnd, h.blockEnd > line
	}
	if end, ok := h.folds[line]; ok {
		return end, end > line
	}
	end := indentFold(h.text, line)
	h.folds[line] = end
	return end, end > line
}

func (h *highlighter) refresh() {
	if !h.dirty {
		return
	}
	h.dirty = false
	text := h.text.String()
	h.lines = tokenLines(text, typst.Highlight(text))
	h.folds = map[int]int{}
	start, end, ok := typst.BibliographyBlockRange(text)
	h.hasBlock = ok && end > start
	if h.hasBlock {
		h.block, h.blockEnd = h.text.LineOf(start), h.text.LineOf(end-1)
	}
}

// tokenLines splits spans into the tokens of each line, the text between them plain.
func tokenLines(text string, spans []typst.Span) [][]editor.Token {
	starts := []int{0}
	for i := range len(text) {
		if text[i] == '\n' {
			starts = append(starts, i+1)
		}
	}
	lines := make([][]editor.Token, len(starts))
	ends := make([]int, len(starts))
	line := 0
	for _, s := range spans {
		kind, ok := tokenKinds[s.Tag]
		if !ok || s.End <= s.Start {
			continue
		}
		for line+1 < len(starts) && starts[line+1] <= s.Start {
			line++
		}
		for l := line; l < len(starts) && starts[l] < s.End; l++ {
			lineEnd := len(text)
			if l+1 < len(starts) {
				lineEnd = starts[l+1] - 1
			}
			from, to := max(s.Start, starts[l])-starts[l], min(s.End, lineEnd)-starts[l]
			if to <= from || from < ends[l] {
				continue
			}
			if from > ends[l] {
				lines[l] = append(lines[l], editor.Token{End: from, Kind: editor.TokenText})
			}
			lines[l] = append(lines[l], editor.Token{End: to, Kind: kind})
			ends[l] = to
		}
	}
	return lines
}

const foldLookahead = 200

// indentFold is the last line indented more than line, among those after it.
func indentFold(text editor.Snapshot, line int) int {
	n := text.LineCount()
	s := text.Line(line)
	if strings.TrimSpace(s) == "" {
		return line
	}
	in := indentWidth(s)
	end := line
	for i := line + 1; i < min(n, line+foldLookahead); i++ {
		t := text.Line(i)
		if strings.TrimSpace(t) == "" {
			continue
		}
		if indentWidth(t) <= in {
			break
		}
		end = i
	}
	return end
}

func indentWidth(s string) int {
	w := 0
	for _, r := range s {
		switch r {
		case ' ':
			w++
		case '\t':
			w += 2
		default:
			return w
		}
	}
	return w
}
