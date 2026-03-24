package main

import (
	"bytes"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"strconv"
	"syscall"
	"time"

	connectcors "connectrpc.com/cors"
	"github.com/go-chi/chi/v5"
	"github.com/rs/cors"
	"github.com/rs/zerolog"

	"github.com/atro-tech/aether-agent/internal/execcontext"
	"github.com/atro-tech/aether-agent/internal/fuse"
	"github.com/atro-tech/aether-agent/internal/manifest"
	aethernet "github.com/atro-tech/aether-agent/internal/net"
	"github.com/atro-tech/aether-agent/internal/network"
	filesystemRpc "github.com/atro-tech/aether-agent/internal/services/filesystem"
	processRpc "github.com/atro-tech/aether-agent/internal/services/process"
	"github.com/atro-tech/aether-agent/internal/tunnel"
	"github.com/atro-tech/aether-agent/internal/utils"
)

const (
	idleTimeout = 640 * time.Second
	maxAge      = 2 * time.Hour
	defaultPort = 49983
	defaultUser = "root"
)

var (
	Version = "0.1.0"

	port        int64
	callbackURL string
	versionFlag bool
)

func main() {
	flag.BoolVar(&versionFlag, "version", false, "print version")
	flag.Int64Var(&port, "port", defaultPort, "port to listen on")
	flag.StringVar(&callbackURL, "callback", "", "WebSocket URL to dial out to (reverse tunnel to control plane)")
	flag.Parse()

	if versionFlag {
		fmt.Println(Version)
		return
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	l := zerolog.New(os.Stdout).With().Timestamp().Str("service", "aether-agent").Logger()
	l.Info().Str("version", Version).Msg("starting")

	// ── Boot sequence ──

	// 1. Mount FUSE overlay
	//
	// We mount over /aether as a staging area. The overlay proxies
	// the real root filesystem underneath and lets Shimmer inject
	// virtual files on top. Processes that need the overlay access
	// it through /aether (or we bind-mount specific paths later).
	//
	// We can't mount directly over / while the agent is running on it.
	// The namespace root mount requires pivot_root from an initramfs,
	// which Sprites doesn't support. So we mount at /aether and the
	// overlay is accessible there.
	var fuseOverlay *fuse.Overlay
	overlayMounted := false

	fuseMountPoint := "/aether"
	fuseLogger := l.With().Str("component", "fuse").Logger()

	if err := os.MkdirAll(fuseMountPoint, 0o755); err != nil {
		l.Warn().Err(err).Msg("failed to create FUSE mount point")
	} else {
		server, overlay, err := fuse.MountAt("/", fuseMountPoint, &fuseLogger)
		if err != nil {
			l.Warn().Err(err).Msg("FUSE mount failed (non-fatal)")
		} else {
			fuseOverlay = overlay
			overlayMounted = true
			go server.Wait()
			l.Info().Str("mount", fuseMountPoint).Msg("FUSE overlay mounted")
		}
	}

	// 2. Create manifest store and wire callbacks
	store := manifest.NewStore()

	if fuseOverlay != nil {
		fuseOverlay.SetStore(store)
	}

	// 3. Set up network firewall (default-deny egress)
	firewallLogger := l.With().Str("component", "firewall").Logger()
	tunnelHost, tunnelPort := parseTunnelURL(callbackURL)

	fw, fwErr := network.Setup(&firewallLogger, tunnelHost, tunnelPort)
	if fwErr != nil {
		l.Warn().Err(fwErr).Msg("firewall setup failed (non-fatal)")
	}

	// Register manifest callbacks
	store.OnUpdate(func(m *manifest.Manifest) {
		if fw != nil {
			if err := fw.Apply(m.Network); err != nil {
				l.Warn().Err(err).Msg("failed to apply network rules")
			}
		}
		l.Info().
			Int("network_rules", len(m.Network)).
			Int("filesystem_rules", len(m.Filesystem)).
			Int("credentials", len(m.Credentials)).
			Msg("manifest applied")
	})

	// 4. Set up eBPF network interceptor on spr0/eth0 (for credential routing)
	aethernet.Setup(&l)

	// 5. Start services
	defaults := &execcontext.Defaults{
		User:       defaultUser,
		EnvVars:    utils.NewMap[string, string](),
		UseOverlay: overlayMounted,
	}

	m := chi.NewRouter()

	fsLogger := l.With().Str("component", "filesystem").Logger()
	filesystemRpc.Handle(m, &fsLogger, defaults)

	processLogger := l.With().Str("component", "process").Logger()
	processRpc.Handle(m, &processLogger, defaults, nil)

	// Simple unary exec endpoint for the reverse tunnel
	m.Post("/exec", func(w http.ResponseWriter, r *http.Request) {
		var req struct {
			Cmd  string            `json:"cmd"`
			Args []string          `json:"args"`
			Envs map[string]string `json:"envs"`
			Cwd  string            `json:"cwd"`
		}
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		cmd := exec.CommandContext(r.Context(), req.Cmd, req.Args...)
		if req.Cwd != "" {
			cmd.Dir = req.Cwd
		}
		for k, v := range req.Envs {
			cmd.Env = append(cmd.Env, k+"="+v)
		}
		if len(cmd.Env) > 0 {
			cmd.Env = append(os.Environ(), cmd.Env...)
		}

		// Chroot into the FUSE overlay when available
		if overlayMounted {
			cmd.SysProcAttr = &syscall.SysProcAttr{
				Chroot: "/aether",
			}
		}

		var stdout, stderr bytes.Buffer
		cmd.Stdout = &stdout
		cmd.Stderr = &stderr
		exitCode := 0
		if err := cmd.Run(); err != nil {
			if ee, ok := err.(*exec.ExitError); ok {
				exitCode = ee.ExitCode()
			} else {
				http.Error(w, err.Error(), 500)
				return
			}
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]any{
			"stdout":    stdout.String(),
			"stderr":    stderr.String(),
			"exit_code": exitCode,
		})
	})

	// Manifest push endpoint — receives full sandbox manifest from backend
	m.Post("/shimmer/manifest", func(w http.ResponseWriter, r *http.Request) {
		var man manifest.Manifest
		if err := json.NewDecoder(r.Body).Decode(&man); err != nil {
			http.Error(w, err.Error(), 400)
			return
		}
		if err := store.Update(&man); err != nil {
			http.Error(w, err.Error(), 500)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]any{
			"ok":               true,
			"network_rules":    len(man.Network),
			"filesystem_rules": len(man.Filesystem),
		})
	})

	s := &http.Server{
		Handler:      withCORS(m),
		Addr:         fmt.Sprintf("0.0.0.0:%d", port),
		ReadTimeout:  0,
		WriteTimeout: 0,
		IdleTimeout:  idleTimeout,
	}

	l.Info().Int64("port", port).Msg("listening")

	// Start reverse tunnel if callback URL provided
	if callbackURL != "" {
		tunnelLogger := l.With().Str("component", "tunnel").Logger()
		go tunnel.Start(tunnel.Config{
			CallbackURL: callbackURL,
			LocalPort:   int(port),
			Logger:      &tunnelLogger,
		})
		l.Info().Str("callback", callbackURL).Msg("reverse tunnel started")
	}

	if err := s.ListenAndServe(); err != nil {
		log.Fatalf("server error: %v", err)
	}

	_ = ctx
}

// parseTunnelURL extracts host and port from the callback WSS URL.
func parseTunnelURL(rawURL string) (string, int) {
	if rawURL == "" {
		return "", 0
	}

	u, err := url.Parse(rawURL)
	if err != nil {
		return "", 0
	}

	host := u.Hostname()
	portStr := u.Port()
	if portStr == "" {
		if u.Scheme == "wss" || u.Scheme == "https" {
			return host, 443
		}
		return host, 80
	}

	p, err := strconv.Atoi(portStr)
	if err != nil {
		return host, 443
	}
	return host, p
}

func withCORS(h http.Handler) http.Handler {
	middleware := cors.New(cors.Options{
		AllowedOrigins: []string{"*"},
		AllowedMethods: []string{
			http.MethodGet, http.MethodPost, http.MethodPut,
			http.MethodPatch, http.MethodDelete, http.MethodHead,
		},
		AllowedHeaders: []string{"*"},
		ExposedHeaders: append(connectcors.ExposedHeaders(), "Location"),
		MaxAge:         int(maxAge.Seconds()),
	})
	return middleware.Handler(h)
}
