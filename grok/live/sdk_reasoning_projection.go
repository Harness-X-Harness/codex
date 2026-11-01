package live

import (
	"encoding/json"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func historyObservationFromSDK(notification protocolv2.ServerNotification) (sdkHistoryObservation, bool, error) {
	if completed, ok := notification.AsItemCompleted(); ok {
		params := completed.Params
		return sdkHistoryObservation{Method: "item/completed", ThreadID: params.ThreadID, TurnID: params.TurnID, Item: historyItemFromSDK(params.Item)}, true, nil
	}
	if completed, ok := notification.AsTurnCompleted(); ok {
		turn, err := historyTurnFromSDK(completed.Params.Turn)
		return sdkHistoryObservation{Method: "turn/completed", ThreadID: completed.Params.ThreadID, Turn: turn}, true, err
	}
	return sdkHistoryObservation{}, false, nil
}

func historyTurnFromSDK(value protocolv2.Turn) (historyTurn, error) {
	result := historyTurn{ID: value.ID, Status: string(value.Status)}
	if value.Error != nil && value.Error.Value != nil {
		// The shared bounded failure classifier consumes the error union as JSON.
		// Lifecycle identity, status and items come directly from the SDK.
		var err error
		result.Error, err = json.Marshal(*value.Error.Value)
		if err != nil {
			return result, err
		}
	}
	for _, item := range value.Items {
		result.Items = append(result.Items, historyItemFromSDK(item))
	}
	return result, nil
}

func historyItemFromSDK(value protocolv2.ThreadItem) historyItem {
	result := historyItem{Type: string(value.Kind())}
	if message, ok := value.AsAgentMessage(); ok {
		result.ID, result.Text = message.ID, message.Text
		if message.Phase != nil && message.Phase.Value != nil {
			result.Phase = string(*message.Phase.Value)
		}
	} else if reasoning, ok := value.AsReasoning(); ok {
		result.ID = reasoning.ID
	} else if tool, ok := value.AsDynamicToolCall(); ok {
		result.ID, result.Tool, result.Status = tool.ID, tool.Tool, string(tool.Status)
		if tool.Namespace != nil && tool.Namespace.Value != nil {
			namespace := *tool.Namespace.Value
			result.Namespace = &namespace
		}
		if tool.Success != nil && tool.Success.Value != nil {
			result.Success = *tool.Success.Value
		}
		if tool.ContentItems != nil && tool.ContentItems.Value != nil {
			for _, content := range *tool.ContentItems.Value {
				projected := item{Type: string(content.Kind())}
				if text, ok := content.AsInputText(); ok {
					projected.Text = text.Text
				}
				result.ContentItems = append(result.ContentItems, projected)
			}
		}
	}
	return result
}
