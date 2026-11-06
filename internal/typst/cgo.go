package typst

/*
#cgo CFLAGS: -I${SRCDIR}/../../typst/include
#cgo darwin,arm64 LDFLAGS: -L${SRCDIR}/../../typst/target/aarch64-apple-darwin/release
#cgo darwin,amd64 LDFLAGS: -L${SRCDIR}/../../typst/target/x86_64-apple-darwin/release
#cgo linux,amd64 LDFLAGS: -L${SRCDIR}/../../typst/target/x86_64-unknown-linux-gnu/release
#cgo linux,arm64 LDFLAGS: -L${SRCDIR}/../../typst/target/aarch64-unknown-linux-gnu/release
#cgo windows,amd64 LDFLAGS: -L${SRCDIR}/../../typst/target/x86_64-pc-windows-gnu/release
#cgo LDFLAGS: -lthesio_typst
#cgo darwin LDFLAGS: -liconv
#cgo linux LDFLAGS: -lgcc_s -lutil -lrt -lpthread -lm -ldl -lc
#cgo windows LDFLAGS: -lbcrypt -ladvapi32 -lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp -lgcc_eh -l:libpthread.a
#include <stdlib.h>
#include "thesio_typst.h"
*/
import "C"

import (
	"encoding/json"
	"errors"
	"fmt"
	"unsafe"
)

// Error is a failure the crate reported, in its words.
type Error struct{ Message string }

func (e *Error) Error() string { return e.Message }

// ErrNoReply is a call the crate answered with nothing.
var ErrNoReply = errors.New("typst: no reply")

// arena holds C strings for one call.
type arena []unsafe.Pointer

func (a *arena) str(s string) *C.char {
	p := C.CString(s)
	*a = append(*a, unsafe.Pointer(p))
	return p
}

func (a arena) free() {
	for _, p := range a {
		C.free(p)
	}
}

// take copies a string the crate handed out and frees it.
func take(p *C.char) (string, bool) {
	if p == nil {
		return "", false
	}
	defer C.thesio_string_free(p)
	return C.GoString(p), true
}

type reply struct {
	OK    json.RawMessage `json:"ok"`
	Error *string         `json:"error"`
}

// result reads a JSON reply: its "ok" value, or its "error" as an error.
func result(p *C.char) (json.RawMessage, error) {
	text, ok := take(p)
	if !ok {
		return nil, ErrNoReply
	}
	var r reply
	if err := json.Unmarshal([]byte(text), &r); err != nil {
		return nil, fmt.Errorf("typst: reading a reply: %w", err)
	}
	if r.Error != nil {
		return nil, &Error{Message: *r.Error}
	}
	return r.OK, nil
}

// decode reads a reply's "ok" value as T, the zero T when the call failed or
// the value is not a T.
func decode[T any](p *C.char) T {
	var v T
	raw, err := result(p)
	if err != nil || json.Unmarshal(raw, &v) != nil {
		var zero T
		return zero
	}
	return v
}

// optionalText reads a reply of a string or null.
func optionalText(p *C.char) (string, bool) {
	raw, err := result(p)
	if err != nil {
		return "", false
	}
	var text *string
	if json.Unmarshal(raw, &text) != nil || text == nil {
		return "", false
	}
	return *text, true
}

func size(n int) C.size_t {
	if n < 0 {
		return 0
	}
	return C.size_t(n)
}
