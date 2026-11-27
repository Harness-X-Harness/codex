package live

import (
	"bytes"
	"encoding/base64"
	"image"
	"image/color"
	_ "image/jpeg"
	_ "image/png"
	"io"
	"os"
	"path/filepath"
	"strings"
)

const maxImageBytes = 32 << 20

// Stock imageGeneration results, GeneratedImageOutput and recent_images all
// own PNG representation. Full decoding rejects JPEG mislabeled by that path.
func imagePayload(encoded string) ([]byte, error) {
	if len(encoded) == 0 || len(encoded) > base64.StdEncoding.EncodedLen(maxImageBytes) {
		return nil, failedObservation("invalid_image_payload_size", "live: invalid image payload size")
	}
	data, err := base64.StdEncoding.Strict().DecodeString(encoded)
	if err != nil || len(data) > maxImageBytes {
		return nil, failedObservation("invalid_image_base64", "live: invalid image base64")
	}
	config, format, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil || format != "png" || config.Width <= 0 || config.Height <= 0 {
		return nil, failedObservation("invalid_image_codec_or_dimensions", "live: invalid image codec or dimensions")
	}
	pixelBytes := int64(4)
	if config.ColorModel == color.RGBA64Model || config.ColorModel == color.NRGBA64Model {
		pixelBytes = 8
	}
	if int64(config.Width)*int64(config.Height) > maxImageBytes/pixelBytes {
		return nil, failedObservation("image_decoded_byte_budget_exceeded", "live: image decoded byte budget exceeded")
	}
	decoded, format, err := image.Decode(bytes.NewReader(data))
	if err != nil || format != "png" || decoded.Bounds().Dx() != config.Width || decoded.Bounds().Dy() != config.Height {
		return nil, failedObservation("image_codec_decode_failed", "live: image codec did not decode")
	}
	return data, nil
}

func inspectImage(root, path, expected string) (int, error) {
	relative, err := filepath.Rel(root, path)
	if err != nil || path != filepath.Clean(path) || !filepath.IsAbs(path) || relative == "." || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		return 0, failedObservation("image_artifact_outside_root", "live: image artifact outside save root")
	}
	current := root
	for _, part := range strings.Split(relative, string(filepath.Separator)) {
		info, err := os.Lstat(current)
		if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return 0, failedObservation("image_artifact_directory_unavailable", "live: image artifact directory unavailable")
		}
		current = filepath.Join(current, part)
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() <= 0 || info.Size() > maxImageBytes {
		return 0, failedObservation("invalid_image_artifact_file", "live: image artifact is not a bounded regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return 0, failedObservation("image_artifact_unavailable", "live: image artifact unavailable")
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		return 0, failedObservation("image_artifact_identity_changed", "live: image artifact identity changed")
	}
	data, err := io.ReadAll(io.LimitReader(file, maxImageBytes+1))
	if err != nil || len(data) > maxImageBytes || editDigest(data) != expected {
		return 0, failedObservation("image_artifact_payload_mismatch", "live: image artifact payload mismatch")
	}
	return len(data), nil
}
