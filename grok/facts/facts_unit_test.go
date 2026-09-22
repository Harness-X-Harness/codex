package facts

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func readTestdata(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", name))
	if err != nil {
		t.Fatal(err)
	}
	return data
}

func TestProfileFromTOMLFixture(t *testing.T) {
	prof, err := profileFromTOML(readTestdata(t, "profile.toml"))
	if err != nil {
		t.Fatal(err)
	}
	want := profile{model: "fixture-model", baseURL: "https://example.invalid/v1"}
	if prof != want {
		t.Fatalf("profileFromTOML = %+v, want %+v", prof, want)
	}
}

func TestTopLevelTomlStringIgnoresTablesAndComments(t *testing.T) {
	data := readTestdata(t, "profile.toml")
	if got := topLevelTomlString(data, "model"); got != "fixture-model" {
		t.Fatalf("model = %q", got)
	}
	if got := topLevelTomlString(data, "model_provider"); got != "fixture-provider" {
		t.Fatalf("model_provider = %q", got)
	}
	if got := topLevelTomlString(data, "name"); got != "" {
		t.Fatalf("table key leaked as top-level: %q", got)
	}
	if got := topLevelTomlString(data, "base_url"); got != "" {
		t.Fatalf("table base_url leaked as top-level: %q", got)
	}
}

func TestTableStringReadsQuotedKeysAndStopsAtNextTable(t *testing.T) {
	data := readTestdata(t, "profile.toml")
	if got := tableString(data, "model_providers.grok", "base_url"); got != "https://example.invalid/v1/" {
		t.Fatalf("table base_url = %q", got)
	}
	if got := tableString(data, "model_providers.grok", "name"); got != "Grok" {
		t.Fatalf("table name = %q", got)
	}
	if got := tableString(data, "model_providers.grok", "model_provider"); got != "nested-provider" {
		t.Fatalf("table model_provider = %q", got)
	}
	if got := tableString(data, "other", "base_url"); got != "https://other.invalid/" {
		t.Fatalf("other base_url = %q", got)
	}
	if got := tableString(data, "missing", "base_url"); got != "" {
		t.Fatalf("missing table = %q", got)
	}
}

func TestLoadShippedProfile(t *testing.T) {
	prof, err := loadShippedProfile()
	if err != nil {
		t.Fatal(err)
	}
	if prof.model == "" || prof.baseURL == "" {
		t.Fatalf("empty shipped profile model=%q baseURL=%q", prof.model, prof.baseURL)
	}
	if strings.HasSuffix(prof.baseURL, "/") {
		t.Fatalf("baseURL should not have a trailing slash: %q", prof.baseURL)
	}
}

func TestProfileFromTOMLMissingFields(t *testing.T) {
	_, err := profileFromTOML([]byte("model = \"only-model\"\n"))
	if err == nil {
		t.Fatal("expected error for missing base_url")
	}
}

func TestClassifyAcceptedOn2xx(t *testing.T) {
	if got := classify(200, []byte(`{}`)); got != classAccepted {
		t.Fatalf("got %s", got)
	}
	if got := classify(204, nil); got != classAccepted {
		t.Fatalf("204: got %s", got)
	}
}

func TestClassifyExternalWebAccessFixture(t *testing.T) {
	got := classify(400, readTestdata(t, "error_external_web_access.json"))
	want := class("rejected:400/Argument not supported: external_web_access")
	if got != want {
		t.Fatalf("got %q want %q", got, want)
	}
}

func TestClassifyCompactionBlobKeepsWholeMessageUnderLimit(t *testing.T) {
	got := classify(400, readTestdata(t, "error_compaction_blob.json"))
	want := class("rejected:400/Could not decode the compaction blob. Ensure it is unmodified from the compact response.")
	if got != want {
		t.Fatalf("got %q want %q", got, want)
	}
	long := []byte(`{"error":"` + strings.Repeat("x", errorPrefixLimit+40) + `"}`)
	prefix := strings.TrimPrefix(string(classify(400, long)), "rejected:400/")
	if n := len([]rune(prefix)); n != errorPrefixLimit {
		t.Fatalf("prefix length %d, want %d", n, errorPrefixLimit)
	}
}

func TestErrorPrefixPrefersErrorOverMessageAndDropsSecrets(t *testing.T) {
	body := []byte(`{"error":"Argument not supported: external_web_access","message":"other","api_key":"secret"}`)
	got := classify(400, body)
	want := class("rejected:400/Argument not supported: external_web_access")
	if got != want {
		t.Fatalf("got %q want %q", got, want)
	}
	if strings.Contains(string(got), "secret") {
		t.Fatalf("class leaked secret: %q", got)
	}
}

func TestErrorPrefixUsesMessageFieldAndObjectMessage(t *testing.T) {
	if got := errorPrefix([]byte(`{"message":"top-level message only"}`)); got != "top-level message only" {
		t.Fatalf("message field = %q", got)
	}
	if got := errorPrefix([]byte(`{"error":{"message":"nested object message"}}`)); got != "nested object message" {
		t.Fatalf("error.message = %q", got)
	}
	if got := errorPrefix([]byte(`not json`)); got != "" {
		t.Fatalf("non-json = %q", got)
	}
}

func TestErrorPrefixCollapsesWhitespaceThenCuts(t *testing.T) {
	body := []byte(`{"error":"  Argument   not\nsupported:   external_web_access  "}`)
	got := classify(400, body)
	want := class("rejected:400/Argument not supported: external_web_access")
	if got != want {
		t.Fatalf("got %q want %q", got, want)
	}
}

func TestObserveEncryptedReasoning(t *testing.T) {
	if got := observeEncryptedReasoning(readTestdata(t, "output_encrypted_reasoning.json")); got != classAccepted {
		t.Fatalf("encrypted = %s", got)
	}
	if got := observeEncryptedReasoning(readTestdata(t, "output_no_encrypted_reasoning.json")); got != classIgnored {
		t.Fatalf("empty blob = %s", got)
	}
	if got := observeEncryptedReasoning([]byte(`{"output":[{"type":"message"}]}`)); got != classIgnored {
		t.Fatalf("no reasoning = %s", got)
	}
}

func TestApplyProfileModelKeepsExplicitProbeModel(t *testing.T) {
	body := map[string]any{"model": "grok-build"}
	applyProfileModel(body, "grok-4.6")
	if body["model"] != "grok-build" {
		t.Fatalf("explicit model replaced: %#v", body["model"])
	}
	omitted := map[string]any{}
	applyProfileModel(omitted, "grok-4.6")
	if omitted["model"] != "grok-4.6" {
		t.Fatalf("profile model = %#v", omitted["model"])
	}
}

func TestResponseModelAndFunctionCallFixture(t *testing.T) {
	body := readTestdata(t, "output_function_call.json")
	if got := responseModel(body); got != "grok-4.7" {
		t.Fatalf("response model = %q", got)
	}
	call := firstFunctionCall(body)
	if call == nil || jsonString(call["call_id"]) != "call_fact" || jsonString(call["name"]) != "fact_echo" {
		t.Fatalf("function call = %#v", call)
	}
	if got := responseModel([]byte(`{"output":[]}`)); got != "" {
		t.Fatalf("missing model = %q", got)
	}
}

func TestModelRouteClassPinsOnlyExactModels(t *testing.T) {
	build := modelRouteObservation{
		requested: "grok-build", responseModel: "grok-4.7", pinResponse: false,
		text: "accepted", reasoning: "accepted", tool: "accepted", history: "accepted",
	}
	if got, want := build.asClass(), class("requested=grok-build;response_model=present;text=accepted;reasoning=accepted;tool=accepted;history=accepted"); got != want {
		t.Fatalf("build class = %q", got)
	}
	pinned := modelRouteObservation{
		requested: "grok-4.7", responseModel: "grok-4.7", pinResponse: true,
		text: "accepted", reasoning: "accepted", tool: "accepted", history: "accepted",
		encryptedReplay: "accepted",
	}
	if got, want := pinned.asClass(), class("requested=grok-4.7;response_model=grok-4.7;text=accepted;reasoning=accepted;tool=accepted;history=accepted;encrypted_replay=accepted"); got != want {
		t.Fatalf("pinned class = %q", got)
	}
}

func TestRedactReplacesSecret(t *testing.T) {
	if got := redact("err key=abc remaining", "abc"); got != "err key=[REDACTED] remaining" {
		t.Fatalf("got %q", got)
	}
	if got := redact("no secret", ""); got != "no secret" {
		t.Fatalf("empty secret = %q", got)
	}
}
