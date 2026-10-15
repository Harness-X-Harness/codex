package live

import (
	"bytes"
	"encoding/json"
	"io"
	"strings"
	"sync"
	"testing"
)

func TestFilterStdoutDropsPublicXSearch(t *testing.T) {
	frames := []string{
		`{"method":"item/started","params":{"completedAtMs":1,"item":{"callId":"PRIVATE_CALL","id":"PRIVATE_ID","input":"PRIVATE_INPUT","name":"x_keyword_search","type":"xSearch"},"threadId":"thread","turnId":"turn1"}}`,
		`{"method":"item/completed","params":{"completedAtMs":1,"item":{"callId":"PRIVATE_CALL","id":"PRIVATE_ID","input":"PRIVATE_INPUT","name":"x_keyword_search","type":"xSearch"},"threadId":"thread","turnId":"turn1"}}`,
		`{"method":"rawResponseItem/completed","params":{"item":{"call_id":"PRIVATE_CALL","id":"PRIVATE_ID","input":"PRIVATE_INPUT","name":"x_keyword_search","status":"completed","type":"custom_tool_call"},"threadId":"thread","turnId":"turn1"}}`,
		`{"method":"turn/completed","params":{"threadId":"thread","turn":{"id":"turn1","items":[{"id":"reply","phase":"final_answer","text":"PRIVATE_REPLY","type":"agentMessage"},{"callId":"PRIVATE_CALL","id":"PRIVATE_ID","input":"PRIVATE_INPUT","name":"x_keyword_search","type":"xSearch"}],"status":"completed"}}}`,
		`{"method":"item/completed","params":{"completedAtMs":1,"item":{"id":"web","query":"q","type":"webSearch"},"threadId":"thread","turnId":"turn1"}}`,
	}
	reader, writer := io.Pipe()
	var writers sync.WaitGroup
	writers.Add(1)
	go func() {
		defer writers.Done()
		defer writer.Close()
		_, _ = io.WriteString(writer, strings.Join(frames, "\n")+"\n")
	}()
	var forwarded bytes.Buffer
	if err := filterStdout(reader, &forwarded); err != nil {
		t.Fatal(err)
	}
	writers.Wait()
	lines := strings.Split(strings.TrimSuffix(forwarded.String(), "\n"), "\n")
	if len(lines) != 3 {
		t.Fatalf("forwarded frames: %q", forwarded.String())
	}
	if lines[0] != frames[2] || lines[2] != frames[4] {
		t.Fatalf("raw or web frame changed: %q", forwarded.String())
	}
	var completed struct {
		Params struct {
			Turn struct {
				Items []struct {
					Type string `json:"type"`
				} `json:"items"`
			} `json:"turn"`
		} `json:"params"`
	}
	if json.Unmarshal([]byte(lines[1]), &completed) != nil || len(completed.Params.Turn.Items) != 1 || completed.Params.Turn.Items[0].Type != "agentMessage" {
		t.Fatalf("turn items: %s", lines[1])
	}
	if strings.Contains(forwarded.String(), `"xSearch"`) {
		t.Fatalf("public xSearch forwarded: %s", forwarded.String())
	}
}
