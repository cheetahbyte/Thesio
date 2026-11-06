package vault

import "strings"

// ResolveLiteral is the vault path a path literal written in the folder dir points
// at, unless it leaves the vault. `@preview/…` is a package, and a URL has no
// place in the vault.
func ResolveLiteral(dir []string, literal string) (string, bool) {
	if strings.HasPrefix(literal, "@") || strings.Contains(literal, "://") {
		return "", false
	}
	var parts []string
	rest, rooted := strings.CutPrefix(literal, "/")
	if !rooted {
		parts = append(parts, dir...)
	}
	for _, segment := range strings.Split(rest, "/") {
		switch segment {
		case "", ".":
		case "..":
			if len(parts) == 0 {
				return "", false
			}
			parts = parts[:len(parts)-1]
		default:
			parts = append(parts, segment)
		}
	}
	if len(parts) == 0 {
		return "", false
	}
	return strings.Join(parts, "/"), true
}

// RelativeTo is target as a path relative to the folder dir.
func RelativeTo(dir []string, target string) string {
	parts := strings.Split(target, "/")
	shared := 0
	for shared < len(dir) && shared < len(parts) && dir[shared] == parts[shared] {
		shared++
	}
	out := make([]string, 0, len(dir)-shared+len(parts)-shared)
	for range len(dir) - shared {
		out = append(out, "..")
	}
	return strings.Join(append(out, parts[shared:]...), "/")
}

// EscapeString is text as the inside of a string literal.
func EscapeString(text string) string {
	return strings.ReplaceAll(strings.ReplaceAll(text, `\`, `\\`), `"`, `\"`)
}

// Quoted is text as a string literal.
func Quoted(text string) string { return `"` + EscapeString(text) + `"` }

// EscapeMarkup is text as Typst markup that renders literally.
func EscapeMarkup(text string) string {
	chars := []rune(text)
	var out strings.Builder
	out.Grow(len(text))
	for i, c := range chars {
		var next rune
		if i+1 < len(chars) {
			next = chars[i+1]
		}
		special := false
		switch c {
		case '\\', '#', '$', '*', '_', '`', '@', '<', '>', '[', ']', '~':
			special = true
		case '/': // comments, and a term list at the start
			special = i == 0 || next == '/' || next == '*'
		case '-': // lists, dashes, soft hyphens, and minus signs
			special = i == 0 || chars[i-1] == '-' || next == '-' || next == '?' || isDigit(next)
		case '+', '=':
			special = i == 0
		case '.': // a numbered list at the start, and the ellipsis shorthand
			special = i > 0 && allDigits(chars[:i]) || strings.HasPrefix(string(chars[i:]), "...")
		}
		if special {
			out.WriteByte('\\')
		}
		out.WriteRune(c)
	}
	return out.String()
}

func allDigits(s []rune) bool {
	for _, c := range s {
		if !isDigit(c) {
			return false
		}
	}
	return true
}

// BlockGlue is what goes after before so that a block starts a paragraph of its own.
func BlockGlue(before string) string {
	switch {
	case before == "" || strings.HasSuffix(before, "\n\n"):
		return ""
	case strings.HasSuffix(before, "\n"):
		return "\n"
	default:
		return "\n\n"
	}
}
