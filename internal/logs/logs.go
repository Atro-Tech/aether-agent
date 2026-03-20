// Package logs provides logging utilities.
// Stripped-down version — no MMDS, no E2B-specific forwarding.
package logs

import (
	"context"

	"connectrpc.com/connect"
	"github.com/google/uuid"
	"github.com/rs/zerolog"
)

type contextKey string

const OperationIDKey contextKey = "operation_id"

func AddRequestIDToContext(ctx context.Context) context.Context {
	return context.WithValue(ctx, OperationIDKey, uuid.New().String())
}

// NewUnaryLogInterceptor returns a connect interceptor that logs RPCs.
func NewUnaryLogInterceptor(l *zerolog.Logger) connect.UnaryInterceptorFunc {
	return func(next connect.UnaryFunc) connect.UnaryFunc {
		return func(ctx context.Context, req connect.AnyRequest) (connect.AnyResponse, error) {
			l.Debug().Str("procedure", req.Spec().Procedure).Msg("rpc")
			return next(ctx, req)
		}
	}
}

// LogServerStreamWithoutEvents wraps a server stream handler with logging.
func LogServerStreamWithoutEvents[Req, Resp any](
	ctx context.Context,
	l *zerolog.Logger,
	req *connect.Request[Req],
	stream *connect.ServerStream[Resp],
	handler func(context.Context, *connect.Request[Req], *connect.ServerStream[Resp]) error,
) error {
	ctx = AddRequestIDToContext(ctx)
	l.Debug().Str("procedure", req.Spec().Procedure).Msg("stream")
	return handler(ctx, req, stream)
}

// LogClientStreamWithoutEvents wraps a client stream handler with logging.
func LogClientStreamWithoutEvents[Req, Resp any](
	ctx context.Context,
	l *zerolog.Logger,
	stream *connect.ClientStream[Req],
	handler func(context.Context, *connect.ClientStream[Req]) (*connect.Response[Resp], error),
) (*connect.Response[Resp], error) {
	ctx = AddRequestIDToContext(ctx)
	return handler(ctx, stream)
}

// LogBufferedDataEvents is a no-op in the stripped version.
// The original buffers data events for log aggregation.
func LogBufferedDataEvents(ch chan []byte, l *zerolog.Logger, event string) func() {
	// Drain the channel so writers don't block
	go func() {
		for range ch {
		}
	}()
	return func() {}
}
