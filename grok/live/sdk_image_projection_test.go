package live

import (
	"context"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
	"testing"
)

func TestImageTypedUnrelatedToolsGrantNoEvidence(t *testing.T) {
	for _, arguments := range []protocolv2.JSONValue{protocolv2.JSONNull(), protocolv2.JSONString("argument"), protocolv2.JSONObject(map[string]protocolv2.JSONValue{}), protocolv2.JSONArray(nil), protocolv2.JSONBool(false)} {
		values := []protocolv2.ThreadItem{
			protocolv2.NewThreadItemDynamicToolCall(protocolv2.ThreadItemDynamicToolCall{ID: "call", Arguments: arguments, Status: protocolv2.DynamicToolCallStatusCompleted}),
			protocolv2.NewThreadItemMCPToolCall(protocolv2.ThreadItemMCPToolCall{ID: "call", Arguments: arguments, Status: protocolv2.McpToolCallStatusCompleted}),
		}
		for _, value := range values {
			item, err := imageItemFromSDK(value)
			if err != nil {
				t.Fatal(err)
			}
			probe := &imageProbe{ctx: context.Background(), calls: map[string]*imageCall{}, evidence: &ImageEvidence{}}
			if err := probe.public(item, false); err != nil || len(probe.calls) != 0 || probe.evidence.Images != 0 || probe.evidence.HistorySelected {
				t.Fatal("unrelated typed tool supplied image evidence")
			}
		}
	}
}
