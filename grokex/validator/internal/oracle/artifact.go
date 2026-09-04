package oracle

import (
	"bytes"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"image"
	_ "image/jpeg"
	_ "image/png"
	"net/http"
	"os"
	"path/filepath"
	"strings"

	_ "golang.org/x/image/webp"

	"github.com/Harness-X-Harness/codex/grokex/validator/internal/rollout"
)

// Artifact is a verified image result: the persisted base64 payload, the file
// the product saved for the user, and the codec a real decoder recognized.
type Artifact struct {
	CallID    string
	Path      string
	MIME      string
	Extension string
	SHA256    string
	Width     int
	Height    int
}

// supportedArtifacts mirrors release.py SUPPORTED_IMAGE_ARTIFACTS: the observed
// MIME decides the extension; nothing is derived from the request format.
var supportedArtifacts = map[string]struct {
	extension string
	format    string
}{
	"image/jpeg": {".jpg", "jpeg"},
	"image/png":  {".png", "png"},
	"image/webp": {".webp", "webp"},
}

// VerifyArtifact proves one completed image result is a real, user-accessible
// image whose saved file equals the persisted payload.
func VerifyArtifact(result rollout.ImageResult) (Artifact, error) {
	if result.Status != "completed" {
		return Artifact{}, fmt.Errorf("image result %s status is %q", result.CallID, result.Status)
	}
	payload, err := base64.StdEncoding.DecodeString(result.Result)
	if err != nil {
		return Artifact{}, fmt.Errorf("image result %s is not base64: %w", result.CallID, err)
	}
	if result.SavedPath == "" {
		return Artifact{}, fmt.Errorf("image result %s has no saved path", result.CallID)
	}
	saved, err := os.ReadFile(result.SavedPath)
	if err != nil {
		return Artifact{}, fmt.Errorf("image artifact %s: %w", result.CallID, err)
	}
	if !bytes.Equal(saved, payload) {
		return Artifact{}, fmt.Errorf("image artifact %s differs from the persisted result", result.CallID)
	}
	mime := http.DetectContentType(payload)
	codec, ok := supportedArtifacts[mime]
	if !ok {
		return Artifact{}, fmt.Errorf("image artifact %s has unsupported content signature %s", result.CallID, mime)
	}
	config, format, err := image.DecodeConfig(bytes.NewReader(payload))
	if err != nil {
		return Artifact{}, fmt.Errorf("image artifact %s does not decode: %w", result.CallID, err)
	}
	if format != codec.format {
		return Artifact{}, fmt.Errorf("image artifact %s decodes as %s but is signed %s", result.CallID, format, mime)
	}
	if extension := strings.ToLower(filepath.Ext(result.SavedPath)); extension != codec.extension {
		return Artifact{}, fmt.Errorf("image artifact %s extension %s does not match %s", result.CallID, extension, mime)
	}
	if config.Width <= 0 || config.Height <= 0 {
		return Artifact{}, errors.New("image artifact has no dimensions")
	}
	sum := sha256.Sum256(payload)
	return Artifact{
		CallID:    result.CallID,
		Path:      result.SavedPath,
		MIME:      mime,
		Extension: codec.extension,
		SHA256:    hex.EncodeToString(sum[:]),
		Width:     config.Width,
		Height:    config.Height,
	}, nil
}
