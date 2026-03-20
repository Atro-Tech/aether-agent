package filesystem

import (
	"connectrpc.com/connect"
	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog"

	"github.com/atro-tech/aether-agent/internal/execcontext"
	"github.com/atro-tech/aether-agent/internal/logs"
	spec "github.com/atro-tech/aether-agent/internal/services/spec/filesystem/filesystemconnect"
	"github.com/atro-tech/aether-agent/internal/utils"
)

type Service struct {
	logger   *zerolog.Logger
	watchers *utils.Map[string, *FileWatcher]
	defaults *execcontext.Defaults
}

func Handle(server *chi.Mux, l *zerolog.Logger, defaults *execcontext.Defaults) {
	service := Service{
		logger:   l,
		watchers: utils.NewMap[string, *FileWatcher](),
		defaults: defaults,
	}

	interceptors := connect.WithInterceptors(
		logs.NewUnaryLogInterceptor(l),
	)

	path, handler := spec.NewFilesystemHandler(service, interceptors)

	server.Mount(path, handler)
}
