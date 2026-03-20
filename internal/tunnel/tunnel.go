// Package tunnel provides a reverse WebSocket tunnel to Phoenix.
//
// When the agent starts with --callback, it dials out to the Phoenix
// backend over WSS. Phoenix pushes ConnectRPC requests through the
// channel; this package proxies them to localhost and sends responses
// back. The backend never needs to reach the agent's IP directly.
package tunnel

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"
	"time"

	"github.com/gorilla/websocket"
	"github.com/rs/zerolog"
)

// Config for the tunnel connection.
type Config struct {
	CallbackURL string // wss://backend/socket/aether/websocket?token=...&workspace_id=...
	LocalPort   int    // port the agent listens on (default 49983)
	Logger      *zerolog.Logger
}

// Start connects to Phoenix and proxies requests. Blocks forever,
// reconnecting on disconnect. Call from a goroutine.
func Start(cfg Config) {
	l := cfg.Logger
	localBase := fmt.Sprintf("http://127.0.0.1:%d", cfg.LocalPort)

	for {
		err := run(cfg.CallbackURL, localBase, l)
		l.Warn().Err(err).Msg("tunnel disconnected, reconnecting in 2s")
		time.Sleep(2 * time.Second)
	}
}

func run(url, localBase string, l *zerolog.Logger) error {
	l.Info().Str("url", url).Msg("connecting to backend")

	conn, _, err := websocket.DefaultDialer.Dial(url, nil)
	if err != nil {
		return fmt.Errorf("dial: %w", err)
	}
	defer conn.Close()

	l.Info().Msg("tunnel connected")

	// Join the aether channel
	joinMsg := map[string]any{
		"topic":   topicFromURL(url),
		"event":   "phx_join",
		"payload": map[string]any{},
		"ref":     "1",
	}
	if err := conn.WriteJSON(joinMsg); err != nil {
		return fmt.Errorf("join: %w", err)
	}

	// Read join reply
	var joinReply map[string]any
	if err := conn.ReadJSON(&joinReply); err != nil {
		return fmt.Errorf("join reply: %w", err)
	}
	l.Info().Interface("reply", joinReply).Msg("joined channel")

	// Send ready signal
	readyMsg := map[string]any{
		"topic":   topicFromURL(url),
		"event":   "ready",
		"payload": map[string]any{},
		"ref":     "2",
	}
	conn.WriteJSON(readyMsg)

	// Start heartbeat
	var mu sync.Mutex
	go heartbeat(conn, &mu)

	// Read and proxy requests
	for {
		_, raw, err := conn.ReadMessage()
		if err != nil {
			return fmt.Errorf("read: %w", err)
		}

		var msg struct {
			Event   string          `json:"event"`
			Payload json.RawMessage `json:"payload"`
			Topic   string          `json:"topic"`
		}
		if err := json.Unmarshal(raw, &msg); err != nil {
			continue
		}

		if msg.Event == "request" {
			go handleRequest(conn, &mu, msg.Topic, msg.Payload, localBase, l)
		}
	}
}

func handleRequest(conn *websocket.Conn, mu *sync.Mutex, topic string, payload json.RawMessage, localBase string, l *zerolog.Logger) {
	var req struct {
		Ref    string `json:"ref"`
		Method string `json:"method"`
		Path   string `json:"path"`
		Body   string `json:"body"`
	}
	if err := json.Unmarshal(payload, &req); err != nil {
		l.Warn().Err(err).Msg("bad request payload")
		return
	}

	// Proxy to local agent
	url := localBase + req.Path
	httpReq, err := http.NewRequest(req.Method, url, bytes.NewBufferString(req.Body))
	if err != nil {
		sendResponse(conn, mu, topic, req.Ref, 500, fmt.Sprintf(`{"error":"%s"}`, err.Error()))
		return
	}
	httpReq.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(httpReq)
	if err != nil {
		sendResponse(conn, mu, topic, req.Ref, 502, fmt.Sprintf(`{"error":"%s"}`, err.Error()))
		return
	}
	defer resp.Body.Close()

	body, _ := io.ReadAll(resp.Body)
	sendResponse(conn, mu, topic, req.Ref, resp.StatusCode, string(body))
}

func sendResponse(conn *websocket.Conn, mu *sync.Mutex, topic, ref string, status int, body string) {
	msg := map[string]any{
		"topic": topic,
		"event": "response",
		"payload": map[string]any{
			"ref":    ref,
			"status": status,
			"body":   body,
		},
		"ref": ref,
	}

	mu.Lock()
	defer mu.Unlock()
	conn.WriteJSON(msg)
}

func heartbeat(conn *websocket.Conn, mu *sync.Mutex) {
	ticker := time.NewTicker(30 * time.Second)
	defer ticker.Stop()

	for range ticker.C {
		msg := map[string]any{
			"topic":   "phoenix",
			"event":   "heartbeat",
			"payload": map[string]any{},
			"ref":     fmt.Sprintf("hb-%d", time.Now().UnixMilli()),
		}
		mu.Lock()
		err := conn.WriteJSON(msg)
		mu.Unlock()
		if err != nil {
			return
		}
	}
}

// Extract workspace_id from the callback URL to build the channel topic.
func topicFromURL(url string) string {
	// URL has ?workspace_id=xxx
	// Parse it out
	for _, part := range splitQuery(url) {
		if len(part) > 13 && part[:13] == "workspace_id=" {
			return "aether:" + part[13:]
		}
	}
	return "aether:unknown"
}

func splitQuery(url string) []string {
	idx := 0
	for i, c := range url {
		if c == '?' {
			idx = i + 1
			break
		}
	}
	if idx == 0 {
		return nil
	}
	query := url[idx:]
	var parts []string
	start := 0
	for i, c := range query {
		if c == '&' {
			parts = append(parts, query[start:i])
			start = i + 1
		}
	}
	parts = append(parts, query[start:])
	return parts
}
