package live

import (
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/exec"
	"os/signal"
	"syscall"
)

const stdoutFilterArg = "live-stdout-filter"

const maxStdoutFrameBytes = 16 << 20

// ServeStdoutFilter runs the fork stdout adapter when args begin with
// live-stdout-filter. The official SDK never sees a public xSearch item.
// Callers exit with the returned status when handled is true.
func ServeStdoutFilter(args []string) (bool, int) {
	if len(args) == 0 || args[0] != stdoutFilterArg {
		return false, 0
	}
	if len(args) < 2 || args[1] == "" {
		return true, 1
	}
	return true, runStdoutFilter(args[1:])
}

// runStdoutFilter copies stdin onto the app-server and forwards stdout with
// public xSearch removed. The copy and the filter each own one pipe direction.
// Waiting for the child follows stdout EOF, which happens-after its last write.
func runStdoutFilter(command []string) int {
	cmd := exec.Command(command[0], command[1:]...)
	cmd.Env = os.Environ()
	if wd, err := os.Getwd(); err == nil {
		cmd.Dir = wd
	}
	cmd.Stderr = os.Stderr
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return 1
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return 1
	}
	if err := cmd.Start(); err != nil {
		return 1
	}
	process := cmd.Process
	signals := make(chan os.Signal, 1)
	signal.Notify(signals, os.Interrupt)
	defer signal.Stop(signals)
	stop := make(chan struct{})
	stopped := make(chan struct{})
	go func() {
		defer close(stopped)
		for {
			select {
			case <-stop:
				return
			case <-signals:
				_ = process.Signal(os.Interrupt)
			}
		}
	}()
	defer func() {
		close(stop)
		<-stopped
	}()
	// The parent keeps stdin open until this process exits, so this copy is not
	// joined. Joining it before exit would wait for the parent while the parent
	// waits for stdout EOF.
	go func() {
		defer stdin.Close()
		_, _ = io.Copy(stdin, os.Stdin)
	}()
	filterErr := filterStdout(stdout, os.Stdout)
	waitErr := cmd.Wait()
	if filterErr != nil {
		return 1
	}
	if waitErr == nil {
		return 0
	}
	var exit *exec.ExitError
	if errors.As(waitErr, &exit) {
		return exit.ExitCode()
	}
	return 1
}

func filterStdout(r io.Reader, w io.Writer) error {
	reader := bufio.NewReader(r)
	for {
		line, overflow, err := readStdoutFrame(reader, maxStdoutFrameBytes)
		if overflow {
			// The SDK reports its own frame budget. Forward the original bytes.
			if werr := writeStdout(w, line); werr != nil {
				return werr
			}
			if errors.Is(err, bufio.ErrBufferFull) {
				if werr := copyUntilNewline(reader, w); werr != nil {
					return werr
				}
				continue
			}
		} else if len(line) > 0 {
			filtered, drop, ferr := filterStdoutFrame(line)
			if ferr != nil {
				return ferr
			}
			if !drop {
				if werr := writeStdout(w, filtered); werr != nil {
					return werr
				}
			}
		}
		if err != nil && !errors.Is(err, bufio.ErrBufferFull) {
			if errors.Is(err, io.EOF) {
				return nil
			}
			return err
		}
	}
}

func writeStdout(w io.Writer, frame []byte) error {
	if len(frame) == 0 {
		return nil
	}
	_, err := w.Write(frame)
	if errors.Is(err, syscall.EPIPE) {
		return nil
	}
	return err
}

func copyUntilNewline(reader *bufio.Reader, w io.Writer) error {
	for {
		chunk, err := reader.ReadSlice('\n')
		if werr := writeStdout(w, chunk); werr != nil {
			return werr
		}
		if err == nil || errors.Is(err, io.EOF) {
			return nil
		}
		if errors.Is(err, bufio.ErrBufferFull) {
			continue
		}
		return err
	}
}

func readStdoutFrame(reader *bufio.Reader, limit int) ([]byte, bool, error) {
	var frame bytes.Buffer
	for {
		chunk, err := reader.ReadSlice('\n')
		overflow := frame.Len()+len(chunk) > limit
		frame.Write(chunk)
		if overflow {
			return frame.Bytes(), true, err
		}
		if err == nil {
			return frame.Bytes(), false, nil
		}
		if errors.Is(err, bufio.ErrBufferFull) {
			continue
		}
		if errors.Is(err, io.EOF) {
			if frame.Len() == 0 {
				return nil, false, io.EOF
			}
			return frame.Bytes(), false, nil
		}
		if frame.Len() == 0 {
			return nil, false, err
		}
		return frame.Bytes(), false, err
	}
}

func filterStdoutFrame(line []byte) ([]byte, bool, error) {
	payload := bytes.TrimSpace(line)
	if len(payload) == 0 || payload[0] != '{' || !bytes.Contains(payload, []byte(`"xSearch"`)) {
		return line, false, nil
	}
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return line, false, nil
	}
	object, ok := value.(map[string]any)
	if !ok {
		return line, false, nil
	}
	if dropXSearchNotification(object) {
		return nil, true, nil
	}
	stripped, removed := stripXSearch(object)
	if !removed {
		return line, false, nil
	}
	var encoded bytes.Buffer
	encoder := json.NewEncoder(&encoded)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(stripped); err != nil {
		return nil, false, err
	}
	return encoded.Bytes(), false, nil
}

func dropXSearchNotification(object map[string]any) bool {
	method, _ := object["method"].(string)
	if method != "item/started" && method != "item/completed" {
		return false
	}
	params, _ := object["params"].(map[string]any)
	item, _ := params["item"].(map[string]any)
	return item["type"] == "xSearch"
}

func stripXSearch(value any) (any, bool) {
	switch node := value.(type) {
	case map[string]any:
		if node["type"] == "xSearch" {
			return nil, true
		}
		removed := false
		for key, child := range node {
			if childObject, ok := child.(map[string]any); ok && childObject["type"] == "xSearch" {
				delete(node, key)
				removed = true
				continue
			}
			next, hit := stripXSearch(child)
			if hit {
				removed = true
			}
			node[key] = next
		}
		return node, removed
	case []any:
		kept := make([]any, 0, len(node))
		removed := false
		for _, child := range node {
			if childObject, ok := child.(map[string]any); ok && childObject["type"] == "xSearch" {
				removed = true
				continue
			}
			next, hit := stripXSearch(child)
			if hit {
				removed = true
			}
			kept = append(kept, next)
		}
		return kept, removed
	default:
		return value, false
	}
}
