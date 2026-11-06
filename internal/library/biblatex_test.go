package library

import (
	"strings"
	"testing"
)

func exported(t *testing.T, yaml string) string {
	raw, err := parseMapping(yaml)
	must(t, err)
	parsed, err := Hayagriva.Entries(yaml)
	must(t, err)
	return export(parsed, raw)
}

func TestEveryFieldWithANameIsExported(t *testing.T) {
	text := exported(t, "a:\n  type: Video\n  title: T\n  genre: Film\n  organization: Org\n  page-total: 3\n  serial-number:\n    serial: 7\n    version: 2\n")
	for _, line := range []string{"title = {T}", "type = {Film}", "organization = {Org}", "pagetotal = {3}", "number = {7}", "version = {2}"} {
		if !strings.Contains(text, line) { t.Errorf("%s in %s", line, text) }
	}
}

func TestANumberIsTheEntrysOwnBeforeItsIssue(t *testing.T) {
	if text := exported(t, "a:\n  type: Report\n  title: T\n  issue: 4\n  serial-number:\n    serial: 7\n"); !strings.Contains(text, "number = {7}") { t.Error(text) }
	if text := exported(t, "a:\n  type: Article\n  title: T\n  parent:\n    type: Periodical\n    title: J\n    issue: 4\n"); !strings.Contains(text, "number = {4}") { t.Error(text) }
}

func TestOrganizationsAndParentsTakeTheNameTheirTypeUses(t *testing.T) {
	thesis := exported(t, "a:\n  type: Thesis\n  title: T\n  organization: TU\n")
	if !strings.HasPrefix(thesis, "@thesis{a,") || !strings.Contains(thesis, "institution = {TU}") { t.Error(thesis) }
	journal := exported(t, "a:\n  type: Article\n  title: T\n  parent:\n    type: Periodical\n    title: J\n")
	if !strings.HasPrefix(journal, "@article{a,") || !strings.Contains(journal, "journaltitle = {J}") { t.Error(journal) }
	paper := exported(t, "a:\n  type: Article\n  title: T\n  parent:\n    type: Proceedings\n    title: P\n")
	if !strings.HasPrefix(paper, "@inproceedings{a,") || !strings.Contains(paper, "booktitle = {P}") { t.Error(paper) }
}

func TestOnlyTwoLetterLanguageCodesImport(t *testing.T) {
	l := library(t)
	importing(t, l, "@book{a, title = {A}, language = {german}}\n@book{b, title = {B}, language = {de}}")
	if got := l.FieldText("a", Language); got != "" { t.Error(got) }
	if got := l.FieldText("b", Language); got != "de" { t.Error(got) }
}
