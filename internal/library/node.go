package library

import (
	"bytes"
	"fmt"

	"gopkg.in/yaml.v3"
)

// Helpers over yaml.v3 nodes, which keep key order, comments, and every field
// the form does not show, as serde_yaml's Mapping did.

func newMap() *yaml.Node { return &yaml.Node{Kind: yaml.MappingNode, Tag: "!!map"} }
func newStr(text string) *yaml.Node { return &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: text} }
func newSeq(items []*yaml.Node) *yaml.Node { return &yaml.Node{Kind: yaml.SequenceNode, Tag: "!!seq", Content: items} }

func deref(n *yaml.Node) *yaml.Node {
	for n != nil && n.Kind == yaml.AliasNode { n = n.Alias }
	return n
}

func asMap(n *yaml.Node) *yaml.Node {
	if n = deref(n); n != nil && n.Kind == yaml.MappingNode { return n }
	return nil
}

func get(m *yaml.Node, key string) *yaml.Node {
	if m = asMap(m); m == nil { return nil }
	for i := 0; i+1 < len(m.Content); i += 2 {
		if m.Content[i].Value == key { return deref(m.Content[i+1]) }
	}
	return nil
}

func has(m *yaml.Node, key string) bool { return get(m, key) != nil }

// set puts value under key, in place when the key is there, else at the end. A
// nil value removes the key, keeping the order of the rest.
func set(m *yaml.Node, key string, value *yaml.Node) {
	for i := 0; i+1 < len(m.Content); i += 2 {
		if m.Content[i].Value != key { continue }
		if value == nil { m.Content = append(m.Content[:i:i], m.Content[i+2:]...) } else { m.Content[i+1] = value }
		return
	}
	if value != nil { m.Content = append(m.Content, newStr(key), value) }
}

func keys(m *yaml.Node) []string {
	var names []string
	for i := 0; i+1 < len(m.Content); i += 2 { names = append(names, m.Content[i].Value) }
	return names
}

func clone(n *yaml.Node) *yaml.Node {
	if n == nil { return nil }
	c := *n
	c.Content = make([]*yaml.Node, len(n.Content))
	for i, child := range n.Content { c.Content[i] = clone(child) }
	return &c
}

func equal(a, b *yaml.Node) bool {
	a, b = deref(a), deref(b)
	if a == nil || b == nil { return a == b }
	if a.Kind != b.Kind || a.Value != b.Value || a.ShortTag() != b.ShortTag() || len(a.Content) != len(b.Content) { return false }
	for i := range a.Content {
		if !equal(a.Content[i], b.Content[i]) { return false }
	}
	return true
}

// scalar is a string or number's text, as serde_yaml's Value::String and Number.
func scalar(n *yaml.Node) (string, bool) {
	if n = deref(n); n == nil || n.Kind != yaml.ScalarNode { return "", false }
	switch n.ShortTag() {
	case "!!null", "!!bool":
		return "", false
	}
	return n.Value, true
}

func text(n *yaml.Node) string { t, _ := scalar(n); return t }

func isNumber(n *yaml.Node) bool {
	n = deref(n)
	return n != nil && n.Kind == yaml.ScalarNode && (n.ShortTag() == "!!int" || n.ShortTag() == "!!float")
}

// parseMapping reads text that must be a mapping or nothing at all.
func parseMapping(text string) (*yaml.Node, error) {
	var doc yaml.Node
	if err := yaml.Unmarshal([]byte(text), &doc); err != nil { return nil, err }
	if doc.Kind == 0 { return newMap(), nil }
	root := deref(doc.Content[0])
	switch {
	case root.Kind == yaml.MappingNode:
		return root, nil
	case root.Kind == yaml.ScalarNode && root.ShortTag() == "!!null":
		return newMap(), nil
	}
	return nil, fmt.Errorf("line %d: expected a mapping of entries", root.Line)
}

// encode writes a mapping as YAML, empty for an empty one.
func encode(m *yaml.Node) (string, error) {
	if len(m.Content) == 0 { return "", nil }
	var out bytes.Buffer
	enc := yaml.NewEncoder(&out)
	enc.SetIndent(2)
	if err := enc.Encode(m); err != nil { return "", err }
	if err := enc.Close(); err != nil { return "", err }
	return out.String(), nil
}
