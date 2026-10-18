package live

import (
	"reflect"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// Private evidence projection stays an offline unit. Real DTO decoding and
// durable root binding are exercised by tests/integration/TestShippedThreadSource.
func TestShippedThreadSourceProjection(t *testing.T) {
	sources := []protocolv2.SessionSource{
		protocolv2.NewSessionSourceCli(), protocolv2.NewSessionSourceVscode(),
		protocolv2.NewSessionSourceExec(), protocolv2.NewSessionSourceAppServer(), protocolv2.NewSessionSourceUnknown(),
	}
	for _, source := range sources {
		t.Run(string(source.Kind()), func(t *testing.T) {
			value := protocolv2.Thread{ID: "root", Model: protocolv2.Value("grok-4.7"), ModelProvider: "grok", Source: source,
				Turns: []protocolv2.Turn{{ID: "turn", Status: protocolv2.TurnStatusCompleted, Items: []protocolv2.ThreadItem{}}}}
			got, err := productThreadFromSDK(value)
			want := productThread{ID: "root", Model: "grok-4.7", Provider: "grok", Turns: []productTurn{{ID: "turn", Status: "completed"}}}
			if err != nil || !reflect.DeepEqual(got, want) {
				t.Fatal("root source projection invented lineage or lost durable turn binding")
			}
		})
	}
}
