package citations

import (
	"context"
	"fmt"

	"github.com/cheetahbyte/thesio/internal/app/theme"
	"github.com/cheetahbyte/thesio/internal/library"
	"github.com/egoist/mygo/ui"
)

// lookup is Add by identifier.
type lookup struct {
	open bool
	text string
	busy bool
	// problems are what could not be added, one line each.
	problems []string
}

// fetched is one identifier looked up: an entry, or why not.
type fetched struct {
	id      library.Identifier
	entry   library.Entry
	problem string
}

// OpenLookup opens Add by identifier.
func (s *State) OpenLookup() { s.lookup = lookup{open: true} }

// runLookup fetches everything in the field on a goroutine, then adds what came back.
func (s *State) runLookup() {
	if s.lookup.busy {
		return
	}
	parsed := library.Identifiers(s.lookup.text)
	if len(parsed) == 0 {
		return
	}
	s.lookup.busy, s.lookup.problems = true, nil
	go func() {
		results := make([]fetched, len(parsed))
		for i, token := range parsed {
			if !token.OK {
				results[i].problem = fmt.Sprintf("%s: not a DOI, arXiv ID, PMID, or ISBN", token.Text)
				continue
			}
			entry, err := library.Fetch(context.Background(), token.ID)
			results[i] = fetched{id: token.ID, entry: entry}
			if err != nil {
				results[i].problem = fmt.Sprintf("%s: %v", token.ID, err)
			}
		}
		s.h.Update(func() { s.finishLookup(results) })
	}()
}

func (s *State) finishLookup(results []fetched) {
	s.h.ReloadLibrary()
	added, already := 0, 0
	var problems, keys []string
	for _, r := range results {
		if r.problem != "" {
			problems = append(problems, r.problem)
			continue
		}
		lib := s.library()
		if lib == nil {
			problems = append(problems, "The library could not be read")
			break
		}
		result, err := lib.AddFetched(r.entry)
		switch {
		case err != nil:
			problems = append(problems, fmt.Sprintf("%s: %v", r.id, err))
		case result.Duplicate:
			already++
		default:
			added++
			keys = append(keys, result.Key)
		}
	}
	if added > 0 {
		s.refresh()
	}
	s.TableChanged()
	if len(keys) > 0 {
		s.h.FetchPapers(keys)
	}
	if summary := lookupSummary(added, already); summary != "" {
		s.h.Toast(summary)
	}
	if !s.lookup.open {
		return
	}
	s.lookup.busy = false
	if len(problems) > 0 {
		s.lookup.problems = problems
	} else {
		s.lookup.open = false
	}
}

func lookupSummary(added, already int) string {
	switch {
	case added == 0 && already == 0:
		return ""
	case added == 1 && already == 0:
		return "Added 1 entry"
	case already == 0:
		return fmt.Sprintf("Added %d entries", added)
	case added == 0 && already == 1:
		return "Already in the library"
	case added == 0:
		return fmt.Sprintf("All %d are already in the library", already)
	}
	return fmt.Sprintf("Added %d, %d already in the library", added, already)
}

func (s *State) lookupView(c *ui.Context) {
	if !s.lookup.open {
		return
	}
	t := c.Theme()
	var after func()
	theme.Dialog(ui.Modal(c.Key("lookup"), &s.lookup.open, func() {
		ui.Column(c).Width(480).Gap(10).Children(func() {
			ui.Text(c, "Add by identifier").FontSize(15).FontWeight(600)
			ui.Text(c, "Enter one or more DOIs, arXiv IDs, PMIDs, or ISBNs. Each is sent to doi.org, NCBI, or Open Library, and Thesio looks for an open-access PDF through arXiv and Unpaywall.").
				FontSize(13).TextColor(theme.Caption(t))
			if ui.TextInput(c.Key("lookup-field"), &s.lookup.text).Placeholder("DOI, arXiv ID, PMID, or ISBN").Label("Identifiers").AutoFocus().Submitted() {
				after = s.runLookup
			}
			for _, problem := range s.lookup.problems {
				ui.Text(c, problem).FontSize(13).TextColor(theme.Danger(t))
			}
			ui.Row(c).Gap(8).Padding(8, 0, 0, 0).Children(func() {
				if ui.Button(c.Key("lookup-manual"), "Enter Manually…").Clicked() {
					after = func() {
						s.lookup.open = false
						s.OpenNewEntry()
					}
				}
				ui.Spacer(c)
				if ui.Button(c.Key("lookup-cancel"), "Cancel").Clicked() {
					s.lookup.open = false
				}
				if s.lookup.busy {
					ui.Text(c, "Looking up…").FontSize(13).TextColor(theme.Caption(t)).PaddingX(12)
				} else if ui.PrimaryButton(c.Key("lookup-add"), "Add").Clicked() {
					after = s.runLookup
				}
			})
		})
	}), c.Theme())
	if after != nil {
		after()
	}
}
