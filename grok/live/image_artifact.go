package live

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"image"
	"image/color"
	_ "image/jpeg"
	_ "image/png"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

const maxImageBytes = 32 << 20

// Stock imageGeneration results, GeneratedImageOutput and recent_images all
// own PNG representation. Full decoding rejects JPEG mislabeled by that path.
func imagePayload(encoded string) ([]byte, error) {
	if len(encoded) == 0 || len(encoded) > base64.StdEncoding.EncodedLen(maxImageBytes) {
		return nil, errors.New("live: invalid image payload size")
	}
	data, err := base64.StdEncoding.Strict().DecodeString(encoded)
	if err != nil || len(data) > maxImageBytes {
		return nil, errors.New("live: invalid image base64")
	}
	config, format, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil || format != "png" || config.Width <= 0 || config.Height <= 0 {
		return nil, errors.New("live: invalid image codec or dimensions")
	}
	pixelBytes := int64(4)
	if config.ColorModel == color.RGBA64Model || config.ColorModel == color.NRGBA64Model {
		pixelBytes = 8
	}
	if int64(config.Width)*int64(config.Height) > maxImageBytes/pixelBytes {
		return nil, errors.New("live: image decoded byte budget exceeded")
	}
	decoded, format, err := image.Decode(bytes.NewReader(data))
	if err != nil || format != "png" || decoded.Bounds().Dx() != config.Width || decoded.Bounds().Dy() != config.Height {
		return nil, errors.New("live: image codec did not decode")
	}
	return data, nil
}

func imageOutputDigest(raw json.RawMessage) (string, error) {
	var failure string
	if len(raw) <= 64<<10 && json.Unmarshal(raw, &failure) == nil && strings.TrimSpace(failure) != "" {
		return "text:" + editDigest([]byte(failure)), nil
	}
	var content []struct {
		Type     string `json:"type"`
		ImageURL string `json:"image_url"`
		FileID   string `json:"file_id"`
	}
	if json.Unmarshal(raw, &content) != nil || len(content) > 16 {
		return "", errors.New("live: invalid canonical image output")
	}
	hash := ""
	for _, item := range content {
		switch item.Type {
		case "input_text":
		case "input_image":
			if hash != "" || item.FileID != "" || !strings.HasPrefix(item.ImageURL, "data:image/png;base64,") {
				return "", errors.New("live: invalid canonical image representation")
			}
			data, err := imagePayload(strings.TrimPrefix(item.ImageURL, "data:image/png;base64,"))
			if err != nil {
				return "", err
			}
			hash = editDigest(data)
		default:
			return "", errors.New("live: invalid canonical image representation")
		}
	}
	if hash == "" {
		return "", errors.New("live: canonical image output missing")
	}
	return hash, nil
}

// Only the controlled artifact subtree is enumerated, before each invocation.
func imagePaths(root string) (map[string]bool, error) {
	paths := map[string]bool{}
	count := 0
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if path == root && os.IsNotExist(err) {
			return nil
		}
		count++
		if err != nil || count > 256 || entry.Type()&os.ModeSymlink != 0 {
			return errors.New("live: image artifact inventory unavailable")
		}
		if !entry.IsDir() {
			paths[path] = true
		}
		return nil
	})
	return paths, err
}

func inspectImage(root, path, expected string) (int, error) {
	relative, err := filepath.Rel(root, path)
	if err != nil || path != filepath.Clean(path) || !filepath.IsAbs(path) || relative == "." || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		return 0, errors.New("live: image artifact outside save root")
	}
	current := root
	for _, part := range strings.Split(relative, string(filepath.Separator)) {
		info, err := os.Lstat(current)
		if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return 0, errors.New("live: image artifact directory unavailable")
		}
		current = filepath.Join(current, part)
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() <= 0 || info.Size() > maxImageBytes {
		return 0, errors.New("live: image artifact is not a bounded regular file")
	}
	file, err := os.Open(path)
	if err != nil {
		return 0, errors.New("live: image artifact unavailable")
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		return 0, errors.New("live: image artifact identity changed")
	}
	data, err := io.ReadAll(io.LimitReader(file, maxImageBytes+1))
	if err != nil || len(data) > maxImageBytes || editDigest(data) != expected {
		return 0, errors.New("live: image artifact payload mismatch")
	}
	return len(data), nil
}

type imageArguments struct {
	Count *int
	Paths []string
}

func parseImageArguments(raw string) (imageArguments, error) {
	var result imageArguments
	invalid := errors.New("live: invalid image arguments")
	if len(raw) > 4096 {
		return result, invalid
	}
	decoder := json.NewDecoder(strings.NewReader(raw))
	token, err := decoder.Token()
	if err != nil || token != json.Delim('{') {
		return result, invalid
	}
	seen := map[string]bool{}
	for decoder.More() {
		token, err := decoder.Token()
		key, ok := token.(string)
		if err != nil || !ok || seen[key] {
			return result, invalid
		}
		seen[key] = true
		switch key {
		case "prompt":
			var prompt string
			if decoder.Decode(&prompt) != nil || strings.TrimSpace(prompt) == "" {
				return result, invalid
			}
		case "num_last_images_to_include":
			if decoder.Decode(&result.Count) != nil {
				return result, invalid
			}
		case "referenced_image_paths":
			if decoder.Decode(&result.Paths) != nil {
				return result, invalid
			}
		default:
			return result, invalid
		}
	}
	if token, err = decoder.Token(); err != nil || token != json.Delim('}') || !seen["prompt"] {
		return result, invalid
	}
	if _, err := decoder.Token(); err != io.EOF {
		return result, invalid
	}
	return result, nil
}
