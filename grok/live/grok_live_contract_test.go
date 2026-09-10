package live

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

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
