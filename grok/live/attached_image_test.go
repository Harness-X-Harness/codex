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

func TestAttachedImageStructuredFacts(t *testing.T) {
	for _, text := range []string{`{"color":"red","shape":"circle"}`, `{"shape":"circle","color":"RED"}`, `{"color":"red","shape":"circular"}`, ` {"color":" red ","shape":"circle"} `} {
		var answer attachedImageProperties
		if err := parseStructuredReply(text, &answer, "color", "shape"); err != nil || !answer.matches("red", "circle") {
			t.Fatalf("valid visual facts rejected: %s: %v", text, err)
		}
	}
	for _, text := range []string{`{"color":null,"shape":"circle"}`, `{"color":"red","shape":null}`, `{"color":"green","shape":"triangle"}`, `{"color":"not red","shape":"circle"}`, `{"color":"red","shape":"not circle"}`} {
		var answer attachedImageProperties
		if err := parseStructuredReply(text, &answer, "color", "shape"); err != nil {
			t.Fatalf("schema-valid nonmatch failed parsing: %v", err)
		}
		if answer.matches("red", "circle") {
			t.Fatalf("unknown or wrong visual facts accepted: %s", text)
		}
	}
}
