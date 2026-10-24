package live

import (
	"bytes"
	"context"
	"errors"
	"github.com/ronhuafeng/llm-go/codexsdk/protocolv2"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"testing"
)

func TestImageSDKAfterCloseRechecksCreditedArtifacts(t *testing.T) {
	for _, changed := range []string{"", "first", "edited"} {
		t.Run("changed_"+changed, func(t *testing.T) {
			root := t.TempDir()
			write := func(name string, red uint8) (string, string) {
				picture := image.NewRGBA(image.Rect(0, 0, 1, 1))
				picture.SetRGBA(0, 0, color.RGBA{R: red, A: 255})
				var data bytes.Buffer
				if err := png.Encode(&data, picture); err != nil {
					t.Fatal(err)
				}
				path := filepath.Join(root, name+".png")
				if err := os.WriteFile(path, data.Bytes(), 0600); err != nil {
					t.Fatal(err)
				}
				return path, editDigest(data.Bytes())
			}
			first, firstDigest := write("first", 10)
			edited, editedDigest := write("edited", 20)
			evidence := &ImageEvidence{FirstImageSHA256: firstDigest, EditedImageSHA256: editedDigest, TurnCompleted: true}
			base := &Evidence{Completed: true, Stage: "image_edit_verified"}
			probe := &imageProbe{root: root, evidence: evidence, prior: &imageCall{path: first}, newest: &imageCall{path: edited}}
			if changed != "" {
				write(changed, 30)
			}
			err := probe.afterClose("", base, nil)
			if (err != nil) != (changed != "") || base.Completed != (changed == "") {
				t.Fatalf("credited artifact readback mismatch: completed=%v err=%v", base.Completed, err)
			}
		})
	}
}

func TestImageSDKAfterCloseRejectsLateContradiction(t *testing.T) {
	evidence := &ImageEvidence{
		TurnCompleted: true, FirstImageSHA256: "first", FirstHistorySHA256: "first-history",
		EditedImageSHA256: "edited", EditedHistorySHA256: "edited-history",
	}
	base := &Evidence{Completed: true, FirstCompleted: true, Stage: "image_edit_verified"}
	probe := &imageProbe{ctx: context.Background(), evidence: evidence, base: base, threadID: "thread", turnID: "edit", changed: make(chan struct{}, 1)}
	late := protocolv2.NewServerNotificationTurnCompleted(protocolv2.ServerNotificationTurnCompleted{
		Params: protocolv2.TurnCompletedNotification{ThreadID: "thread", Turn: protocolv2.Turn{ID: "edit", Status: protocolv2.TurnStatusFailed}},
	})
	if err := probe.typedNotification(context.Background(), late); err != nil || probe.cause == nil {
		t.Fatal("late contradictory callback did not retain its failure")
	}
	if err := probe.afterClose("", base, nil); err == nil || base.Completed || evidence.TurnCompleted || base.Stage == "image_edit_verified" || evidence.EditedImageSHA256 != "" || evidence.EditedHistorySHA256 != "" {
		t.Fatal("joined late contradiction retained completed image credit")
	}
	if !base.FirstCompleted || evidence.FirstImageSHA256 != "first" || evidence.FirstHistorySHA256 != "first-history" {
		t.Fatal("late edit contradiction erased earlier generation facts")
	}
}

func TestImageSDKAfterCloseCannotPromoteOrHideRunFailure(t *testing.T) {
	for _, cause := range []error{nil, errors.New("controlled run failure")} {
		base := &Evidence{Completed: false, FirstCompleted: true, Stage: "image_generation_verified"}
		probe := &imageProbe{evidence: &ImageEvidence{}}
		if err := probe.afterClose("", base, cause); err != nil || base.Completed || !base.FirstCompleted {
			t.Fatal("cleanup created completion or erased partial facts")
		}
	}
}

func TestImageTypedUnrelatedToolsGrantNoEvidence(t *testing.T) {
	for _, arguments := range []protocolv2.JSONValue{protocolv2.JSONNull(), protocolv2.JSONString("argument"), protocolv2.JSONObject(map[string]protocolv2.JSONValue{}), protocolv2.JSONArray(nil), protocolv2.JSONBool(false)} {
		values := []protocolv2.ThreadItem{
			protocolv2.NewThreadItemDynamicToolCall(protocolv2.ThreadItemDynamicToolCall{ID: "call", Arguments: arguments, Status: protocolv2.DynamicToolCallStatusCompleted}),
			protocolv2.NewThreadItemMCPToolCall(protocolv2.ThreadItemMCPToolCall{ID: "call", Arguments: arguments, Status: protocolv2.McpToolCallStatusCompleted}),
		}
		for _, value := range values {
			item, err := imageItemFromSDK(value)
			if err != nil {
				t.Fatal(err)
			}
			probe := &imageProbe{ctx: context.Background(), calls: map[string]*imageCall{}, evidence: &ImageEvidence{}}
			if err := probe.public(item, false); err != nil || len(probe.calls) != 0 || probe.evidence.Images != 0 || probe.evidence.HistorySelected {
				t.Fatal("unrelated typed tool supplied image evidence")
			}
		}
	}
}
