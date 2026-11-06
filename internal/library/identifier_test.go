package library

import "testing"

func TestIdentify(t *testing.T) {
	cases := []struct {
		token string
		want  Identifier
		ok    bool
	}{
		{"10.1145/3476105", Identifier{IDDOI, "10.1145/3476105"}, true},
		{"https://doi.org/10.1145/3476105.", Identifier{IDDOI, "10.1145/3476105"}, true},
		{"doi:10.1000/xyz", Identifier{IDDOI, "10.1000/xyz"}, true},
		{"2112.12331", Identifier{IDArxiv, "2112.12331"}, true},
		{"2112.12331v2", Identifier{IDArxiv, "2112.12331"}, true},
		{"https://arxiv.org/pdf/2112.12331v1.pdf", Identifier{IDArxiv, "2112.12331"}, true},
		{"arXiv:hep-th/9901001", Identifier{IDArxiv, "hep-th/9901001"}, true},
		{"https://arxiv.org/abs/nonsense", Identifier{}, false},
		{"PMID:12345678", Identifier{IDPMID, "12345678"}, true},
		{"https://pubmed.ncbi.nlm.nih.gov/31452104/", Identifier{IDPMID, "31452104"}, true},
		{"31452104", Identifier{IDPMID, "31452104"}, true},
		{"0-201-13447-0", Identifier{IDISBN, "0201134470"}, true},
		{"ISBN:978-0-306-40615-7", Identifier{IDISBN, "9780306406157"}, true},
		{"080442957X", Identifier{IDISBN, "080442957X"}, true},
		{"0-201-13447-1", Identifier{}, false},
		{"hello", Identifier{}, false},
	}
	for _, c := range cases {
		if got, ok := Identify(c.token); got != c.want || ok != c.ok { t.Errorf("%s: %v %v", c.token, got, ok) }
	}
}

func TestUnrecognisedTokensComeBackAsText(t *testing.T) {
	got := Identifiers("10.1/x, nonsense\n2112.12331")
	if len(got) != 3 || !got[0].OK || got[1].OK || got[1].Text != "nonsense" || got[2].ID != (Identifier{IDArxiv, "2112.12331"}) { t.Fatal(got) }
}

func TestArxivsOwnDOINamesThePaper(t *testing.T) {
	if got := (PaperIDs{DOI: ArxivDOI("2112.12331")}).ArxivID(); got != "2112.12331" { t.Fatal(got) }
	if got := (PaperIDs{DOI: "10.1/x"}).ArxivID(); got != "" { t.Fatal(got) }
	if got := ArxivOfLink("https://arxiv.org/abs/2112.12331"); got != "2112.12331" { t.Fatal(got) }
}
