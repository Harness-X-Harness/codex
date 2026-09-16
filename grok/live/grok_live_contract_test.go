package live

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/ronhuafeng/llm-go/codexsdk"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
)

func TestLastAgentMessageSkipsEmptyItems(t *testing.T) {
	items := []protocolv2.ThreadItem{
		protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "1", Text: ""}),
		protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: "2", Text: " visible "}),
	}
	if got := lastAgentMessage(items); got != "visible" {
		t.Fatalf("lastAgentMessage = %q", got)
	}
}

func TestStreamedTextLossDetectsDroppedDeltas(t *testing.T) {
	delta := func(itemID, text string) protocolv2.ServerNotification {
		return protocolv2.NewServerNotificationItemAgentMessageDelta(protocolv2.ServerNotificationItemAgentMessageDelta{
			Params: protocolv2.AgentMessageDeltaNotification{ItemID: itemID, Delta: text},
		})
	}
	message := func(itemID, text string) protocolv2.ThreadItem {
		return protocolv2.NewThreadItemAgentMessage(protocolv2.ThreadItemAgentMessage{ID: itemID, Text: text})
	}
	intact := codexsdk.ThreadRunResult{
		Notifications: []protocolv2.ServerNotification{delta("msg_1", "Let me "), delta("msg_1", "check.")},
	}
	intact.Turn.Items = []protocolv2.ThreadItem{message("msg_0", "never streamed"), message("msg_1", "Let me check.")}
	if id, _, _, lost := streamedTextLoss(intact); lost {
		t.Fatalf("intact stream reported loss on %s", id)
	}

	dropped := codexsdk.ThreadRunResult{
		Notifications: []protocolv2.ServerNotification{delta("msg_1", "Let me ")},
	}
	dropped.Turn.Items = []protocolv2.ThreadItem{message("msg_1", "Let me check.")}
	id, streamed, completed, lost := streamedTextLoss(dropped)
	if !lost || id != "msg_1" || streamed != 7 || completed != 13 {
		t.Fatalf("dropped stream: id=%q streamed=%d completed=%d lost=%t", id, streamed, completed, lost)
	}
}

func TestCompletedWithoutFinalAnswerIsNotARetrySignal(t *testing.T) {
	err := errors.New("codexsdk: turn completed without final_answer agent message")
	if !completedWithoutFinalAnswer(err, string(protocolv2.TurnStatusCompleted)) {
		t.Fatal("completed Grok Turn without final_answer should remain a completed Turn")
	}
	if completedWithoutFinalAnswer(err, string(protocolv2.TurnStatusFailed)) {
		t.Fatal("failed Turn must not be classified as completed")
	}
}

func TestNonceOfStripsCodeFence(t *testing.T) {
	got := nonceOf("```\n01234567-89ab-cdef-0123-456789abcdef\n```")
	if !uuidShaped.MatchString(got) {
		t.Fatalf("nonce %q is not UUID-shaped", got)
	}
}

func TestTopLevelTomlStringReadsQuotedKeysAndIgnoresTables(t *testing.T) {
	const fixture = "" +
		"model = \"fixture-model\"\n" +
		"model_provider = 'fixture-provider'\n" +
		"\n" +
		"[model_providers.grok]\n" +
		"model = \"nested-model\"\n" +
		"model_provider = \"nested-provider\"\n" +
		"name = \"Grok\"\n" +
		"base_url = \"https://example.invalid/v1\"\n"
	if got := topLevelTomlString([]byte(fixture), "model"); got != "fixture-model" {
		t.Fatalf("model = %q", got)
	}
	if got := topLevelTomlString([]byte(fixture), "model_provider"); got != "fixture-provider" {
		t.Fatalf("model_provider = %q", got)
	}
	if got := topLevelTomlString([]byte(fixture), "name"); got != "" {
		t.Fatalf("table key leaked as top-level: %q", got)
	}
	if got := tableString([]byte(fixture), "model_providers.grok", "base_url"); got != "https://example.invalid/v1" {
		t.Fatalf("table base_url = %q", got)
	}
	if got := tableString([]byte(fixture), "model_providers.grok", "name"); got != "Grok" {
		t.Fatalf("table name = %q", got)
	}
	if got := tableString([]byte(fixture), "model_providers.grok", "model_provider"); got != "nested-provider" {
		t.Fatalf("table model_provider = %q", got)
	}
	rewritten := setTableString([]byte(fixture), "model_providers.grok", "base_url", "http://127.0.0.1:9/v1")
	if got := tableString(rewritten, "model_providers.grok", "base_url"); got != "http://127.0.0.1:9/v1" {
		t.Fatalf("rewritten base_url = %q", got)
	}
	if got := topLevelTomlString(rewritten, "model"); got != "fixture-model" {
		t.Fatalf("rewrite changed top-level model: %q", got)
	}
}

func TestShippedGrokProfilePathIsReadable(t *testing.T) {
	path := shippedGrokProfilePath()
	if _, err := os.ReadFile(path); err != nil {
		t.Fatalf("shipped profile %s: %v", path, err)
	}
}

func TestEnsureShellToolDisabledIsIdempotent(t *testing.T) {
	first := ensureShellToolDisabled([]byte("model = \"grok-4.6\"\n"))
	second := ensureShellToolDisabled(first)
	if string(first) != string(second) {
		t.Fatal("disabling shell_tool twice changed the config")
	}
}

func TestTailFileWriterKeepsOnlyBoundedSuffix(t *testing.T) {
	path := filepath.Join(t.TempDir(), "stderr")
	writer, err := newTailFileWriter(path, 8)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := writer.Write([]byte("12345")); err != nil {
		t.Fatal(err)
	}
	if _, err := writer.Write([]byte("67890")); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "34567890" {
		t.Fatalf("tail = %q", got)
	}
}

func TestSecretRedactorRemovesConfigEnvAndHeaderCredentials(t *testing.T) {
	t.Setenv("TEST_API_KEY", "ENV-SECRET-123456")
	redactor := newSecretRedactor([]byte("api_key = \"CONFIG-SECRET-987654\"\nmodel = \"grok-4.6\"\n"))
	input := "env=ENV-SECRET-123456 config=CONFIG-SECRET-987654 Authorization: Bearer bearer-secret token=plain-secret model=grok-4.6"
	got := redactor.redact(input)
	for _, secret := range []string{"ENV-SECRET-123456", "CONFIG-SECRET-987654", "bearer-secret", "plain-secret"} {
		if strings.Contains(got, secret) {
			t.Fatalf("redacted output retained %q: %s", secret, got)
		}
	}
	if !strings.Contains(got, "model=grok-4.6") {
		t.Fatalf("redaction removed non-secret diagnostic fact: %s", got)
	}
}

func TestSensitiveNameIsNarrowlyCredentialOriented(t *testing.T) {
	for _, name := range []string{"GROK_API_KEY", "AUTH_TOKEN", "client_secret", "PASSWORD", "credential_file"} {
		if !sensitiveName(name) {
			t.Fatalf("%s should be sensitive", name)
		}
	}
	for _, name := range []string{"GROK_MODEL", "CODEX_HOME", "RUN_ID"} {
		if sensitiveName(name) {
			t.Fatalf("%s should not be sensitive", name)
		}
	}
}

func TestDurableEncryptedReasoningIsPresenceOnly(t *testing.T) {
	home := t.TempDir()
	dir := filepath.Join(home, "sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	line := "{\"type\":\"response_item\",\"payload\":{\"type\":\"reasoning\",\"encrypted_content\":\"SECRET\"}}\n"
	if err := os.WriteFile(filepath.Join(dir, "rollout.jsonl"), []byte(line), 0o600); err != nil {
		t.Fatal(err)
	}
	if !durableHasEncryptedReasoning(home) {
		t.Fatal("expected encrypted reasoning presence")
	}
}

func TestApplyPatchObservedAcceptsFileChange(t *testing.T) {
	items := []protocolv2.ThreadItem{
		protocolv2.NewThreadItemFileChange(protocolv2.ThreadItemFileChange{
			ID: "1", Status: protocolv2.PatchApplyStatusCompleted, Changes: []protocolv2.FileUpdateChange{},
		}),
	}
	if !applyPatchObserved(items) {
		t.Fatal("completed file_change should prove the apply_patch path")
	}
	if hasCommandExecution(items) {
		t.Fatal("file_change must not count as a shell path")
	}
}

func TestScanDurableFactsIgnoresPromptText(t *testing.T) {
	home := t.TempDir()
	dir := filepath.Join(home, "sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	promptOnly := "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"use apply_patch and exec_command\"}]}}\n"
	call := "{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call\",\"name\":\"apply_patch\",\"call_id\":\"c1\"}}\n"
	if err := os.WriteFile(filepath.Join(dir, "prompt.jsonl"), []byte(promptOnly), 0o600); err != nil {
		t.Fatal(err)
	}
	facts := scanDurableFacts(home)
	if facts.applyPatchCall || facts.commandExecution {
		t.Fatal("prompt text must not prove a tool path")
	}
	if err := os.WriteFile(filepath.Join(dir, "call.jsonl"), []byte(call), 0o600); err != nil {
		t.Fatal(err)
	}
	facts = scanDurableFacts(home)
	if !facts.applyPatchCall {
		t.Fatal("custom_tool_call named apply_patch should prove the path")
	}
}

func TestCopyRedactedSessionJSONLKeepsOnlySessions(t *testing.T) {
	home := t.TempDir()
	sessions := filepath.Join(home, "sessions", "2026", "09", "15")
	images := filepath.Join(home, "generated_images", "sess")
	if err := os.MkdirAll(sessions, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(images, 0o700); err != nil {
		t.Fatal(err)
	}
	secret := "LIVE-SECRET-123456"
	line := `{"api_key":"` + secret + `","type":"event"}` + "\n"
	if err := os.WriteFile(filepath.Join(sessions, "rollout.jsonl"), []byte(line), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(home, "config.toml"), []byte("api_key = \""+secret+"\"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(images, "call.png"), []byte("PNG"), 0o600); err != nil {
		t.Fatal(err)
	}
	dest := t.TempDir()
	t.Setenv(grokLiveFailedSessionsEnv, dest)
	copyRedactedSessionJSONL(t, home, newSecretRedactor([]byte("api_key = \""+secret+"\"\n")))
	copied := filepath.Join(dest, strings.ReplaceAll(t.Name(), "/", "_"), "2026", "09", "15", "rollout.jsonl")
	got, err := os.ReadFile(copied)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(got), secret) {
		t.Fatalf("copied jsonl retained secret: %s", got)
	}
	if !strings.Contains(string(got), "[REDACTED]") {
		t.Fatalf("copied jsonl missing redaction: %s", got)
	}
	if _, err := os.Stat(filepath.Join(dest, strings.ReplaceAll(t.Name(), "/", "_"), "config.toml")); err == nil {
		t.Fatal("config.toml must not be copied")
	}
	if _, err := os.Stat(filepath.Join(dest, strings.ReplaceAll(t.Name(), "/", "_"), "generated_images", "sess", "call.png")); err == nil {
		t.Fatal("generated images must not be copied")
	}
}

func TestPreserveFailedSessionsSkipsPassingTests(t *testing.T) {
	home := t.TempDir()
	sessions := filepath.Join(home, "sessions")
	if err := os.MkdirAll(sessions, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(sessions, "rollout.jsonl"), []byte("{\"type\":\"event\"}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	dest := t.TempDir()
	t.Setenv(grokLiveFailedSessionsEnv, dest)
	rec := newWireRecorder()
	rec.add(wireExchange{
		method: "POST", path: "/v1/responses", status: 400,
		requestBody: []byte(`{"tools":[{"external_web_access":true}]}`), responseBody: []byte("rejected"),
	})
	preserveFailedSessions(t, home, newSecretRedactor(nil), rec)
	entries, err := os.ReadDir(dest)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 0 {
		t.Fatal("passing tests must not copy session jsonl or wire captures")
	}
}

func TestLiveCodexHomeLivesInWorkspaceUnderHOME(t *testing.T) {
	parent := t.TempDir()
	t.Setenv("HOME", parent)
	workspace := liveWorkspaceDir(t)
	home := liveCodexHome(t, workspace)
	rel, err := filepath.Rel(parent, workspace)
	if err != nil || rel == "." || strings.HasPrefix(rel, "..") {
		t.Fatalf("workspace %q is not under HOME %q", workspace, parent)
	}
	if filepath.Dir(home) != workspace || filepath.Base(home) != ".codex" {
		t.Fatalf("CODEX_HOME %q should be workspace/.codex, workspace=%q", home, workspace)
	}
}

func TestCommandExecutionApprovalAcceptsForSession(t *testing.T) {
	got := commandExecutionApproval(true)
	if _, ok := got.AsAcceptForSession(); !ok {
		t.Fatalf("decision kind = %s, want acceptForSession", got.Kind())
	}
	declined := commandExecutionApproval(false)
	if _, ok := declined.AsDecline(); !ok {
		t.Fatalf("decision kind = %s, want decline", declined.Kind())
	}
}
