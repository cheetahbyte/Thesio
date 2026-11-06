package papers

import (
	"context"
	"strconv"
	"strings"

	"github.com/cheetahbyte/thesio/internal/library"
)

type fetchJob struct {
	ids  library.PaperIDs
	dest string
}

// usableEmail is an address Unpaywall may accept: something@host.tld.
func usableEmail(email string) string {
	name, domain, ok := strings.Cut(strings.TrimSpace(email), "@")
	if !ok || name == "" || !strings.Contains(domain, ".") {
		return ""
	}
	return strings.TrimSpace(email)
}

// FetchPapers downloads open-access papers for keys, one after another,
// skipping entries that have one or have nothing to find it by.
func (s *State) FetchPapers(keys []string) {
	lib := s.h.Library()
	if lib == nil {
		return
	}
	email := usableEmail(s.h.Prefs().UnpaywallEmail)
	var jobs []fetchJob
	for _, key := range keys {
		if s.HasPaper(key) {
			continue
		}
		ids := lib.PaperIDs(key)
		dest, err := s.h.Store().Path(library.PaperFile(key))
		if err != nil || ids.DOI == "" && ids.Arxiv == "" {
			continue
		}
		jobs = append(jobs, fetchJob{ids, dest})
	}
	if len(jobs) == 0 {
		if len(keys) == 1 && !s.HasPaper(keys[0]) {
			s.h.ToastError("This entry has no DOI or arXiv ID to find a paper by")
		}
		return
	}
	total := len(jobs)
	s.fetching += total
	go func() {
		found, problem := 0, ""
		for _, job := range jobs {
			downloaded, err := library.FetchPaper(context.Background(), job.ids, email, job.dest)
			if downloaded {
				found++
			}
			if err != nil {
				problem = err.Error()
			}
			s.h.Update(func() {
				s.fetching--
				if downloaded {
					s.h.RefreshEntries()
				}
			})
		}
		s.h.Update(func() {
			switch {
			case found == 0 && problem != "":
				s.h.ToastError("Could not download the paper: " + problem)
			case found == 0 && email == "":
				s.h.ToastError("No PDF on arXiv. Add an email in Settings to search Unpaywall")
			case found == 0:
				s.h.ToastError("No open-access PDF found")
			case total == 1:
				s.h.Toast("Downloaded the paper")
			default:
				s.h.Toast("Downloaded " + strconv.Itoa(found) + " of " + strconv.Itoa(total) + " papers")
			}
		})
	}()
}
