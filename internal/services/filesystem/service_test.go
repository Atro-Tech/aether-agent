package filesystem

import (
	"github.com/atro-tech/aether-agent/internal/execcontext"
	"github.com/atro-tech/aether-agent/internal/utils"
)

func mockService() Service {
	return Service{
		defaults: &execcontext.Defaults{
			EnvVars: utils.NewMap[string, string](),
		},
	}
}
