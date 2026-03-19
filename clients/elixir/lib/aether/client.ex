# Copyright 2024 The Æther Agent Authors
# SPDX-License-Identifier: Apache-2.0
#
# Elixir control-plane client stub for the Æther Agent ttrpc service.
# Full implementation requires an Elixir ttrpc library or a gRPC gateway bridge.

defmodule Aether.Client do
  @moduledoc """
  Control-plane client for the Æther Agent.

  Connects to the agent's ttrpc service to register add-ins,
  materialize paths, and manage proxy/credential rules.

  This is a stub. Full implementation requires either:
  - An Elixir ttrpc library (raw socket + protobuf framing)
  - A gRPC gateway sidecar that bridges to ttrpc
  """

  defstruct [:address, :connection]

  @type t :: %__MODULE__{
          address: String.t(),
          connection: any()
        }

  @doc "Connect to an Æther Agent at the given address."
  @spec connect(String.t()) :: {:ok, t()} | {:error, term()}
  def connect(address) do
    # TODO: Implement vsock or unix socket connection
    {:ok, %__MODULE__{address: address, connection: nil}}
  end

  @doc "Register an add-in package from a TOML manifest."
  @spec register_addin(t(), String.t(), String.t(), binary()) ::
          {:ok, String.t()} | {:error, term()}
  def register_addin(%__MODULE__{} = _client, _name, _version, _manifest_toml) do
    {:error, :not_implemented}
  end

  @doc "Trigger lazy materialization of a path within an add-in."
  @spec materialize_path(t(), String.t(), String.t()) ::
          {:ok, String.t()} | {:error, term()}
  def materialize_path(%__MODULE__{} = _client, _addin_id, _path) do
    {:error, :not_implemented}
  end

  @doc "Retrieve the manifest for a registered add-in."
  @spec get_manifest(t(), String.t()) :: {:ok, map()} | {:error, term()}
  def get_manifest(%__MODULE__{} = _client, _addin_id) do
    {:error, :not_implemented}
  end

  @doc "Close the connection."
  @spec close(t()) :: :ok
  def close(%__MODULE__{} = _client) do
    :ok
  end
end
