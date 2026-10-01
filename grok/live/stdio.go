package live

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"time"
)

type frame struct {
	size   int
	ID     json.RawMessage `json:"id,omitempty"`
	Method string          `json:"method,omitempty"`
	Params json.RawMessage `json:"params,omitempty"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  json.RawMessage `json:"error,omitempty"`
}

type appServer struct {
	cmd     *exec.Cmd
	cancel  context.CancelFunc
	input   io.WriteCloser
	output  *bufio.Scanner
	nextID  int
	pending []frame
}

func startServer(ctx context.Context, binary, home, cwd, key string) (*appServer, error) {
	processCtx, cancel := context.WithCancel(ctx)
	cmd := exec.CommandContext(processCtx, binary, "app-server", "--strict-config", "--listen", "stdio://")
	cmd.Dir, cmd.Stderr, cmd.WaitDelay = cwd, io.Discard, 2*time.Second
	for _, entry := range os.Environ() {
		name, _, _ := strings.Cut(entry, "=")
		if !strings.EqualFold(name, "CODEX_HOME") && !strings.EqualFold(name, "GROK_API_KEY") {
			cmd.Env = append(cmd.Env, entry)
		}
	}
	cmd.Env = append(cmd.Env, "CODEX_HOME="+home, "GROK_API_KEY="+key)
	input, err := cmd.StdinPipe()
	if err != nil {
		cancel()
		return nil, errors.New("live: process input unavailable")
	}
	output, err := cmd.StdoutPipe()
	if err != nil {
		cancel()
		_ = input.Close()
		return nil, errors.New("live: process output unavailable")
	}
	if err := cmd.Start(); err != nil {
		cancel()
		_ = input.Close()
		_ = output.Close()
		return nil, errors.New("live: process startup failed")
	}
	go func() { <-processCtx.Done(); _ = output.Close() }()
	scanner := bufio.NewScanner(output)
	scanner.Buffer(make([]byte, 4096), 16<<20)
	return &appServer{cmd: cmd, cancel: cancel, input: input, output: scanner}, nil
}

func (server *appServer) close() {
	_ = server.input.Close()
	done := make(chan struct{})
	go func() { _ = server.cmd.Wait(); close(done) }()
	timer := time.NewTimer(2 * time.Second)
	defer timer.Stop()
	select {
	case <-done:
	case <-timer.C:
		server.cancel()
		<-done
	}
	server.cancel()
}

func (server *appServer) send(message frame) error {
	if json.NewEncoder(server.input).Encode(message) != nil {
		return errors.New("live: protocol write failed")
	}
	return nil
}

func (server *appServer) read() (frame, error) {
	var message frame
	if !server.output.Scan() {
		if errors.Is(server.output.Err(), bufio.ErrTooLong) {
			return message, errors.New("live: protocol frame budget exceeded")
		}
		return message, errors.New("live: protocol ended before proof completion")
	}
	if json.Unmarshal(server.output.Bytes(), &message) != nil {
		return message, errors.New("live: invalid protocol frame")
	}
	message.size = len(server.output.Bytes())
	return message, nil
}

func (server *appServer) next() (frame, error) {
	if len(server.pending) == 0 {
		return server.read()
	}
	message := server.pending[0]
	server.pending[0] = frame{}
	server.pending = server.pending[1:]
	return message, nil
}

func (server *appServer) call(method string, params any, result any) error {
	server.nextID++
	id := json.RawMessage(strconv.Itoa(server.nextID))
	body, err := json.Marshal(params)
	if err != nil {
		return errors.New("live: invalid protocol request")
	}
	if err := server.send(frame{ID: id, Method: method, Params: body}); err != nil {
		return err
	}
	pendingBytes := 0
	for _, message := range server.pending {
		pendingBytes += message.size
	}
	for {
		message, err := server.read()
		if err != nil {
			return err
		}
		if message.Method != "" {
			if len(message.ID) != 0 {
				_ = server.send(frame{ID: message.ID, Error: json.RawMessage(`{"code":-32601,"message":"Unsupported Live request"}`)})
				return errors.New("live: unsupported server request")
			}
			pendingBytes += message.size
			if len(server.pending) == 128 || pendingBytes > 8<<20 {
				return errors.New("live: early evidence budget exceeded")
			}
			server.pending = append(server.pending, message)
			continue
		}
		if string(message.ID) != string(id) || len(message.Result) == 0 || len(message.Error) != 0 && string(message.Error) != "null" {
			return errors.New("live: protocol request failed")
		}
		if result != nil && json.Unmarshal(message.Result, result) != nil {
			return errors.New("live: invalid protocol response")
		}
		return nil
	}
}
