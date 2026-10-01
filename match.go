package cmslens

import (
	"regexp"
	"regexp/syntax"
	"strings"
)

/*
Body features are searched in a single automaton pass rather than one expression
after another; otherwise the cost of a check grows linearly with the number of
platforms in the table. A literal feature is decided by the automaton alone. For
a pattern, the automaton looks for its required pieces, and the expression runs
only on a page where all of them were found.
*/
var signatureIndex = newBodyIndex(cmsSignatures)

// A piece shorter than three bytes occurs on every page and filters nothing
// out.
const minFactorLen = 3

type featurePlan struct {
	literal  int32 // index of the literal in the automaton if the feature is a pure literal, otherwise -1
	boundary bool
	factors  []int32
}

type bodyIndex struct {
	ac    *automaton
	texts []string
	plans [][]featurePlan // [table entry][body feature]
}

type bodyHits struct {
	seen    []bool
	bounded []bool // at least one occurrence sits on a word boundary, as a leading \b requires
}

func newBodyIndex(table []cmsEntry) *bodyIndex {
	x := &bodyIndex{plans: make([][]featurePlan, len(table))}
	ids := map[string]int32{}
	intern := func(s string) int32 {
		s = strings.ToLower(s)
		if id, ok := ids[s]; ok {
			return id
		}
		id := int32(len(x.texts))
		ids[s] = id
		x.texts = append(x.texts, s)
		return id
	}
	for i, entry := range table {
		x.plans[i] = make([]featurePlan, len(entry.sig.body))
		for j, item := range entry.sig.body {
			x.plans[i][j] = planFeature(item.re, intern)
		}
	}
	x.ac = newAutomaton(x.texts)
	return x
}

func planFeature(re *regexp.Regexp, intern func(string) int32) featurePlan {
	tree, err := syntax.Parse(re.String(), syntax.Perl)
	if err != nil {
		panic(err)
	}
	nodes := []*syntax.Regexp{tree}
	if tree.Op == syntax.OpConcat {
		nodes = tree.Sub
	}

	plan := featurePlan{literal: -1}
	if len(nodes) == 2 && nodes[0].Op == syntax.OpWordBoundary {
		plan.boundary = true
		nodes = nodes[1:]
	}
	// Without (?i) a literal is not handed to the automaton whole: the automaton
	// always folds case
	if s, ok := asciiLiteral(nodes[0]); ok && len(nodes) == 1 && nodes[0].Flags&syntax.FoldCase != 0 {
		plan.literal = intern(s)
		return plan
	}

	plan.boundary = false
	for _, n := range nodes {
		if s, ok := asciiLiteral(n); ok && len(s) >= minFactorLen {
			plan.factors = append(plan.factors, intern(s))
		}
	}
	return plan
}

func asciiLiteral(n *syntax.Regexp) (string, bool) {
	if n.Op != syntax.OpLiteral {
		return "", false
	}
	for _, r := range n.Rune {
		if r >= 0x80 {
			return "", false
		}
	}
	return string(n.Rune), true
}

func (x *bodyIndex) scan(html string) bodyHits {
	h := bodyHits{seen: make([]bool, len(x.texts)), bounded: make([]bool, len(x.texts))}
	x.ac.scan(html, func(id int32, end int) {
		h.seen[id] = true
		if h.bounded[id] {
			return
		}
		text := x.texts[id]
		start := end + 1 - len(text)
		prevWord := start > 0 && isWordByte(html[start-1])
		h.bounded[id] = prevWord != isWordByte(text[0])
	})
	return h
}

func (p featurePlan) matches(h bodyHits, re *regexp.Regexp, html string) bool {
	if p.literal >= 0 {
		if p.boundary {
			return h.bounded[p.literal]
		}
		return h.seen[p.literal]
	}
	for _, f := range p.factors {
		if !h.seen[f] {
			return false
		}
	}
	return re.MatchString(html)
}

// isWordByte mirrors RE2's \b: word boundaries there are ASCII only.
func isWordByte(c byte) bool {
	return c == '_' || '0' <= c && c <= '9' || 'a' <= c && c <= 'z' || 'A' <= c && c <= 'Z'
}

/*
automaton is Aho-Corasick expanded into a full transition table: one table read
per byte, with no walking of failure links. Bytes that occur in no literal share
one class, and ASCII upper case maps to lower case, so the table is states ×
classes rather than states × 256. Case is therefore folded in ASCII only, while
RE2's (?i) also folds "ſ" to "s" and the Kelvin sign to "k"; neither shows up in
real markup.
*/
type automaton struct {
	class  [256]int32
	width  int
	next   []int32   // [state*width + class]
	out    [][]int32 // literals ending exactly in this state
	suffix []int32   // nearest state along the failure chain that has literals, 0 if none
}

func newAutomaton(texts []string) *automaton {
	a := &automaton{width: 1}
	for _, t := range texts {
		for i := 0; i < len(t); i++ {
			if a.class[t[i]] == 0 {
				a.class[t[i]] = int32(a.width)
				a.width++
			}
		}
	}
	for c := byte('A'); c <= 'Z'; c++ {
		a.class[c] = a.class[c+'a'-'A']
	}

	a.grow()
	for id, t := range texts {
		s := int32(0)
		for i := 0; i < len(t); i++ {
			k := int(s)*a.width + int(a.class[t[i]])
			if a.next[k] == 0 {
				// On its own line: grow reallocates a.next, and the order of evaluating the
				// left-hand side of an assignment relative to the call is unspecified
				n := a.grow()
				a.next[k] = n
			}
			s = a.next[k]
		}
		a.out[s] = append(a.out[s], int32(id))
	}

	fail := make([]int32, len(a.out))
	a.suffix = make([]int32, len(a.out))
	var queue []int32
	for _, n := range a.next[:a.width] {
		if n != 0 {
			queue = append(queue, n)
		}
	}
	for len(queue) > 0 {
		s := queue[0]
		queue = queue[1:]
		row := a.next[int(s)*a.width : (int(s)+1)*a.width]
		failRow := a.next[int(fail[s])*a.width : (int(fail[s])+1)*a.width]
		for c, n := range row {
			if n == 0 {
				row[c] = failRow[c]
				continue
			}
			f := failRow[c]
			fail[n] = f
			if len(a.out[f]) > 0 {
				a.suffix[n] = f
			} else {
				a.suffix[n] = a.suffix[f]
			}
			queue = append(queue, n)
		}
	}
	return a
}

func (a *automaton) grow() int32 {
	a.next = append(a.next, make([]int32, a.width)...)
	a.out = append(a.out, nil)
	return int32(len(a.out) - 1)
}

func (a *automaton) scan(data string, emit func(id int32, end int)) {
	s := int32(0)
	for i := 0; i < len(data); i++ {
		s = a.next[int(s)*a.width+int(a.class[data[i]])]
		for t := s; t != 0; t = a.suffix[t] {
			for _, id := range a.out[t] {
				emit(id, i)
			}
		}
	}
}
