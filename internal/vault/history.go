package vault

import (
	"cmp"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
)

// Earlier versions of a file are plain files in `.thesio/history/<id>/`, each
// named for the second it was taken, readable and recoverable by hand.
const (
	// versionInterval is the least time between versions taken while writing, in seconds.
	versionInterval = 5 * 60
	// dailyDays: every version stays for a day, then one a day for this many days.
	dailyDays = 90
)

type Versions struct{ dir string }

// VersionsOf is the versions of id, which must be a checked vault path, in the vault at root.
func VersionsOf(root, id string) *Versions {
	return &Versions{dir: filepath.Join(root, StateDir, "history", filepath.FromSlash(id))}
}

func versionTime(name string) (int64, bool) {
	stem, _, _ := splitName(name)
	t, err := strconv.ParseUint(stem, 10, 63)
	return int64(t), err == nil
}

// List is their times, newest first.
func (v *Versions) List() []int64 {
	items, _ := os.ReadDir(v.dir)
	var times []int64
	for _, item := range items {
		if t, ok := versionTime(item.Name()); ok { times = append(times, t) }
	}
	slices.SortFunc(times, func(a, b int64) int { return cmp.Compare(b, a) })
	return times
}

// file is the version from time. A rename can change the file's extension, and
// versions keep the one they were taken with.
func (v *Versions) file(time int64) (string, bool) {
	items, _ := os.ReadDir(v.dir)
	for _, item := range items {
		if t, ok := versionTime(item.Name()); ok && t == time { return filepath.Join(v.dir, item.Name()), true }
	}
	return "", false
}

func (v *Versions) newFile(time int64) string {
	ext := strings.TrimPrefix(filepath.Ext(v.dir), ".")
	if ext == "" { ext = "txt" }
	return filepath.Join(v.dir, fmt.Sprintf("%d.%s", time, ext))
}

func (v *Versions) Read(time int64) (string, error) {
	file, ok := v.file(time)
	if !ok { return "", fmt.Errorf("Could not find the version from %d", time) }
	data, err := os.ReadFile(file)
	if err != nil { return "", fmt.Errorf("Could not read the version from %d: %w", time, err) }
	return string(data), nil
}

// Keep keeps text unless the newest version already holds it.
func (v *Versions) Keep(text string) (bool, error) {
	if times := v.List(); len(times) > 0 {
		if kept, err := v.Read(times[0]); err == nil && kept == text { return false, nil }
	}
	if err := os.MkdirAll(v.dir, 0o755); err != nil { return false, err }
	time := Now()
	file, ok := v.file(time)
	if !ok { file = v.newFile(time) }
	if err := WriteAtomic(file, []byte(text)); err != nil { return false, err }
	v.prune(time)
	return true, nil
}

// KeepPaced is Keep at most once per interval, for use while writing.
func (v *Versions) KeepPaced(text string) (bool, error) {
	if times := v.List(); len(times) > 0 && Now()-times[0] < versionInterval { return false, nil }
	return v.Keep(text)
}

// prune thins out old versions. The newest always stays, however old.
func (v *Versions) prune(now int64) {
	keptDay, kept := int64(0), false
	for i, time := range v.List() {
		age := max(now-time, 0)
		if i == 0 || age < Day { continue }
		day := time / Day
		if age < dailyDays*Day && !(kept && keptDay == day) {
			keptDay, kept = day, true
		} else if file, ok := v.file(time); ok {
			os.Remove(file)
		}
	}
}

func (v *Versions) moveTo(other *Versions) error {
	if _, err := os.Stat(v.dir); err != nil { return nil }
	if err := os.MkdirAll(filepath.Dir(other.dir), 0o755); err != nil { return err }
	return os.Rename(v.dir, other.dir)
}
