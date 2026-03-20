package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"time"

	connectcors "connectrpc.com/cors"
	"github.com/go-chi/chi/v5"
	"github.com/rs/cors"
	"github.com/rs/zerolog"

	"github.com/atro-tech/aether-agent/internal/execcontext"
	"github.com/atro-tech/aether-agent/internal/fuse"
	aethernet "github.com/atro-tech/aether-agent/internal/net"
	filesystemRpc "github.com/atro-tech/aether-agent/internal/services/filesystem"
	processRpc "github.com/atro-tech/aether-agent/internal/services/process"
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
	versionFlag bool
)

func main() {
	flag.BoolVar(&versionFlag, "version", false, "print version")
	flag.Int64Var(&port, "port", defaultPort, "port to listen on")
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

	// 1. Mount FUSE at / (the filesystem)
	fuse.Mount(&l)

	// 2. Set up eBPF network interceptor on spr0/eth0
	aethernet.Setup(&l)

	// 3. Start services
	defaults := &execcontext.Defaults{
		User:    defaultUser,
		EnvVars: utils.NewMap[string, string](),
	}

	m := chi.NewRouter()

	fsLogger := l.With().Str("component", "filesystem").Logger()
	filesystemRpc.Handle(m, &fsLogger, defaults)

	processLogger := l.With().Str("component", "process").Logger()
	processRpc.Handle(m, &processLogger, defaults, nil)

	s := &http.Server{
		Handler:      withCORS(m),
		Addr:         fmt.Sprintf("0.0.0.0:%d", port),
		ReadTimeout:  0,
		WriteTimeout: 0,
		IdleTimeout:  idleTimeout,
	}

	l.Info().Int64("port", port).Msg("listening")

	if err := s.ListenAndServe(); err != nil {
		log.Fatalf("server error: %v", err)
	}

	_ = ctx
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
