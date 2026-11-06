package library

import "testing"

func label(t *testing.T, yaml string) string { return SourceTypeOf(entryOf(t, yaml)).Label }

func TestAnArticleWithoutAJournalIsAJournalArticleUntilSomethingSaysOtherwise(t *testing.T) {
	if got := label(t, "type: Article\ntitle: T"); got != "Journal Article" {
		t.Fatal(got)
	}
}

func TestAnArxivIDLinkOrArchiveMakesAPreprint(t *testing.T) {
	for _, yaml := range []string{"type: Article\nserial-number:\n  arxiv: 2112.12331", "type: Article\nurl: https://arxiv.org/abs/2112.12331", "type: Article\narchive: SSRN"} {
		if got := label(t, yaml); got != "Preprint" {
			t.Errorf("%s: %s", yaml, got)
		}
	}
}
