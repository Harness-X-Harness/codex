package live

import (
	"encoding/json"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// These are application evidence projections of already decoded SDK values,
// not a second request dispatcher or alternate protocol decoder.
func productTurnFromSDK(value protocolv2.Turn) (productTurn, error) {
	result := productTurn{ID: value.ID, Status: string(value.Status)}
	if value.Error != nil && value.Error.Value != nil {
		var err error
		result.Error, err = json.Marshal(*value.Error.Value)
		if err != nil {
			return result, err
		}
	}
	result.Items = append([]protocolv2.ThreadItem(nil), value.Items...)
	return result, nil
}

func productThreadFromSDK(value protocolv2.Thread) (productThread, error) {
	result := productThread{ID: value.ID, Model: sdkString(value.Model), Provider: value.ModelProvider,
		ParentThreadID: sdkString(value.ParentThreadID), ForkedFromID: sdkString(value.ForkedFromID)}
	if source, ok := value.Source.AsSubAgent(); ok {
		if spawn, ok := source.SubAgent.AsThreadSpawn(); ok {
			result.Source.SubAgent.ThreadSpawn.AgentPath = sdkString(spawn.AgentPath)
		}
	}
	for _, value := range value.Turns {
		turn, err := productTurnFromSDK(value)
		if err != nil {
			return result, err
		}
		result.Turns = append(result.Turns, turn)
	}
	return result, nil
}

func searchTurnFromSDK(value protocolv2.Turn) (turn, error) {
	result := turn{ID: value.ID, Status: string(value.Status)}
	if value.Error != nil && value.Error.Value != nil {
		var err error
		result.Error, err = json.Marshal(*value.Error.Value)
		if err != nil {
			return result, err
		}
	}
	for _, value := range value.Items {
		item := item{Type: string(value.Kind())}
		if message, ok := value.AsAgentMessage(); ok {
			item.Text = message.Text
			if message.Phase != nil && message.Phase.Value != nil {
				item.Phase = string(*message.Phase.Value)
			}
		}
		result.Items = append(result.Items, item)
	}
	return result, nil
}

func sdkString(value *protocolv2.Nullable[string]) string {
	if value != nil && value.Value != nil {
		return *value.Value
	}
	return ""
}
