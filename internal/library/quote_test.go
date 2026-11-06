package library

import (
	"strings"
	"testing"

	"github.com/cheetahbyte/thesio/internal/vault"
)

func TestBlockGlueMatchesTheVaults(t *testing.T) {
	for _, before := range []string{"", "a", "a\n", "a\n\n"} {
		if BlockGlue(before) != vault.BlockGlue(before) { t.Errorf("%q", before) }
	}
}

func TestACleanQuoteJoinsBrokenWordsOnly(t *testing.T) {
	if got := CleanQuote("an exam- ple of  the ﬁrst\nwell- known pre- and post-war Tests- Case"); got != "an example of the first wellknown pre- and post-war Tests- Case" { t.Fatal(got) }
}

func TestPagesFollowTheOffset(t *testing.T) {
	if got := PrintedPage("", 4, 0); got != "5" { t.Error(got) }
	if got := PrintedPage("12", 0, -2); got != "10" { t.Error(got) }
	if got := PrintedPage("", 0, -3); got != "" { t.Error(got) }
	if got := PrintedPage("iv", 3, 5); got != "iv" { t.Error(got) }
	if offset, ok := PageOffset("", 2, " 10 "); !ok || offset != 7 { t.Error(offset, ok) }
	if _, ok := PageOffset("iv", 2, "10"); ok { t.Error("offset for a roman label") }
}

func TestALongQuoteStandsAsABlock(t *testing.T) {
	if got, block := QuoteMarkup("short  text", "key", "3-4"); block || got != "#quote[short text] @key[pp. 3\\-4]" { t.Error(got) }
	if got, _ := QuoteMarkup("short", "key", ""); got != "#quote[short] @key" { t.Error(got) }
	long := strings.Repeat("word ", 40)
	if got, block := QuoteMarkup(long, "key", "7"); !block || !strings.HasPrefix(got, "#quote(block: true, attribution: [@key[p. 7]])[word") { t.Error(got) }
}
