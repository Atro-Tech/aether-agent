package filesystem

import (
	"context"

	"connectrpc.com/connect"

	"github.com/atro-tech/aether-agent/internal/permissions"
	rpc "github.com/atro-tech/aether-agent/internal/services/spec/filesystem"
)

func (s Service) Stat(ctx context.Context, req *connect.Request[rpc.StatRequest]) (*connect.Response[rpc.StatResponse], error) {
	u, err := permissions.GetAuthUser(ctx, s.defaults.User)
	if err != nil {
		return nil, err
	}

	path, err := permissions.ExpandAndResolve(req.Msg.GetPath(), u, s.defaults.Workdir)
	if err != nil {
		return nil, connect.NewError(connect.CodeInvalidArgument, err)
	}

	entry, err := entryInfo(path)
	if err != nil {
		return nil, err
	}

	return connect.NewResponse(&rpc.StatResponse{Entry: entry}), nil
}
