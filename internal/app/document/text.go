package document

import (
	"unicode"
	"unicode/utf8"
)

func lastRune(s string) rune {
	r, n := utf8.DecodeLastRuneInString(s)
	if n == 0 {
		return 0
	}
	return r
}

func firstRune(s string) rune {
	r, n := utf8.DecodeRuneInString(s)
	if n == 0 {
		return 0
	}
	return r
}

func isSpace(r rune) bool { return unicode.IsSpace(r) }
func isAlnum(r rune) bool { return unicode.IsLetter(r) || unicode.IsDigit(r) }

// isIdent is what continues a Typst identifier.
func isIdent(r rune) bool {
	return r == '_' || r == '-' || unicode.IsLetter(r) || unicode.IsDigit(r) || unicode.Is(unicode.Mn, r) || unicode.Is(unicode.Mc, r)
}

// wordStart is where the identifier before caret starts.
func wordStart(text string, caret int) int {
	for caret > 0 {
		r, n := utf8.DecodeLastRuneInString(text[:caret])
		if !isIdent(r) {
			break
		}
		caret -= n
	}
	return caret
}

// wordAround is the identifier at offset, or else the character there.
func wordAround(text string, offset int) (int, int) {
	if offset < 0 || offset > len(text) {
		return offset, offset
	}
	end := offset
	for end < len(text) {
		r, n := utf8.DecodeRuneInString(text[end:])
		if !isIdent(r) {
			break
		}
		end += n
	}
	if end == offset && end < len(text) {
		_, n := utf8.DecodeRuneInString(text[end:])
		end += n
	}
	return wordStart(text, offset), end
}
