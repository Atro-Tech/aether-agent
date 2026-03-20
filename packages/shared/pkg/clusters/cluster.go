package clusters

import (
	"github.com/google/uuid"

	"github.com/atro-tech/aether-agent/packages/shared/pkg/consts"
)

func WithClusterFallback(clusterID *uuid.UUID) uuid.UUID {
	if clusterID == nil {
		return consts.LocalClusterID
	}

	return *clusterID
}
