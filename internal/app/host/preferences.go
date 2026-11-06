package host

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/cheetahbyte/thesio/internal/vault"
)

// Preferences is `.thesio/preferences.json` in the vault. Fields keep the
// names earlier builds wrote.
type Preferences struct {
	// Appearance is "light", "dark", or "" to follow the system.
	Appearance string `json:"appearance"`
	// HideLibraryFiles keeps library.yaml and the papers out of the sidebar.
	HideLibraryFiles bool `json:"hide_library_files"`
	// UnpaywallEmail is sent to Unpaywall, which requires one, to find open-access papers.
	UnpaywallEmail string `json:"unpaywall_email"`
	// LibraryColumns holds column widths by column key, where they were resized.
	LibraryColumns map[string]float32 `json:"library_columns"`
	// LibraryDetailsHidden keeps the Library's details panel closed.
	LibraryDetailsHidden bool `json:"library_details_hidden"`

	path string
	// problem is why the file could not be read; nothing writes to it meanwhile.
	problem error
}

// ErrPreferencesUnreadable is Save refusing to overwrite a file it could not read.
var ErrPreferencesUnreadable = errors.New("preferences.json could not be read and was left alone")

// DefaultPreferences are the preferences of a vault without the file.
func DefaultPreferences() Preferences {
	return Preferences{HideLibraryFiles: true, LibraryColumns: map[string]float32{}}
}

// LoadPreferences reads the vault's preferences. A file that cannot be read
// gives the defaults and is left alone by Save.
func LoadPreferences(root string) *Preferences {
	p := DefaultPreferences()
	p.path = filepath.Join(root, vault.StateDir, "preferences.json")
	data, err := os.ReadFile(p.path)
	switch {
	case errors.Is(err, os.ErrNotExist):
	case err != nil:
		p.problem = err
	default:
		if err := json.Unmarshal(data, &p); err != nil {
			p.problem = err
		}
	}
	if p.LibraryColumns == nil {
		p.LibraryColumns = map[string]float32{}
	}
	return &p
}

// Save writes the preferences.
func (p *Preferences) Save() error {
	if p.problem != nil {
		return fmt.Errorf("%w: %w", ErrPreferencesUnreadable, p.problem)
	}
	if err := os.MkdirAll(filepath.Dir(p.path), 0o755); err != nil {
		return fmt.Errorf("create the preferences folder: %w", err)
	}
	data, err := json.MarshalIndent(p, "", "  ")
	if err != nil {
		return fmt.Errorf("encode the preferences: %w", err)
	}
	return vault.WriteAtomic(p.path, data)
}
