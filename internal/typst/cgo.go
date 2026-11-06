package typst

/*
#cgo CFLAGS: -I${SRCDIR}/../../typst/include
#cgo LDFLAGS: -L${SRCDIR}/../../typst/target/release -lthesio_typst
#cgo darwin LDFLAGS: -liconv
#cgo linux LDFLAGS: -lm -ldl -lpthread
#include <stdlib.h>
#include "thesio_typst.h"
*/
import "C"

import (
	"encoding/json"
	"errors"
	"unsafe"
)

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
		return nil, errors.New("typst: no reply")
	}
	var r reply
	if err := json.Unmarshal([]byte(text), &r); err != nil {
		return nil, err
	}
	if r.Error != nil {
		return nil, errors.New(*r.Error)
	}
	return r.OK, nil
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
