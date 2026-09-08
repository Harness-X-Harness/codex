package driver

import (
	"context"
	"fmt"
	"sort"
	"sync"
	"time"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

// DynamicTool is a client-owned tool the validator offers to the model and
// answers itself. The answer is a fixed marker so a later Turn can prove that
// the tool output travelled through canonical history.
type DynamicTool struct {
	Name        string
	Description string
	Output      string
}

// Spec renders the tool as the app-server expects it at thread/start; the
// tool takes no arguments.
func (t DynamicTool) Spec() protocolv2.DynamicToolSpec {
	return protocolv2.NewDynamicToolSpecFunction(protocolv2.DynamicToolSpecFunction{
		Name:        t.Name,
		Description: t.Description,
		InputSchema: protocolv2.JSONObject(map[string]protocolv2.JSONValue{
			"type":                 protocolv2.JSONString("object"),
			"properties":           protocolv2.JSONObject(map[string]protocolv2.JSONValue{}),
			"additionalProperties": protocolv2.JSONBool(false),
		}),
	})
}

// ServerRequests answers app-server requests fail-closed and keeps counts for
// the evidence. Approvals are declined, dynamic tool calls are answered only
// for the configured tool, everything else fails the exact client.
type ServerRequests struct {
	mu            sync.Mutex
	tool          *DynamicTool
	byKind        map[string]int
	toolCalls     int
	unexpected    []string
	badArguments  int
	answeredTools int
}

// NewServerRequests builds the handler state; tool may be nil.
func NewServerRequests(tool *DynamicTool) *ServerRequests {
	return &ServerRequests{tool: tool, byKind: map[string]int{}}
}

// ToolRequestCount is the number of calls to the configured tool.
func (s *ServerRequests) ToolRequestCount() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.toolCalls
}

// Diagnostics returns secret-safe counts for the evidence file.
func (s *ServerRequests) Diagnostics() map[string]any {
	s.mu.Lock()
	defer s.mu.Unlock()
	kinds := map[string]int{}
	for kind, count := range s.byKind {
		kinds[kind] = count
	}
	unexpected := append([]string(nil), s.unexpected...)
	sort.Strings(unexpected)
	return map[string]any{
		"server_requests_by_kind":       kinds,
		"tool_request_count":            s.toolCalls,
		"tool_requests_answered":        s.answeredTools,
		"tool_requests_with_arguments":  s.badArguments,
		"unexpected_tool_request_names": unexpected,
	}
}

// Handler is the codexsdk ServerRequestHandler.
func (s *ServerRequests) Handler(_ context.Context, request protocolv2.ServerRequest) (codexsdk.ServerRequestResponse, error) {
	kind := request.Kind()
	s.mu.Lock()
	s.byKind[string(kind)]++
	s.mu.Unlock()
	switch kind {
	case protocolv2.ServerRequestKindItemToolCall:
		call, _ := request.AsItemToolCall()
		return s.answerTool(call.Params)
	case protocolv2.ServerRequestKindItemCommandExecutionRequestApproval:
		return codexsdk.CommandExecutionApprovalResponse(protocolv2.CommandExecutionRequestApprovalResponse{
			Decision: protocolv2.NewCommandExecutionApprovalDecisionDecline(),
		}), nil
	case protocolv2.ServerRequestKindItemFileChangeRequestApproval:
		return codexsdk.FileChangeApprovalResponse(protocolv2.FileChangeRequestApprovalResponse{
			Decision: protocolv2.FileChangeApprovalDecisionDecline,
		}), nil
	case protocolv2.ServerRequestKindItemToolRequestUserInput:
		return codexsdk.ToolUserInputResponse(protocolv2.ToolRequestUserInputResponse{
			Answers: map[string]protocolv2.ToolRequestUserInputAnswer{},
		}), nil
	case protocolv2.ServerRequestKindMCPServerElicitationRequest:
		return codexsdk.MCPElicitationResponse(protocolv2.McpServerElicitationRequestResponse{
			Action: protocolv2.McpServerElicitationActionDecline,
		}), nil
	case protocolv2.ServerRequestKindItemPermissionsRequestApproval:
		return codexsdk.PermissionsApprovalResponse(protocolv2.PermissionsRequestApprovalResponse{
			Permissions: protocolv2.GrantedPermissionProfile{},
		}), nil
	case protocolv2.ServerRequestKindCurrentTimeRead:
		return codexsdk.CurrentTimeResponse(protocolv2.CurrentTimeReadResponse{
			CurrentTimeAt: time.Now().UnixMilli(),
		}), nil
	case protocolv2.ServerRequestKindApplyPatchApproval:
		return codexsdk.ApplyPatchApprovalResponse(protocolv2.ApplyPatchApprovalResponse{
			Decision: protocolv2.NewReviewDecisionDenied(protocolv2.ReviewDecisionDenied{Rejection: "grokex-live declines approvals"}),
		}), nil
	case protocolv2.ServerRequestKindExecCommandApproval:
		return codexsdk.ExecCommandApprovalResponse(protocolv2.ExecCommandApprovalResponse{
			Decision: protocolv2.NewReviewDecisionDenied(protocolv2.ReviewDecisionDenied{Rejection: "grokex-live declines approvals"}),
		}), nil
	default:
		return codexsdk.ServerRequestResponse{}, fmt.Errorf("grokex-live has no answer for server request %s", kind)
	}
}

func (s *ServerRequests) answerTool(params protocolv2.DynamicToolCallParams) (codexsdk.ServerRequestResponse, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.tool == nil || params.Tool != s.tool.Name {
		s.unexpected = append(s.unexpected, params.Tool)
		return codexsdk.ServerRequestResponse{}, fmt.Errorf("unexpected dynamic tool request %q", params.Tool)
	}
	s.toolCalls++
	if arguments, ok := params.Arguments.AsObject(); !ok || len(arguments) != 0 {
		s.badArguments++
	}
	s.answeredTools++
	return codexsdk.DynamicToolResponse(protocolv2.DynamicToolCallResponse{
		ContentItems: []protocolv2.DynamicToolCallOutputContentItem{
			protocolv2.NewDynamicToolCallOutputContentItemInputText(protocolv2.DynamicToolCallOutputContentItemInputText{Text: s.tool.Output}),
		},
		Success: true,
	}), nil
}
