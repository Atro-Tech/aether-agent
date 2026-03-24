package execcontext

import (
	"errors"

	"github.com/atro-tech/aether-agent/internal/utils"
)

type Defaults struct {
	EnvVars    *utils.Map[string, string]
	User       string
	Workdir    *string
	UseOverlay bool // true when FUSE overlay is mounted at /aether
}

func ResolveDefaultWorkdir(workdir string, defaultWorkdir *string) string {
	if workdir != "" {
		return workdir
	}

	if defaultWorkdir != nil {
		return *defaultWorkdir
	}

	return ""
}

func ResolveDefaultUsername(username *string, defaultUsername string) (string, error) {
	if username != nil {
		return *username, nil
	}

	if defaultUsername != "" {
		return defaultUsername, nil
	}

	return "", errors.New("username not provided")
}
