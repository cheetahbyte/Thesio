package vault

import (
	"cmp"
	"errors"
	"fmt"
	"io/fs"
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

// Versions is the earlier versions of one file.
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

// items is the files in the history folder, none before the first version.
func (v *Versions) items() ([]os.DirEntry, error) {
	items, err := os.ReadDir(v.dir)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("read the history: %w", err)
	}
	return items, nil
}

// List is their times, newest first.
func (v *Versions) List() ([]int64, error) {
	items, err := v.items()
	if err != nil {
		return nil, err
	}
	var times []int64
	for _, item := range items {
		if t, ok := versionTime(item.Name()); ok {
			times = append(times, t)
		}
	}
	slices.SortFunc(times, func(a, b int64) int { return cmp.Compare(b, a) })
	return times, nil
}

// file is the version from time. A rename can change the file's extension, and
// versions keep the one they were taken with.
func (v *Versions) file(time int64) (string, bool, error) {
	items, err := v.items()
	if err != nil {
		return "", false, err
	}
	for _, item := range items {
		if t, ok := versionTime(item.Name()); ok && t == time {
			return filepath.Join(v.dir, item.Name()), true, nil
		}
	}
	return "", false, nil
}

func (v *Versions) newFile(time int64) string {
	ext := strings.TrimPrefix(filepath.Ext(v.dir), ".")
	if ext == "" {
		ext = "txt"
	}
	return filepath.Join(v.dir, fmt.Sprintf("%d.%s", time, ext))
}

// Read is the text of the version from time.
func (v *Versions) Read(time int64) (string, error) {
	file, ok, err := v.file(time)
	if err != nil {
		return "", err
	}
	if !ok {
		return "", fmt.Errorf("%w %d", ErrVersionNotFound, time)
	}
	data, err := os.ReadFile(file)
	if err != nil {
		return "", fmt.Errorf("%w the version from %d: %w", ErrCannotRead, time, err)
	}
	return string(data), nil
}

// Keep keeps text unless the newest version already holds it.
func (v *Versions) Keep(text string) (bool, error) {
	times, err := v.List()
	if err != nil {
		return false, err
	}
	if len(times) > 0 {
		if kept, rerr := v.Read(times[0]); rerr == nil && kept == text {
			return false, nil
		}
	}
	if err = os.MkdirAll(v.dir, 0o755); err != nil {
		return false, fmt.Errorf("keep a version: %w", err)
	}
	time := Now()
	file, ok, err := v.file(time)
	if err != nil {
		return false, err
	}
	if !ok {
		file = v.newFile(time)
	}
	if err := WriteAtomic(file, []byte(text)); err != nil {
		return false, err
	}
	return true, v.prune(time)
}

// KeepPaced is Keep at most once per interval, for use while writing.
func (v *Versions) KeepPaced(text string) (bool, error) {
	times, err := v.List()
	if err != nil {
		return false, err
	}
	if len(times) > 0 && Now()-times[0] < versionInterval {
		return false, nil
	}
	return v.Keep(text)
}

// prune thins out old versions. The newest always stays, however old.
func (v *Versions) prune(now int64) error {
	times, err := v.List()
	if err != nil {
		return err
	}
	keptDay, kept := int64(0), false
	for i, time := range times {
		age := max(now-time, 0)
		if i == 0 || age < Day {
			continue
		}
		day := time / Day
		if age < dailyDays*Day && (!kept || keptDay != day) {
			keptDay, kept = day, true
			continue
		}
		file, ok, err := v.file(time)
		if err != nil {
			return err
		}
		if !ok {
			continue
		}
		if err := os.Remove(file); err != nil {
			return fmt.Errorf("prune old versions: %w", err)
		}
	}
	return nil
}

func (v *Versions) moveTo(other *Versions) error {
	_, err := os.Stat(v.dir)
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("move the history: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(other.dir), 0o755); err != nil {
		return fmt.Errorf("move the history: %w", err)
	}
	if err := os.Rename(v.dir, other.dir); err != nil {
		return fmt.Errorf("move the history: %w", err)
	}
	return nil
}
