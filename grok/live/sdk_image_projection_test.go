package live

import (
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"hash/crc32"
	"image"
	"image/jpeg"
	"image/png"
	"testing"
)

func TestImageRecognitionChecksFactsNotSentenceTemplate(t *testing.T) {
	for _, text := range []string{"A dog.", "The image depicts a happy dog sitting outside.", "DOG", "A dog with no accessories.", "A dog, not wearing anything."} {
		if !imageRecognized(text, false) {
			t.Fatalf("valid answer rejected: %s", text)
		}
	}
	for _, text := range []string{"A dog wearing a bright red hat.", "The red hat is on the dog's head.", "A dog wearing a red hat, with no other accessories."} {
		if !imageRecognized(text, true) {
			t.Fatalf("valid edit rejected: %s", text)
		}
	}
	for _, text := range []string{"No dog.", "A hotdog.", "A cat.", "Possibly a dog.", "A dog without a hat.", "A dog with a blue hat.", "A dog, not wearing a red hat."} {
		if imageRecognized(text, true) {
			t.Fatalf("wrong edit accepted: %s", text)
		}
	}
}

func TestImagePayloadRequiresBoundedCompletePNG(t *testing.T) {
	var pngBytes, jpegBytes bytes.Buffer
	canvas := image.NewRGBA(image.Rect(0, 0, 2, 2))
	if err := png.Encode(&pngBytes, canvas); err != nil {
		t.Fatal(err)
	}
	if err := jpeg.Encode(&jpegBytes, canvas, nil); err != nil {
		t.Fatal(err)
	}
	valid := pngBytes.Bytes()
	oversized := append([]byte(nil), valid...)
	binary.BigEndian.PutUint32(oversized[16:20], 100000)
	binary.BigEndian.PutUint32(oversized[20:24], 100000)
	binary.BigEndian.PutUint32(oversized[29:33], crc32.ChecksumIEEE(oversized[12:29]))
	if _, err := imagePayload(base64.StdEncoding.EncodeToString(valid)); err != nil {
		t.Fatal(err)
	}
	for _, bad := range [][]byte{nil, jpegBytes.Bytes(), valid[:len(valid)/2], oversized} {
		if _, err := imagePayload(base64.StdEncoding.EncodeToString(bad)); err == nil {
			t.Fatal("invalid or unbounded PNG accepted")
		}
	}
}
