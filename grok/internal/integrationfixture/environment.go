package integrationfixture

import "strings"

// Environment replaces inherited fixture identity without mutating the parent.
func Environment(parent []string, home, key string) []string {
	env := make([]string, 0, len(parent)+2)
	for _, entry := range parent {
		name, _, _ := strings.Cut(entry, "=")
		if !strings.EqualFold(name, "CODEX_HOME") && !strings.EqualFold(name, "GROK_API_KEY") {
			env = append(env, entry)
		}
	}
	return append(env, "CODEX_HOME="+home, "GROK_API_KEY="+key)
}
