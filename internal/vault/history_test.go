package vault

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"testing"
)

type kept struct {
	time int64
	text string
}

// seeded is versions of `a.typ` holding these files, as earlier runs left them.
func seeded(t *testing.T, files ...kept) *Versions {
	versions := VersionsOf(t.TempDir(), "a.typ")
	if err := os.MkdirAll(versions.dir, 0o755); err != nil {
		t.Fatal(err)
	}
	for _, f := range files {
		if err := os.WriteFile(filepath.Join(versions.dir, fmt.Sprintf("%d.typ", f.time)), []byte(f.text), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	return versions
}

func must[T any](v T, err error) T {
	if err != nil {
		panic(err)
	}
	return v
}

func TestAnUnchangedDocumentAddsNoVersion(t *testing.T) {
	versions := seeded(t, kept{100, "one"})
	if must(versions.Keep("one")) {
		t.Fatal("kept")
	}
	if !slices.Equal(must(versions.List()), []int64{100}) {
		t.Fatal(must(versions.List()))
	}
}

func TestAChangedDocumentAddsOneAndTheOldStaysReadable(t *testing.T) {
	versions := seeded(t, kept{Now() - 100, "one"})
	if !must(versions.Keep("two")) {
		t.Fatal("not kept")
	}
	times := must(versions.List())
	if len(times) != 2 {
		t.Fatal(times)
	}
	if must(versions.Read(times[0])) != "two" || must(versions.Read(times[1])) != "one" {
		t.Fatal("texts")
	}
}

func TestOldVersionsThinToOneADay(t *testing.T) {
	// Aligned to a day boundary, so now cannot split the seeded days.
	today := Now() / Day * Day
	a, b := today-10*Day, today-9*Day
	versions := seeded(t, kept{a, "1"}, kept{a + 60, "2"}, kept{b, "3"}, kept{b + 60, "4"})
	must(versions.Keep("latest"))
	if times := must(versions.List()); !slices.Equal(times[1:], []int64{b + 60, a + 60}) {
		t.Fatal(times)
	}
}

func TestVersionsPastTheDailyLimitGo(t *testing.T) {
	versions := seeded(t, kept{Now() - (dailyDays+1)*Day, "old"})
	must(versions.Keep("latest"))
	if len(must(versions.List())) != 1 {
		t.Fatal(must(versions.List()))
	}
}

func TestVersionsOfTheLastDayAllStay(t *testing.T) {
	now := Now()
	versions := seeded(t, kept{now - 200, "1"}, kept{now - 100, "2"})
	must(versions.Keep("latest"))
	if len(must(versions.List())) != 3 {
		t.Fatal(must(versions.List()))
	}
}

func TestWritingInsideTheIntervalKeepsNothing(t *testing.T) {
	versions := seeded(t, kept{Now() - versionInterval + 60, "one"})
	if must(versions.KeepPaced("two")) {
		t.Fatal("kept")
	}
}

func TestWritingPastTheIntervalKeepsAVersion(t *testing.T) {
	versions := seeded(t, kept{Now() - versionInterval, "one"})
	if !must(versions.KeepPaced("two")) {
		t.Fatal("not kept")
	}
}

func TestVersionsSurviveARenameThatChangesTheExtension(t *testing.T) {
	root := t.TempDir()
	old := VersionsOf(root, "a.typ")
	if err := os.MkdirAll(old.dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(old.dir, "100.typ"), []byte("text"), 0o644); err != nil {
		t.Fatal(err)
	}
	renamed := VersionsOf(root, "a.txt")
	if err := old.moveTo(renamed); err != nil {
		t.Fatal(err)
	}
	if !slices.Equal(must(renamed.List()), []int64{100}) || must(renamed.Read(100)) != "text" {
		t.Fatal(must(renamed.List()))
	}
}
