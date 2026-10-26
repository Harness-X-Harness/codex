package live

import "testing"

func TestImageRecognitionScopesUnrelatedQualifiers(t *testing.T) {
	for _, text := range []string{
		"A dog, possibly a retriever.",
		"No hat. A dog is shown.",
		"A dog. I cannot determine its breed.",
		"A dog, with maybe a collar.",
	} {
		if !imageRecognized(text, false) {
			t.Errorf("unrelated qualifier rejected: %q", text)
		}
	}
	for _, text := range []string{
		"A dog wearing a red hat, possibly a retriever.",
		"A dog of unknown breed wearing a red hat.",
		"No collar. A dog is wearing a red hat.",
		"A dog wearing a red hat. I am unsure about the background.",
	} {
		if !imageRecognized(text, true) {
			t.Errorf("valid edited scene rejected: %q", text)
		}
	}
	for _, text := range []string{
		"Possibly a dog.", "It is unknown whether the picture shows a dog.", "I cannot tell whether the animal is a dog.",
		"I cannot see the image; a dog.", "I can’t view the image; a dog.",
		"No dog is shown.", "There is not a dog in this picture.",
	} {
		if imageRecognized(text, false) {
			t.Errorf("absent/uncertain dog accepted: %q", text)
		}
	}
	for _, text := range []string{
		"A dog without a red hat.", "A dog, possibly wearing a red hat.",
		"It is unknown whether the dog wears a red hat.",
		"A dog wearing a hat, but not red.", "A dog wearing a blue hat.",
	} {
		if imageRecognized(text, true) {
			t.Errorf("absent/uncertain red hat accepted: %q", text)
		}
	}
}

func TestAttachedImageScopesUnrelatedQualifiers(t *testing.T) {
	for _, text := range []string{
		"A red circle, possibly made of paper.",
		"An object of unknown material shaped as a red circle.",
		"No border. A red circle is on the left.",
		"The left object is a red circle. I cannot identify its material.",
	} {
		if !attachedImageAnswer(text, "red", "circle", "left") {
			t.Errorf("valid shape rejected: %q", text)
		}
	}
	for _, text := range []string{
		"Possibly a red circle.", "I cannot tell whether it is a red circle.",
		"Not a red circle.", "A blue square.", "A red circle on the right.",
	} {
		if attachedImageAnswer(text, "red", "circle", "left") {
			t.Errorf("wrong shape accepted: %q", text)
		}
	}
}
