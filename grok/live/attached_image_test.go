package live

import (
	"bytes"
	"encoding/base64"
	"image/color"
	"image/png"
	"strings"
	"testing"
)

func TestAttachedImagePixelsAndUnleakedPrompts(t *testing.T) {
	url, err := attachedImageURL()
	if err != nil {
		t.Fatal(err)
	}
	again, _ := attachedImageURL()
	if url != again {
		t.Fatal("fixture is not deterministic")
	}
	raw, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(url, "data:image/png;base64,"))
	if err != nil {
		t.Fatal(err)
	}
	picture, err := png.Decode(bytes.NewReader(raw))
	if err != nil {
		t.Fatal(err)
	}
	for _, point := range []struct {
		x, y int
		want color.RGBA
	}{
		{120, 120, color.RGBA{R: 255, A: 255}}, {40, 40, color.RGBA{R: 255, G: 255, B: 255, A: 255}},
		{360, 120, color.RGBA{B: 255, A: 255}}, {280, 40, color.RGBA{B: 255, A: 255}},
	} {
		if color.RGBAModel.Convert(picture.At(point.x, point.y)) != point.want {
			t.Fatal("visual fixture changed")
		}
	}
	for _, answer := range []string{"red", "circle", "blue", "square"} {
		if strings.Contains(attachedImageFirstPrompt+attachedImageNextPrompt, answer) {
			t.Fatal("prompt leaks image answer")
		}
	}
}

func TestAttachedImageAnswerRejectsFalseAcceptance(t *testing.T) {
	for _, text := range []string{"red circle", "Red Circle.", "RED, CIRCLE"} {
		if !attachedImageAnswer(text, "red circle") {
			t.Fatal("equivalent semantic answer rejected")
		}
	}
	for _, text := range []string{"", "not red circle", "red circle blue square", "blue square", "red circular", "I cannot see the image; red circle"} {
		if attachedImageAnswer(text, "red circle") {
			t.Fatal("false visual proof accepted")
		}
	}
}
