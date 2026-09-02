package rollout

import (
	"path/filepath"
	"reflect"
	"testing"
)

// The fixtures are real rollouts written by the stock 0.151 recorder under a
// Grok profile (mock provider): a paginated app-server thread that generated
// and edited an image, and a legacy exec thread whose Ultra Turn spawned one
// full-history child.

const (
	imageRoot    = "01a062cb-2f05-74b3-981e-7a01ab0c5fc1"
	imageTurnGen = "01a062cb-2f59-7b03-87a2-8a932b79e846"
	imageTurnEd  = "01a062cb-316c-70e3-887a-b61b4d9bbe42"
	collabRoot   = "01a062cd-189e-7b21-bb5c-9988de260c79"
	collabRootT  = "01a062cd-18bb-71f3-9b11-224bd51b1a30"
	collabChild  = "01a062cd-1914-71b3-95a5-0e75b761d563"
	collabChildT = "01a062cd-1939-7a43-b5b5-3e209555bac1"
	nonce        = "3f2b1c9e-8d4a-4b6f-9a1e-7c5d2e8f0a11"
)

func TestScanPaginatedImageThread(t *testing.T) {
	graph, err := Scan(filepath.Join("testdata", "image"))
	if err != nil {
		t.Fatal(err)
	}
	if len(graph.Sessions) != 1 {
		t.Fatalf("sessions = %d, want 1", len(graph.Sessions))
	}
	session := graph.Sessions[0]
	if session.ID != imageRoot || session.ModelProvider != "grok" || session.HistoryMode != "paginated" || session.SourceKind != "vscode" {
		t.Fatalf("session identity = %+v", *session)
	}
	if len(session.Turns) != 2 || session.Turns[0].ID != imageTurnGen || session.Turns[1].ID != imageTurnEd {
		t.Fatalf("turns = %+v", session.Turns)
	}
	generation, edit := session.Turns[0], session.Turns[1]
	for _, turn := range []*Turn{generation, edit} {
		if !turn.Started || !turn.Completed || turn.Failed || turn.Model != "grok-4.6" || turn.MultiAgentVersion != "v2" {
			t.Fatalf("turn lifecycle = %+v", *turn)
		}
	}
	if generation.LastAgentMessage != "Generated the image." || edit.LastAgentMessage != "Edited the image." {
		t.Fatalf("last messages = %q %q", generation.LastAgentMessage, edit.LastAgentMessage)
	}
	if got := generation.FunctionCallCounts(); !reflect.DeepEqual(got, map[string]int{"image_gen.imagegen": 1}) {
		t.Fatalf("generation calls = %v", got)
	}
	if len(generation.ImageResults) != 1 || generation.ImageResults[0].CallID != "generate-1" || generation.ImageResults[0].Status != "completed" || filepath.Base(generation.ImageResults[0].SavedPath) != "generate-1.jpg" {
		t.Fatalf("generation image results = %+v", generation.ImageResults)
	}
	call, ok := edit.FunctionCall("edit-1")
	if !ok || call.Arguments != `{"prompt":"make the circle green","num_last_images_to_include":1}` {
		t.Fatalf("edit call = %+v ok=%v", call, ok)
	}
	if len(edit.ImageResults) != 1 || edit.ImageResults[0].CallID != "edit-1" {
		t.Fatalf("edit image results = %+v", edit.ImageResults)
	}
	if !reflect.DeepEqual(generation.AgentMessages, []string{"Generated the image."}) {
		t.Fatalf("agent messages deduplicated = %v", generation.AgentMessages)
	}
}

func TestScanLegacyCollaborationGraph(t *testing.T) {
	graph, err := Scan(filepath.Join("testdata", "collab"))
	if err != nil {
		t.Fatal(err)
	}
	if len(graph.Sessions) != 2 {
		t.Fatalf("sessions = %d, want 2", len(graph.Sessions))
	}
	root, ok := graph.Session(collabRoot)
	if !ok || root.ParentThreadID != "" || root.HistoryMode != "legacy" || root.ModelProvider != "grok" {
		t.Fatalf("root = %+v", root)
	}
	children := graph.Children(collabRoot)
	if len(children) != 1 || children[0].ID != collabChild {
		t.Fatalf("children = %+v", children)
	}
	child := children[0]
	if child.ForkedFromID != collabRoot || child.SourceKind != "subagent" || !reflect.DeepEqual(child.InheritedMetaIDs, []string{collabRoot}) {
		t.Fatalf("child fork identity = %+v", *child)
	}

	rootTurn, ok := root.Turn(collabRootT)
	if !ok || !rootTurn.Completed || rootTurn.Effort != "ultra" || rootTurn.MultiAgentVersion != "v2" || rootTurn.LastAgentMessage != nonce {
		t.Fatalf("root turn = %+v", rootTurn)
	}
	if got := rootTurn.FunctionCallCounts(); !reflect.DeepEqual(got, map[string]int{"collaboration.spawn_agent": 1, "collaboration.wait_agent": 1}) {
		t.Fatalf("root calls = %v", got)
	}
	if !reflect.DeepEqual(rootTurn.SubAgentCompletions, []string{collabChild}) {
		t.Fatalf("sub-agent completions = %v", rootTurn.SubAgentCompletions)
	}

	// The child file replays the parent's head (its Turn never completes there)
	// before recording the child's own completed Turn.
	completed := child.CompletedTurns()
	if len(completed) != 1 || completed[0].ID != collabChildT || completed[0].LastAgentMessage != nonce || completed[0].Model != "grok-4.6" {
		t.Fatalf("child completed turns = %+v", completed)
	}
	if inherited, ok := child.Turn(collabRootT); !ok || inherited.Completed {
		t.Fatalf("inherited parent turn = %+v ok=%v", inherited, ok)
	}
}

func TestScanMissingSessionsDirIsEmpty(t *testing.T) {
	graph, err := Scan(t.TempDir())
	if err != nil || len(graph.Sessions) != 0 {
		t.Fatalf("graph=%+v err=%v", graph, err)
	}
}
