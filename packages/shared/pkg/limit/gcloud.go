package limit

import (
	"context"

	"github.com/atro-tech/aether-agent/packages/shared/pkg/featureflags"
	"github.com/atro-tech/aether-agent/packages/shared/pkg/utils"
)

func (l *Limiter) GCloudUploadLimiter() *utils.AdjustableSemaphore {
	return l.gCloudUploadLimiter
}

func (l *Limiter) GCloudMaxTasks(ctx context.Context) int {
	maxTasks := l.featureFlags.IntFlag(ctx, featureflags.GcloudMaxTasks)

	return maxTasks
}
