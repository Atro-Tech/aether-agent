// Copyright 2024 The Æther Agent Authors
// SPDX-License-Identifier: Apache-2.0
//
// Go control-plane client for the Æther Agent ttrpc service.
// Connects over vsock (Firecracker) or unix socket (namespace mode).

package aether

import (
	"context"
	"fmt"
	"net"
	"os"
)

// Client connects to an Æther Agent instance.
type Client struct {
	address string
	conn    net.Conn
}

// NewClient creates a new client connected to the given address.
// Address format: "unix:///path/to/socket" or "vsock://CID:PORT"
func NewClient(address string) (*Client, error) {
	// TODO: Parse address scheme and connect via ttrpc
	// For now, support unix socket only
	conn, err := net.Dial("unix", address)
	if err != nil {
		return nil, fmt.Errorf("aether: failed to connect to %s: %w", address, err)
	}

	return &Client{
		address: address,
		conn:    conn,
	}, nil
}

// RegisterAddIn registers a new add-in package with the agent.
func (c *Client) RegisterAddIn(ctx context.Context, name, version string, manifestTOML []byte) (string, error) {
	// TODO: Encode AddInRequest protobuf, send over ttrpc, decode AddInResponse
	_ = ctx
	return "", fmt.Errorf("aether: RegisterAddIn not yet implemented (ttrpc client pending)")
}

// RegisterAddInFromFile reads a manifest TOML file and registers it.
func (c *Client) RegisterAddInFromFile(ctx context.Context, name, version, manifestPath string) (string, error) {
	data, err := os.ReadFile(manifestPath)
	if err != nil {
		return "", fmt.Errorf("aether: failed to read manifest %s: %w", manifestPath, err)
	}
	return c.RegisterAddIn(ctx, name, version, data)
}

// MaterializePath triggers lazy materialization of a file within an add-in.
func (c *Client) MaterializePath(ctx context.Context, addinID, path string) (string, error) {
	// TODO: Encode MaterializeRequest, send over ttrpc, decode MaterializeResponse
	_ = ctx
	return "", fmt.Errorf("aether: MaterializePath not yet implemented")
}

// GetManifest retrieves the parsed manifest for a registered add-in.
func (c *Client) GetManifest(ctx context.Context, addinID string) (map[string]interface{}, error) {
	// TODO: Encode GetManifestRequest, send over ttrpc, decode ManifestResponse
	_ = ctx
	return nil, fmt.Errorf("aether: GetManifest not yet implemented")
}

// Close closes the connection to the agent.
func (c *Client) Close() error {
	if c.conn != nil {
		return c.conn.Close()
	}
	return nil
}
