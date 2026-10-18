package live

import (
	"encoding/json"

	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// frame retains SDK notifications and opaque raw extension parameters only.
// Server requests are admitted separately by codexsdk.ServerRequestHandler.
type frame struct {
	size         int
	Method       string
	Params       json.RawMessage
	Notification protocolv2.ServerNotification
}
