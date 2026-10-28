package live

import (
	"context"
	"errors"
	"github.com/ronhuafeng/llm-go/codexsdk"
	"strings"
	"testing"
)

func TestImageFailureTransportKeepsSafeClassification(t *testing.T) {
	for _, tc := range []struct {
		err  error
		code string
	}{
		{&codexsdk.ProtocolError{Message: "PRIVATE_IMAGE_CANARY"}, "protocol_request_failed"},
		{codexsdk.ErrClientClosed, "protocol_ended_before_completion"},
		{errors.New("JSON-RPC frame too large: 100 bytes"), "protocol_frame_budget_exceeded"},
		{errors.New("PRIVATE_IMAGE_CANARY"), "sdk_observation_failed"},
	} {
		err := imageTransport(context.Background(), tc.err)
		if strings.Contains(err.Error(), "PRIVATE_IMAGE_CANARY") {
			t.Fatal("private transport error escaped")
		}
		if got := DescribeFailure(err); got.Code != tc.code {
			t.Fatalf("classification: %+v", got)
		}
	}
}
