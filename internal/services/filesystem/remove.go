package filesystem

import (
	"context"
	"fmt"
	"os"

	"connectrpc.com/connect"

	"github.com/atro-tech/aether-agent/internal/permissions"
	rpc "github.com/atro-tech/aether-agent/internal/services/spec/filesystem"
)

func (s Service) Remove(ctx context.Context, req *connect.Request[rpc.RemoveRequest]) (*connect.Response[rpc.RemoveResponse], error) {
	u, err := permissions.GetAuthUser(ctx, s.defaults.User)
	if err != nil {
		return nil, err
	}

	path, err := permissions.ExpandAndResolve(req.Msg.GetPath(), u, s.defaults.Workdir)
	if err != nil {
		return nil, connect.NewError(connect.CodeInvalidArgument, err)
	}

	err = os.RemoveAll(path)
	if err != nil {
		return nil, connect.NewError(connect.CodeInternal, fmt.Errorf("error removing file or directory: %w", err))
	}

	return connect.NewResponse(&rpc.RemoveResponse{}), nil
}
