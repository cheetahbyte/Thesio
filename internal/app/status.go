package app

import (
	"fmt"

	"github.com/cheetahbyte/thesio/internal/app/document"
	"github.com/cheetahbyte/thesio/internal/vault"
)

// docSaveError is why d's last save failed, "" when it did not.
func docSaveError(d *document.Open) string {
	if st := d.Status(); st.State == document.Failed {
		return st.Error
	}
	return ""
}

// rememberWindowSize keeps the window's size for the next launch.
func (ws *Workspace) rememberWindowSize() {
	if ws.win == nil {
		return
	}
	w, h := ws.win.Size()
	size := [2]float32{float32(w), float32(h)}
	if err := ws.store.UpdateSettings(func(s *vault.Settings) { s.WindowSize = &size }); err != nil {
		ws.ToastError(fmt.Sprintf("Could not remember the window size: %v", err))
	}
}
